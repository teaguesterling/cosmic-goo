#!/usr/bin/env bats
# Context-sensitive content inference: a bare positional with no path/scheme
# is typed by *structural* inference, re-ranked by the verb's `accepts`. The
# motivating case is a raw JSON literal reaching a verb that accepts
# application/json — `detect_content` alone returns text/plain for it.
#
# This is Rust-only (the bash bin/goo types bare positionals via detect_content
# + glob accepts, with no JSON-shape signal). setup() auto-skips on any engine
# without it, so `make test` (bash) skips cleanly and the Rust bin exercises it.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-infer.toml" <<'EOF'
name = "test-infer"

[[types]]
name = "application/json"
display = "JSON"
kind = "content"

# Accepts ONLY application/json — never text/*. So it's reachable from a bare
# positional only when inference types the content as JSON. Echoes the resolved
# type so the test can assert what inference picked.
[[verbs]]
name = "eat-json"
accepts = ["application/json"]
cmd = "printf '%s' {subject.type|q}"

# A plain text verb, for the parity-direction assertion: a JSON literal handed
# to a text-only verb must still type as text/plain (json-shape only wins when
# the verb actually accepts json).
[[verbs]]
name = "eat-text"
accepts = ["text/*"]
cmd = "printf '%s' {subject.type|q}"
EOF

    # Skip unless this engine does JSON-shape inference (bash bin/goo doesn't).
    # (Use `if`, not `&&` — a false `[[ ]]` as setup's last line fails setup.)
    run "$GOO" eat-json '{"k":1}' </dev/null
    if [ "$status" -ne 0 ]; then
        skip "engine has no JSON-shape content inference"
    fi
}

@test "infer: a JSON literal types as application/json for a json-accepting verb" {
    run "$GOO" eat-json '{"k":1}' </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "application/json" ]
}

@test "infer: a JSON array literal infers application/json" {
    run "$GOO" eat-json '[1,2,3]' </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "application/json" ]
}

@test "infer: a JSON document on stdin infers application/json" {
    # The implicit chain (no positional) infers structure from stdin too.
    run bash -c "printf '%s' '{\"k\":1}' | '$GOO' eat-json"
    [ "$status" -eq 0 ]
    [ "$output" = "application/json" ]
}

@test "infer: a non-JSON literal does not resolve for a json-only verb" {
    # Inference is selective — it only fires on a positive json signal, so a
    # plain word never reaches a json-only verb.
    run "$GOO" eat-json 'just some words' </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"could not resolve"* ]]
}

# Note on the parity direction (a text verb never gets json *via inference*):
# that's covered by the engine unit test `infer_for_declines_json_for_a_text_only_verb`.
# It can't be shown end-to-end here because libmagic already types every JSON
# literal as application/json in detect_content's fallback — so `eat-text` on a
# JSON literal is governed by detect_content, not by infer_for, on this system.

@test "infer: a bare word is unaffected (text verb still gets text/plain)" {
    run "$GOO" eat-text 'hello' </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "text/plain" ]
}
