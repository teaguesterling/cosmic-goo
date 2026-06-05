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

# An enumerable source + a handle verb — to exercise the ENTITY-subject path
# (`:box/alpha`), §6.3's headline case, where the recorded type (resolved with
# verb context) must match the type `goo what` resolves (without it).
[[sources]]
name = "boxes"
prefix = "box"
emits = "application/vnd.againfix.box"
list_cmd = "echo '[{\"id\":\"alpha\",\"title\":\"Alpha\"}]'"

[[verbs]]
name = "poke"
accepts = ["application/vnd.againfix.box"]
cmd = "printf 'poked'"
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

# ---- §6.3: recency hint in `goo what` (annotate-only — never reorders) ----

@test "what: shows a recency hint of recently-run verbs for the type, most-recent first" {
    run "$GOO" shout "x" </dev/null            # records {shout, text/plain}
    [ "$status" -eq 0 ]
    run "$GOO" echotext "y" </dev/null         # records {echotext, text/plain}
    [ "$status" -eq 0 ]
    run "$GOO" what "=text/plain" </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "recently run on this type: echotext, shout" ]]
    # The hint is a COLUMN-0 annotation, and the indented verb listing is
    # unchanged — so verb-name extraction / SSOT order is unaffected (Gate-4
    # stays valid even with history present).
    echo "$output" | grep -qE '^recently run on this type:'      # hint at column 0
    ! echo "$output" | grep -qE '^[[:space:]]+recently'          # NOT an indented (extractable) line
    echo "$output" | grep -qE '^    echotext'                    # real verb lines still indented
}

@test "what: recency hint fires for an ENTITY subject (the :source/id headline case)" {
    # The recorded type (resolved WITH verb context) must match the type
    # `goo what` resolves (WITHOUT it), or the hint silently never fires for the
    # case §6.3 exists for ("last time you opened a :repo:, you ran status").
    run "$GOO" poke :box/alpha </dev/null
    [ "$status" -eq 0 ]
    run "$GOO" what :box/alpha </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "recently run on this type: poke" ]]
}

@test "what: no recency hint when the type has no history" {
    run "$GOO" what "=text/plain" </dev/null
    [ "$status" -eq 0 ]
    [[ ! "$output" =~ "recently run" ]]
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
