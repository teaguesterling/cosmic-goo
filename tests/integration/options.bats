#!/usr/bin/env bats
# `goo options <subject | =TYPE>` — the OPTIONS discovery surface (goo-protocol §7).
# Emits JSON: applicable verbs + per-verb slots (Using:/With:/object_type) —
# the single shape the compose-gui's verb-pick, completion, and (later) the daemon
# all consume. Read-only. Rust-only (exposes `Using:` channels; bash has none).
#
# Tests against the REAL shipped registry — `critique`/`think` use the `via`/`depth`
# adverbs and prove the With: projection. The no-leak guarantee (no `cmd`/`prompt`
# in the output) is the contract a daemon-as-transport would wrap.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    # Skip unless this engine ships `goo options` (bash doesn't).
    run "$GOO" options =text/markdown </dev/null
    if ! echo "$output" | grep -q '"schema_version"'; then
        skip "engine has no `goo options`"
    fi
}

@test "options: =TYPE prints a JSON view with allow + schema_version" {
    run "$GOO" options =text/markdown </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *'"schema_version": "0.1"'* ]]
    [[ "$output" == *'"stable": false'* ]]
    [[ "$output" == *'"type": "text/markdown"'* ]]
    [[ "$output" == *'"allow":'* ]]
    [[ "$output" == *'"verbs":'* ]]
}

@test "options: with-slots populate from a verb's uses_adverbs (critique/via)" {
    run "$GOO" options =text/markdown </dev/null
    [ "$status" -eq 0 ]
    # critique → uses `via` (a selector adverb with real choices in claude-routing.toml)
    [[ "$output" == *'"critique"'* ]]
    [[ "$output" == *'"via"'* ]]
    [[ "$output" == *'"kind": "selector"'* ]]
    [[ "$output" == *'"clipboard"'* ]]   # one of the via values
    [[ "$output" == *'"fabric"'* ]]      # another via value
}

@test "options: think exposes BOTH via and depth (multi-adverb verb)" {
    run "$GOO" options =text/markdown </dev/null
    [ "$status" -eq 0 ]
    # `think` uses_adverbs = ["via", "depth"]; both must appear in its `with`.
    [[ "$output" == *'"think"'* ]]
    [[ "$output" == *'"depth"'* ]]
    [[ "$output" == *'"ultra"'* ]]       # a depth selector value
}

# The projection guarantee: OPTIONS surfaces never leak verb internals (cmd,
# prompt, description). This is the contract the daemon-as-transport will wrap;
# it must hold against the real registry, not just the unit-test fixture.
@test "options: never leaks verb internals (no cmd / prompt in the output)" {
    run "$GOO" options =text/markdown </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" != *'"cmd"'* ]]
    [[ "$output" != *'"prompt"'* ]]
    [[ "$output" != *'"description"'* ]]
}

# Image subject → image verbs only; the text verbs are absent from `allow`.
@test "options: allow is type-scoped (image subject → image verbs)" {
    run "$GOO" options =image/png </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" != *'"critique"'* ]]    # text-only, not for images
    [[ "$output" != *'"trim"'* ]]
}

@test "options: missing subject fails cleanly" {
    run "$GOO" options </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"usage:"* ]]
}
