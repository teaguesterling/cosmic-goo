#!/usr/bin/env bats
# Action history + `goo again` / `goo forget` (data-entry-ux §6.1, roadmap #13).
# A successful verb run is recorded (verb + subject TYPE + selector adverbs —
# never subject content) to $XDG_STATE_HOME/cosmic-goo/history.jsonl. `goo again
# [subject]` repeats the last verb (+adverbs) on a new subject; `goo forget`
# clears the history; GOO_NO_HISTORY=1 disables recording. Recording is on by
# default. Rust-only; auto-skips on the bash legacy engine.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    export XDG_STATE_HOME="$BATS_TEST_TMPDIR/state"   # hermetic: never touch ~/.local/state
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" \
             "$XDG_STATE_HOME" "$HOME"
    HIST="$XDG_STATE_HOME/cosmic-goo/history.jsonl"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/again.toml" <<'EOF'
name = "againfix"

# Echoes the subject text — lets us see the verb ran on the NEW subject.
[[verbs]]
name = "echotext"
accepts = ["text/*"]
cmd = "printf '%s' {subject.text|q}"

# Echoes a tone template_var — lets us see a selector adverb was replayed.
[[verbs]]
name = "shout"
accepts = ["text/*"]
uses_adverbs = ["tone"]
cmd = "printf '%s' {p}"

[[adverbs]]
name = "tone"
kind = "selector"
applies_to_verbs = ["shout"]
default = "loud"
[adverbs.values.loud]
template_var = { p = "LOUD" }
[adverbs.values.soft]
template_var = { p = "soft" }

# A confirm/destructive verb — to prove `--confirm-dangerous` is neither
# persisted nor replayed (the gate must be restored on a repeat).
[[verbs]]
name = "zap"
accepts = ["text/*"]
confirm = true
destructive = true
cmd = "printf 'zapped'"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    echo "$output" | grep -q "again" || skip "engine has no action history (bash legacy)"
}

@test "history: a successful run is recorded (verb + type, no content)" {
    run "$GOO" echotext "hello world" </dev/null
    [ "$status" -eq 0 ]
    [ -f "$HIST" ]
    grep -q '"verb":"echotext"' "$HIST"
    grep -q '"type":"text/plain"' "$HIST"
    # The subject content is NOT persisted.
    ! grep -q 'hello world' "$HIST"
}

@test "again: repeats the last verb on a new subject" {
    run "$GOO" echotext "first" </dev/null
    [ "$status" -eq 0 ]
    run "$GOO" again "second" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "second" ]            # echotext ran on the NEW subject
}

@test "again: with no history is friendly, not a crash" {
    run "$GOO" again "x" </dev/null
    [ "$status" -eq 1 ]
    [[ "$output" =~ "nothing to repeat yet" ]]
}

@test "again: replays the remembered selector adverb" {
    run "$GOO" shout "hi" --tone=soft </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "soft" ]
    run "$GOO" again "ho" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "soft" ]              # the --tone=soft was remembered, not the default LOUD
}

@test "again: never records itself (repeating stays the underlying verb)" {
    run "$GOO" echotext "a" </dev/null
    run "$GOO" again "b" </dev/null
    run "$GOO" again "c" </dev/null
    # The meta-verb 'again' must never land in the history…
    ! grep -q '"verb":"again"' "$HIST"
    # …so a further again still repeats echotext, not itself.
    run "$GOO" again "d" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "d" ]
}

@test "history: GOO_NO_HISTORY disables recording" {
    GOO_NO_HISTORY=1 run "$GOO" echotext "quiet" </dev/null
    [ "$status" -eq 0 ]
    [ ! -f "$HIST" ]
    run "$GOO" again "x" </dev/null
    [ "$status" -eq 1 ]
    [[ "$output" =~ "nothing to repeat yet" ]]
}

@test "history: only selector adverbs persist — run-control flags are dropped" {
    # parse_args folds EVERY --flag into the adverb map, incl. synthesised
    # run-control flags. None but declared selectors may be persisted: no path
    # leak (-o), no remembered safety bypass (--confirm-dangerous).
    run "$GOO" echotext "x" --confirm-dangerous=echotext -o "$BATS_TEST_TMPDIR/out.txt" </dev/null
    [ "$status" -eq 0 ]
    [ -f "$HIST" ]
    ! grep -q 'confirm-dangerous' "$HIST"
    ! grep -q 'out.txt' "$HIST"
    ! grep -q '"to"' "$HIST"
    grep -q '"adverbs":{}' "$HIST"
}

@test "again: does not replay --confirm-dangerous (the gate is restored on repeat)" {
    # Pre-approve the gate ONCE for this run...
    run "$GOO" zap "a" --confirm-dangerous=zap </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "zapped" ]]
    # ...the approval must NOT be remembered: the repeat prompts again, and EOF
    # (</dev/null) cancels → 130, never ran. (Resurrecting the bypass here would
    # silently re-run a destructive verb — the 45dc7ce regression.)
    run "$GOO" again "b" </dev/null
    [ "$status" -eq 130 ]
    [[ "$output" =~ "proceed?" ]]
}

@test "forget: clears the recorded history" {
    run "$GOO" echotext "remembered" </dev/null
    [ -f "$HIST" ]
    run "$GOO" forget </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "cleared action history" ]]
    run "$GOO" again "x" </dev/null
    [ "$status" -eq 1 ]
    [[ "$output" =~ "nothing to repeat yet" ]]
}
