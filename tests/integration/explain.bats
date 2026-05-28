#!/usr/bin/env bats
# `goo --explain` — the negotiation plan explainer (goo-debug), against the REAL
# shipped registry (presentation.toml channels + view verb; content.toml's
# application/json is_a text/plain). Read-only: shows the Accept profile and the
# planned route or a 415.
#
# Rust-bin only (bash bin/goo has no --explain). setup() auto-skips on any engine
# without it, so `make test` (bash) skips cleanly.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    # Skip unless this engine has --explain (bash doesn't).
    run "$GOO" --explain view @image/png --explain-env tty </dev/null
    if ! echo "$output" | grep -q "Accept:"; then
        skip "engine has no --explain"
    fi
}

@test "explain: view image on a tty → chafa (image→ansi)" {
    run "$GOO" --explain view @image/png --explain-env tty </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"Accept: text/x-ansi"* ]]
    [[ "$output" == *"chafa"* ]]
    [[ "$output" == *"text/x-ansi"* ]]
}

@test "explain: view image on a desktop → eog (image→surface)" {
    run "$GOO" --explain view @image/png --explain-env desktop </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"eog"* ]]
    [[ "$output" == *"surface"* ]]
}

@test "explain: view image piped → raw bytes (identity)" {
    run "$GOO" --explain view @image/png --explain-env piped </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"(cost 0)"* ]]
}

@test "explain: view a JSON subject → 415 (view doesn't accept it, no route)" {
    run "$GOO" --explain view @application/json --explain-env tty </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]
}

@test "explain: input coercion — json-keys on a CSV → csv2json first" {
    run "$GOO" --explain json-keys @text/csv --explain-env tty </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"csv2json"* ]]
    [[ "$output" == *"application/json"* ]]
}

@test "explain: --as pins the Accept (image as bytes on a tty)" {
    run "$GOO" --explain view @image/png --explain-env tty --as image/png </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"Accept: image/png"* ]]
    [[ "$output" == *"(cost 0)"* ]]   # identity: view emits image/png, already accepted
}

@test "explain: unknown verb fails cleanly" {
    run "$GOO" --explain nope @image/png --explain-env tty </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown verb"* ]]
}

# --- slice 5: the subject line annotates the signal source ---
@test "explain: @type subject is annotated 'via explicit'" {
    run "$GOO" --explain view @image/png --explain-env tty </dev/null
    [[ "$output" == *"subject: image/png (via explicit)"* ]]
}

@test "explain: a JSON literal is annotated 'via checker'" {
    run "$GOO" --explain json-keys '{"k":1}' --explain-env piped </dev/null
    [[ "$output" == *"subject: application/json (via checker)"* ]]
}

@test "explain: a bare word is annotated 'via content'" {
    run "$GOO" --explain upper 'hello there' --explain-env piped </dev/null
    [[ "$output" == *"(via content)"* ]]
}

@test "explain: a file with no declared extension is annotated 'via libmagic'" {
    printf 'plain text body\n' > "$BATS_TEST_TMPDIR/s.txt"
    run "$GOO" --explain upper "$BATS_TEST_TMPDIR/s.txt" --explain-env piped </dev/null
    [[ "$output" == *"(via libmagic)"* ]]
}
