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

# --- rich rendering: cost by color/marker, not inline ':cheap' noise ---
@test "explain: a lossy edge is marked '(lossy)', no inline ':cheap'" {
    run "$GOO" --explain view @image/png --explain-env tty </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"chafa (lossy)"* ]]   # the edge that matters is flagged
    [[ "$output" != *": cheap"* ]]         # cheap/normal tiers dropped from the line
}

# --- detail modes: --explain-with route|steps|shell + adaptive default ---
@test "explain: --explain-with steps lists numbered steps with the cmd template" {
    run "$GOO" --explain json-keys @text/csv --explain-env tty --explain-with steps </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"1."* ]]              # numbered
    [[ "$output" == *"csv2json"* ]]        # the coercion step
    [[ "$output" == *"{in.path"* ]]        # the literal cmd template (plumbing visible)
}

@test "explain: --explain-with shell shows the commands block" {
    run "$GOO" --explain view @image/png --explain-env tty --explain-with shell </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"commands"* ]]
    [[ "$output" == *"chafa"* ]]
}

@test "explain: --explain-with route is the one-liner only (no detail block)" {
    run "$GOO" --explain view @image/png --explain-env tty --explain-with route </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"text/x-ansi"* ]]     # the route line is still there
    [[ "$output" != *"commands"* ]]        # …but no shell block
    [[ "$output" != *"1."* ]]              # …and no steps block
}

@test "explain: adaptive default shows a detail block for a simple route" {
    run "$GOO" --explain view @image/png --explain-env tty </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"commands"* ]]        # ≤2 hops → the shell block by default
}

@test "explain: an unknown --explain-with mode fails cleanly" {
    run "$GOO" --explain view @image/png --explain-env tty --explain-with bogus </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown --explain-with"* ]]
}

# --- --paths: enumerate all routes A→B (§4.2) ---
@test "explain: --paths lists multiple routes (image on cosmic → chafa + eog)" {
    run "$GOO" --explain view @image/png --explain-env cosmic --paths </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"route(s)"* ]]
    [[ "$output" == *"chafa"* ]]   # image → ansi
    [[ "$output" == *"eog"* ]]     # image → surface — a distinct route
}

@test "explain: --paths --max-hops bounds the depth" {
    # ≤1 hop keeps the direct chafa/eog routes, drops the chafa→cosmic-edit chain.
    run "$GOO" --explain view @image/png --explain-env cosmic --paths --max-hops 1 </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" != *"cosmic-edit"* ]]   # the 2-hop route is pruned
}

@test "explain: --paths --format mermaid emits a graph LR with shared nodes" {
    run "$GOO" --explain view @image/png --explain-env cosmic --paths --format mermaid </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"graph LR"* ]]
    [[ "$output" == *"-->"* ]]
    [[ "$output" == *"chafa"* ]]
}

@test "explain: --paths with no route → 415" {
    run "$GOO" --explain view @application/json --explain-env tty --paths </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]
}

@test "explain: --paths --format bogus fails cleanly" {
    run "$GOO" --explain view @image/png --explain-env cosmic --paths --format bogus </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown --format"* ]]
}
