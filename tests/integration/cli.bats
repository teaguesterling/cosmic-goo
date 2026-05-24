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
cmd = "printf '%s' {subject.text|q}"

[[verbs]]
name = "wrap"
accepts = ["text/*"]
uses_adverbs = ["via"]
prompt = "WRAPPED:{subject.text}:END"

[[verbs]]
name = "name-of"
accepts = ["application/vnd.test.gadget"]
cmd = "printf '%s' {subject.id|q}"
EOF

    # A handle source + a custom sigil, for addressing tests.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-gadgets.toml" <<'EOF'
name = "test-gadgets"

[[sources]]
name = "gadgets"
prefix = "gad"
emits = "application/vnd.test.gadget"
list_cmd = "echo '[{\"id\":\"sprocket\",\"title\":\"Sprocket\"},{\"id\":\"cog\",\"title\":\"Cog\"}]'"

[[sigils]]
char = "%"
expands = ":gad:"
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

@test "goo compose cancels cleanly with an empty pick" {
    # Drive the picker with an empty answer (= cancel at the subject step).
    local ans="$BATS_TEST_TMPDIR/ans"
    printf '\n' > "$ans"
    GOO_COMPOSE_ANSWERS="$ans" run "$GOO" compose
    [ "$status" -eq 130 ]
    [[ "$output" =~ "cancelled" ]]
}

# ---------------- addressing through the CLI ----------------

@test "goo VERB reads piped stdin when no positional given" {
    run bash -c 'printf "%s" "from a pipe" | "$0" echo-back' "$GOO"
    [ "$status" -eq 0 ]
    [ "$output" = "from a pipe" ]
}

@test "goo VERB with a positional ignores stdin" {
    run bash -c 'printf "%s" "PIPED" | "$0" echo-back "explicit"' "$GOO"
    [ "$status" -eq 0 ]
    [ "$output" = "explicit" ]
}

@test "goo VERB reads a native file path (contents, not the path)" {
    printf 'file body' > "$BATS_TEST_TMPDIR/note.txt"
    run "$GOO" echo-back "$BATS_TEST_TMPDIR/note.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "file body" ]
}

@test "goo VERB errors on a missing native file path" {
    run "$GOO" echo-back "$BATS_TEST_TMPDIR/does-not-exist.txt" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no such file" ]]
}

@test "goo HANDLE-VERB resolves :source:item" {
    run "$GOO" name-of ":gad:sprocket" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "sprocket" ]
}

@test "goo HANDLE-VERB resolves a custom sigil (% -> :gad:)" {
    run "$GOO" name-of "%cog" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "cog" ]
}

@test "goo HANDLE-VERB resolves a bare positional via type search" {
    run "$GOO" name-of "sprocket" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "sprocket" ]
}

@test "goo list emits a source's raw JSON" {
    run "$GOO" list gadgets </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | jq -e 'map(.id) | contains(["sprocket","cog"])' >/dev/null
}
