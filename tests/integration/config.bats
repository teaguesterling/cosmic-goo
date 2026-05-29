#!/usr/bin/env bats
# `-c`/`--config <path>`: merge an ad-hoc config (extra plugin file/dir) LAST,
# so it extends/overrides the shipped plugins for one invocation — without
# touching the install. Threaded to the registry via COSMIC_GOO_EXTRA_CONFIG.
#
# Rust-only (parsed in the Rust `main`; bash bin/goo doesn't know `-c`).
# setup() auto-skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    EXTRA="$BATS_TEST_TMPDIR/extra.toml"
    cat > "$EXTRA" <<'EOF'
name = "extra"
[[verbs]]
name = "gconf-echo"
accepts = ["text/*"]
cmd = "printf '%s' {subject.text|q}"
EOF

    # Skip unless this engine supports -c (bash doesn't parse it).
    run "$GOO" -c "$EXTRA" gconf-echo probe </dev/null
    if [ "$output" != "probe" ]; then
        skip "engine has no -c/--config"
    fi
}

@test "config: -c <file> loads the extra config's verb" {
    run "$GOO" -c "$EXTRA" gconf-echo "hello config" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hello config" ]
}

@test "config: --config=<file> form works too" {
    run "$GOO" --config="$EXTRA" gconf-echo "eq form" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "eq form" ]
}

@test "config: without -c the verb is absent (proves it's the config adding it)" {
    run "$GOO" gconf-echo "x" </dev/null
    [ "$status" -ne 0 ]
}

@test "config: -c accepts a directory of *.toml" {
    local dir="$BATS_TEST_TMPDIR/cfgdir"
    mkdir -p "$dir"
    cp "$EXTRA" "$dir/e.toml"
    run "$GOO" -c "$dir" gconf-echo "from a dir" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "from a dir" ]
}
