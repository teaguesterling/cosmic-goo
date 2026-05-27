#!/usr/bin/env bats
# Tests for lib/plugin-loader.sh.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # shellcheck source=../lib/plugin-loader.sh
    . "$REPO_ROOT/lib/plugin-loader.sh"

    # Isolate this test run from the real filesystem by pointing all four
    # discovery dirs at empty paths and then overriding the built-in dir
    # to our fixture. XDG_RUNTIME_DIR isolates the registry cache.
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg-config"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    cd "$BATS_TEST_TMPDIR" || return 1   # PWD-based dir won't accidentally match
}

# ---------------- plugin_dirs ----------------

@test "plugin_dirs lists four dirs in expected order" {
    run plugin_dirs
    [ "$status" -eq 0 ]
    lines=( $(plugin_dirs) )
    [ "${lines[0]}" = "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" ]
    [ "${lines[1]}" = "/etc/cosmic-goo/plugins" ]
    [ "${lines[2]}" = "$XDG_CONFIG_HOME/cosmic-goo/plugins" ]
    [ "${lines[3]}" = "$PWD/.cosmic-goo/plugins" ]
}

# ---------------- plugin_discover ----------------

@test "plugin_discover returns empty when no plugins exist" {
    run plugin_discover
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

@test "plugin_discover finds single-file plugins" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/foo.toml" <<'EOF'
name = "foo"
EOF
    run plugin_discover
    [ "$status" -eq 0 ]
    [[ "$output" =~ foo.toml ]]
}

@test "plugin_discover finds directory plugins" {
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bar"
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bar/plugin.toml" <<'EOF'
name = "bar"
EOF
    run plugin_discover
    [ "$status" -eq 0 ]
    [[ "$output" =~ bar/plugin.toml ]]
}

@test "plugin_discover finds both forms in order" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/aaa.toml" <<'EOF'
name = "aaa"
EOF
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bbb"
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bbb/plugin.toml" <<'EOF'
name = "bbb"
EOF
    run plugin_discover
    [ "$status" -eq 0 ]
    # Two lines, one per plugin
    [ "$(echo "$output" | wc -l)" -eq 2 ]
}

# ---------------- plugin_load ----------------

@test "plugin_load parses a simple plugin and tags items" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/tmux.toml" <<'EOF'
name = "tmux"
description = "tmux session source"

[[verbs]]
name = "switch"
accepts = ["application/vnd.tmux-use.session"]
cmd = "tmux-use switch {subject.id}"
EOF
    run plugin_load "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/tmux.toml"
    [ "$status" -eq 0 ]
    # Output is JSON with the verbs array, each tagged with _plugin
    echo "$output" | jq -e '.plugins[0].name == "tmux"' >/dev/null
    echo "$output" | jq -e '.verbs[0].name == "switch"' >/dev/null
    echo "$output" | jq -e '.verbs[0]._plugin == "tmux"' >/dev/null
    echo "$output" | jq -e '.verbs[0]._plugin_dir | endswith("builtin")' >/dev/null
}

@test "plugin_load falls back to filename when name field missing" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/anonymous.toml" <<'EOF'
description = "no name field"
EOF
    run plugin_load "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/anonymous.toml"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.plugins[0].name == "anonymous"' >/dev/null
}

@test "plugin_load handles plugins contributing all four item kinds" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/multi.toml" <<'EOF'
name = "multi"

[[types]]
name = "application/vnd.multi.thing"
display = "thing"
kind = "handle"

[[sources]]
name = "things"
emits = "application/vnd.multi.thing"
list_cmd = "echo []"

[[verbs]]
name = "do-thing"
accepts = ["application/vnd.multi.thing"]
cmd = "true"

[[adverbs]]
name = "mode"
kind = "selector"
default = "fast"
EOF
    run plugin_load "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/multi.toml"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.types  | length == 1' >/dev/null
    echo "$output" | jq -e '.sources| length == 1' >/dev/null
    echo "$output" | jq -e '.verbs  | length == 1' >/dev/null
    echo "$output" | jq -e '.adverbs| length == 1' >/dev/null
}

# Parity guard for the negotiation [[channels]] collection: the bash loader must
# pass it through with provenance, byte-identically to the Rust engine's
# registry::channels (asserted by the same fixture + assertions in
# crates/goo-engine/src/registry.rs `channels_pass_through_with_provenance`).
@test "plugin_load passes [[channels]] through with provenance" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/chtest.toml" <<'EOF'
name = "chtest"

[[channels]]
name = "chafa"
accepts = ["image/*"]
emits = "text/x-ansi"
cost = "lossy"
cmd = "chafa {in.path|q}"
EOF
    run plugin_load "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/chtest.toml"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.channels | length == 1' >/dev/null
    echo "$output" | jq -e '.channels[0].name == "chafa"' >/dev/null
    echo "$output" | jq -e '.channels[0].emits == "text/x-ansi"' >/dev/null
    echo "$output" | jq -e '.channels[0]._plugin == "chtest"' >/dev/null
}

@test "plugin_load fails clearly on missing file" {
    run plugin_load "$BATS_TEST_TMPDIR/does-not-exist.toml"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not a file" ]]
}

# ---------------- plugin_load_all ----------------

@test "plugin_load_all returns empty registry shape when no plugins" {
    run plugin_load_all
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.plugins == [] and .types == [] and .sources == [] and .verbs == [] and .adverbs == []' >/dev/null
}

@test "plugin_load_all collates verbs from multiple plugins" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/a.toml" <<'EOF'
name = "a"
[[verbs]]
name = "alpha"
accepts = ["text/*"]
cmd = "true"
EOF
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/b.toml" <<'EOF'
name = "b"
[[verbs]]
name = "beta"
accepts = ["text/*"]
cmd = "true"
EOF
    run plugin_load_all
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.verbs | length == 2' >/dev/null
    echo "$output" | jq -e '.verbs | map(.name) | contains(["alpha", "beta"])' >/dev/null
}

@test "plugin_load_all: user dir overrides built-in by name" {
    # Built-in defines a verb
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/text-verbs.toml" <<'EOF'
name = "text-verbs"
[[verbs]]
name = "critique"
accepts = ["text/*"]
cmd = "echo from-builtin"
EOF
    # User overrides with same plugin name and same verb name
    mkdir -p "$XDG_CONFIG_HOME/cosmic-goo/plugins"
    cat > "$XDG_CONFIG_HOME/cosmic-goo/plugins/text-verbs.toml" <<'EOF'
name = "text-verbs"
[[verbs]]
name = "critique"
accepts = ["text/*"]
cmd = "echo from-user"
EOF
    run plugin_load_all
    [ "$status" -eq 0 ]
    # The user override should win for the `critique` verb
    cmd=$(echo "$output" | jq -r '.verbs[] | select(.name=="critique") | .cmd')
    [ "$cmd" = "echo from-user" ]
}

@test "plugin_load_all skips a malformed plugin and continues" {
    # Good plugin
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/good.toml" <<'EOF'
name = "good"
[[verbs]]
name = "alpha"
accepts = ["text/*"]
cmd = "true"
EOF
    # Malformed plugin (invalid TOML — unclosed bracket)
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bad.toml" <<'EOF'
name = "bad
EOF
    # Stderr will carry an intentional warning about bad.toml; only stdout
    # (the registry JSON) is the thing we want to inspect.
    out=$(plugin_load_all 2>/dev/null)
    echo "$out" | jq -e '.verbs | map(.name) | contains(["alpha"])' >/dev/null
}

@test "plugin_registry_export is a working alias of plugin_load_all" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/p.toml" <<'EOF'
name = "p"
EOF
    a=$(plugin_load_all)
    b=$(plugin_registry_export)
    [ "$a" = "$b" ]
}
