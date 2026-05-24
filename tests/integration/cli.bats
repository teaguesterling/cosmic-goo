#!/usr/bin/env bats
# Integration tests: drive bin/goo end-to-end against a fixture plugin set.
# Uses a "dump" route that writes to a temp file instead of touching the real
# clipboard, so the test suite is hermetic.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="$REPO_ROOT/bin/goo"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    DUMP_FILE="$BATS_TEST_TMPDIR/dump.out"
    export DUMP_FILE

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-routes.toml" <<EOF
name = "test-routes"

[[adverbs]]
name = "via"
kind = "selector"
applies_to = ["text/*"]
default = "dump"

[adverbs.values.dump]
template = "printf '%s' '{verb.prompt}' > '$DUMP_FILE'"
EOF

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-verbs.toml" <<'EOF'
name = "test-verbs"

[[verbs]]
name = "echo-back"
accepts = ["text/*"]
# Phase-1 template authors must quote substitutions to be whitespace-safe.
cmd = "printf '%s' '{subject.text}'"

[[verbs]]
name = "wrap"
accepts = ["text/*"]
uses_adverbs = ["via"]
prompt = "WRAPPED:{subject.text}:END"
EOF

    cd "$BATS_TEST_TMPDIR" || return 1
}

@test "goo --help prints usage" {
    run "$GOO" --help
    [ "$status" -eq 0 ]
    [[ "$output" =~ "Grammar Of Operations" ]]
    [[ "$output" =~ "USAGE" ]]
}

@test "goo plugins lists loaded fixtures" {
    run "$GOO" plugins
    [ "$status" -eq 0 ]
    [[ "$output" =~ "test-routes" ]]
    [[ "$output" =~ "test-verbs" ]]
}

@test "goo validate accepts well-formed fixture" {
    run "$GOO" validate
    [ "$status" -eq 0 ]
    [[ "$output" =~ "OK" ]]
}

@test "goo describe shows verb details" {
    run "$GOO" describe wrap
    [ "$status" -eq 0 ]
    [[ "$output" =~ "verb: wrap" ]]
    [[ "$output" =~ "accepts: text/*" ]]
    [[ "$output" =~ "uses_adverbs: via" ]]
}

@test "goo describe unknown verb fails cleanly" {
    run "$GOO" describe nonexistent
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no verb named" ]]
}

@test "goo <unknown> reports as unknown verb" {
    run "$GOO" definitely-not-a-verb hello
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
}

@test "goo VERB POSITIONAL executes with text subject" {
    run "$GOO" echo-back "hello goo"
    [ "$status" -eq 0 ]
    [ "$output" = "hello goo" ]
}

@test "goo VERB renders prompt through the via adverb route" {
    run "$GOO" wrap "important text" --via=dump
    [ "$status" -eq 0 ]
    [ -f "$DUMP_FILE" ]
    [ "$(cat "$DUMP_FILE")" = "WRAPPED:important text:END" ]
}

@test "goo VERB uses the adverb's default when --via is omitted" {
    run "$GOO" wrap "default route text"
    [ "$status" -eq 0 ]
    [ -f "$DUMP_FILE" ]
    [ "$(cat "$DUMP_FILE")" = "WRAPPED:default route text:END" ]
}

@test "goo compose prints stub message" {
    run "$GOO" compose
    [ "$status" -ne 0 ]
    [[ "$output" =~ "Phase 4 feature" ]]
}
