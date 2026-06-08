#!/usr/bin/env bats
# The woollama inference route: `--via=woollama` POSTs the rendered prompt to the
# local woollama router (~/Projects/woollama) over its Unix socket and prints the
# model's reply. This locks the REAL plugins/claude-routing.toml route template
# hermetically — a stub `curl` stands in for the daemon, so the route's own
# `jq -n` body construction and content/error extraction run for real against
# the actual template, and a real socket node satisfies the route's `test -S`
# guard. No live woollama needed; assertions are HARD so a route-template typo
# (a future broken `jq` filter → non-zero / wrong output) fails the gate.
#
# Verified to pass on BOTH engines (the route is plain shell + the standard
# adverb system), so there is no engine-skip. A live round-trip is opt-in via
# GOO_WOOLLAMA_LIVE=1 (the last test), so a bare `make test` makes no LLM calls.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    # Load the REAL plugins — the actual woollama route + `model` adverb live in
    # plugins/claude-routing.toml; summarize lives in plugins/text-verbs.toml.
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME" "$BATS_TEST_TMPDIR/bin"

    # The route guards on `test -S $XDG_RUNTIME_DIR/woollama.sock`; bind a real
    # socket node so the guard passes (the stub curl never actually connects).
    command -v python3 >/dev/null || skip "python3 needed to mint a socket node"
    python3 -c 'import socket,sys; socket.socket(socket.AF_UNIX).bind(sys.argv[1])' \
        "$XDG_RUNTIME_DIR/woollama.sock"

    # Stub curl: ignore all args, emit the canned JSON the test sets. Real jq is
    # used by the route for BOTH the request-body construction and the response
    # extraction, so the template's quoting/escaping is exercised for real.
    cat > "$BATS_TEST_TMPDIR/bin/curl" <<'STUB'
#!/usr/bin/env bash
printf '%s' "${STUB_WOOLLAMA_JSON:-}"
STUB
    chmod +x "$BATS_TEST_TMPDIR/bin/curl"
    export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
}

@test "via=woollama prints the model's reply content" {
    export STUB_WOOLLAMA_JSON='{"choices":[{"message":{"content":"STUB-SUMMARY"}}]}'
    run "$GOO" summarize "The mitochondria is the powerhouse of the cell." --via=woollama </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"STUB-SUMMARY"* ]]
}

@test "via=woollama survives an injection-hostile prompt (jq -n body is safe)" {
    # A prompt full of quotes/newlines/$() must reach woollama as one JSON string
    # without breaking the shell or the body — proves the `jq -n --arg` path.
    export STUB_WOOLLAMA_JSON='{"choices":[{"message":{"content":"SAFE"}}]}'
    run "$GOO" summarize 'he said "hi"; rm -rf / $(touch pwned) `id`' --via=woollama </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"SAFE"* ]]
    [ ! -e pwned ]
}

@test "via=woollama surfaces a woollama error AND exits non-zero" {
    export STUB_WOOLLAMA_JSON='{"error":{"message":"unknown model namespace"}}'
    run "$GOO" summarize "x" --via=woollama </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"woollama: unknown model namespace"* ]]
}

@test "via=woollama errors clearly when the daemon socket is absent" {
    rm -f "$XDG_RUNTIME_DIR/woollama.sock"
    run "$GOO" summarize "x" --via=woollama </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"woollama not running"* ]]
}

# Opt-in live round-trip — only with GOO_WOOLLAMA_LIVE=1 (so a bare `make test`
# never makes an LLM call). Mirrors woollama's own `-m integration` convention.
@test "via=woollama round-trips against a live woollama (opt-in)" {
    [ -n "$GOO_WOOLLAMA_LIVE" ] || skip "set GOO_WOOLLAMA_LIVE=1 for the live round-trip"
    local live="${GOO_WOOLLAMA_SOCK:-/run/user/$(id -u)/woollama.sock}"
    [ -S "$live" ] || skip "no live woollama socket at $live"
    # Point the route at the live socket and drop the curl stub for this test.
    export XDG_RUNTIME_DIR="$(dirname "$live")"
    export PATH="${PATH#"$BATS_TEST_TMPDIR/bin:"}"
    run "$GOO" summarize "Cats are small carnivorous mammals kept as pets." --via=woollama --model=fast </dev/null
    [ "$status" -eq 0 ]
    [ -n "$output" ]
    [[ "$output" != *"woollama not running"* ]]
}
