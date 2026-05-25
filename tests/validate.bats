#!/usr/bin/env bats
# Failure-case tests for `goo validate`. Each writes a deliberately-broken
# plugin and asserts validate rejects it with a useful message.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
}

fresh() { rm -f "$XDG_RUNTIME_DIR/cosmic-goo/registry.json"; }

@test "validate: clean plugin passes" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/ok.toml" <<'EOF'
name = "ok"
[[verbs]]
name = "go"
accepts = ["text/*"]
cmd = "true"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "OK" ]]
}

@test "validate: empty accept pattern is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bad.toml" <<'EOF'
name = "bad"
[[verbs]]
name = "oops"
accepts = [""]
cmd = "true"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "empty accept pattern" ]]
}

@test "validate: empty accepts array is allowed (no-subject verb)" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/ns.toml" <<'EOF'
name = "ns"
[[verbs]]
name = "ping"
accepts = []
cmd = "true"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
}

@test "validate: adverb without scope is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/adv.toml" <<'EOF'
name = "adv"
[[adverbs]]
name = "loose"
kind = "selector"
[adverbs.values.x]
template_var = { a = "b" }
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "neither applies_to nor applies_to_verbs" ]]
}

@test "validate: selector adverb with no values is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/sel.toml" <<'EOF'
name = "sel"
[[adverbs]]
name = "empty"
kind = "selector"
applies_to = ["text/*"]
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "has no values" ]]
}

@test "validate: multi-char sigil is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/sig.toml" <<'EOF'
name = "sig"
[[sigils]]
char = "ab"
expands = ":app:"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "exactly one character" ]]
}

@test "validate: sigil colliding with reserved prefix is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/sig.toml" <<'EOF'
name = "sig"
[[sigils]]
char = "/"
expands = ":app:"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "collides" ]]
}

@test "validate: sigil with no expansion is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/sig.toml" <<'EOF'
name = "sig"
[[sigils]]
char = "@"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no expansion" ]]
}

@test "validate: a well-formed custom sigil passes" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/sig.toml" <<'EOF'
name = "sig"
[[sigils]]
char = "@"
expands = ":app:"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
}

@test "validate: alias with no expansion is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/al.toml" <<'EOF'
name = "al"
[[aliases]]
name = "g"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no expansion" ]]
}

@test "validate: alias shadowing a reserved subcommand is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/al.toml" <<'EOF'
name = "al"
[[aliases]]
name = "list"
expands = "echo hi"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "reserved subcommand" ]]
}

@test "validate: alias shadowing a verb passes but warns" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/al.toml" <<'EOF'
name = "al"
[[verbs]]
name = "go"
accepts = ["text/*"]
cmd = "true"
[[aliases]]
name = "go"
expands = "go --fast"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "shadows a verb" ]]
}

@test "validate: a well-formed alias passes" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/al.toml" <<'EOF'
name = "al"
[[verbs]]
name = "search"
accepts = ["text/*"]
cmd = "true"
[[aliases]]
name = "g"
expands = "search --engine=google"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "1 aliases" ]]
}

@test "validate: dispatch rule with no matches pattern is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/dsp.toml" <<'EOF'
name = "dsp"
[[verbs]]
name = "go"
accepts = ["text/*"]
cmd = "true"
[[dispatch]]
verb = "go"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no \"matches\"" ]]
}

@test "validate: dispatch rule with no verb is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/dsp.toml" <<'EOF'
name = "dsp"
[[dispatch]]
matches = "foo"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no \"verb\"" ]]
}

@test "validate: dispatch rule routing to an unknown verb is rejected" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/dsp.toml" <<'EOF'
name = "dsp"
[[dispatch]]
matches = "foo"
verb = "ghost"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
}

@test "validate: a well-formed dispatch rule passes" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/dsp.toml" <<'EOF'
name = "dsp"
[[verbs]]
name = "open-url"
accepts = ["text/x-uri"]
cmd = "true"
[[dispatch]]
matches = 'RFC ([0-9]+)'
type = "text/x-uri"
set = { text = "https://rfc/${1}" }
verb = "open-url"
EOF
    fresh
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "1 dispatch" ]]
}
