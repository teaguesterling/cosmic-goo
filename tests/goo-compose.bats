#!/usr/bin/env bats
# goo-compose (v0) — the picker-driven sentence builder. Driven non-interactively
# via GOO_COMPOSE_ANSWERS, with a shim `goo` (GOO_BIN) that records its argv
# instead of executing — so we assert exactly the `goo …` invocation compose
# builds. Gated on GOO_COMPOSE_BIN (the goo-compose release binary):
#
#   GOO_COMPOSE_BIN=$PWD/crates/target/release/goo-compose bats tests/goo-compose.bats
#
# `make test` (no GOO_COMPOSE_BIN) skips this cleanly.

setup() {
    [ -n "${GOO_COMPOSE_BIN:-}" ] || skip "set GOO_COMPOSE_BIN to the goo-compose binary"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-gc.toml" <<'EOF'
name = "test-gc"

[[sources]]
name = "gadgets"
prefix = "gad"
emits = "application/vnd.test.gadget"
list_cmd = "echo '[{\"id\":\"sprocket\",\"title\":\"Sprocket\"},{\"id\":\"cog\",\"title\":\"Cog\"}]'"

[[verbs]]
name = "name-of"
accepts = ["application/vnd.test.gadget"]
cmd = "true"

[[verbs]]
name = "wrap"
accepts = ["text/*"]
uses_adverbs = ["via"]
prompt = "W"

[[adverbs]]
name = "via"
kind = "selector"
applies_to = ["text/*"]
default = "dump"
[adverbs.values.dump]
template = "true"
EOF

    # Shim `goo`: record argv, don't execute. compose execs $GOO_BIN.
    REC="$BATS_TEST_TMPDIR/argv"
    cat > "$BATS_TEST_TMPDIR/goo" <<EOF
#!/bin/sh
printf '%s' "\$*" > "$REC"
EOF
    chmod +x "$BATS_TEST_TMPDIR/goo"
    export GOO_BIN="$BATS_TEST_TMPDIR/goo"
    cd "$BATS_TEST_TMPDIR" || return 1
}

# Write one answer per line and run goo-compose.
compose_with() {
    local ans="$BATS_TEST_TMPDIR/answers"
    printf '%s\n' "$@" > "$ans"
    GOO_COMPOSE_ANSWERS="$ans" run "$GOO_COMPOSE_BIN"
}

@test "goo-compose: subject + verb execs goo <verb> <address>" {
    compose_with "goo://gad/sprocket" "name-of" "yes"
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/argv")" = "name-of goo://gad/sprocket" ]
}

@test "goo-compose: an adverb value is carried into the exec" {
    compose_with "goo://text/hello" "wrap" "dump" "yes"
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/argv")" = "wrap goo://text/hello --via=dump" ]
}

@test "goo-compose: empty subject pick cancels (130, nothing run)" {
    compose_with ""
    [ "$status" -eq 130 ]
    [ ! -e "$BATS_TEST_TMPDIR/argv" ]
}

@test "goo-compose: confirm=no cancels (130, nothing run)" {
    compose_with "goo://gad/sprocket" "name-of" "no"
    [ "$status" -eq 130 ]
    [ ! -e "$BATS_TEST_TMPDIR/argv" ]
}
