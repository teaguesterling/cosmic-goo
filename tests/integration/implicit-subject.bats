#!/usr/bin/env bats
# Implicit-subject fallback nudge — RUN-TIME transparency. When a text-* verb
# runs with NO subject, the no-positional fallback chain (stdin → PRIMARY
# selection → clipboard) used to resolve SILENTLY. goo now emits a one-line
# stderr nudge AFTER it borrows a subject, naming the source plus a snippet of
# the value — the silent chain made visible. Resolution is unchanged in every
# context (transparency only). Reuses the existing nudge knobs:
# GOO_INFER_STRICTNESS picks the context, GOO_INFER_NO_NUDGE silences it.
#
# NOTE: this is the run-time complement to — NOT the same as — the data-entry-ux
# §5.4 / roadmap #6 "implicit-subject preview", which is a COMPLETION-TIME
# (pre-Enter) hint on the shell/GUI surface so the user can avoid Enter if the
# fallback would grab the wrong thing. That preview is still open; this nudge
# fires post-resolution. Rust-only (new nudge); auto-skips on the bash engine.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" \
             "$XDG_RUNTIME_DIR" "$HOME" "$BATS_TEST_TMPDIR/bin"

    # Stub wl-paste so the selection/clipboard are deterministic: `--primary`
    # prints $STUB_PRIMARY, otherwise $STUB_CLIP (both default empty).
    cat > "$BATS_TEST_TMPDIR/bin/wl-paste" <<'EOF'
#!/usr/bin/env bash
for a in "$@"; do
  if [ "$a" = "--primary" ]; then printf '%s' "${STUB_PRIMARY:-}"; exit 0; fi
done
printf '%s' "${STUB_CLIP:-}"
EOF
    chmod +x "$BATS_TEST_TMPDIR/bin/wl-paste"
    export PATH="$BATS_TEST_TMPDIR/bin:$PATH"

    # A text-accepting verb that echoes the resolved subject text verbatim, so
    # the resolved value (stdout) is observable independently of the nudge.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/isfix.toml" <<'EOF'
name = "isfix"

[[verbs]]
name = "echotext"
accepts = ["text/*"]
cmd = "printf '%s' {subject.text|q}"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no implicit-subject nudge (bash legacy)"
    fi
}

@test "implicit: PRIMARY selection fallback nudges (interactive) and still resolves" {
    export STUB_PRIMARY="the selected paragraph of text"
    export GOO_INFER_STRICTNESS=tty
    run "$GOO" echotext </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "no subject given" ]]
    [[ "$output" =~ "using the PRIMARY selection" ]]
    [[ "$output" =~ "the selected paragraph of text" ]]   # snippet + resolved value
}

@test "implicit: clipboard fallback (PRIMARY empty) names the clipboard" {
    export STUB_PRIMARY=""
    export STUB_CLIP="copied snippet"
    export GOO_INFER_STRICTNESS=tty
    run "$GOO" echotext </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "using the clipboard" ]]
    [[ "$output" =~ "copied snippet" ]]
}

@test "implicit: script context resolves silently — no nudge, value unchanged" {
    export STUB_PRIMARY="quiet selection"
    export GOO_INFER_STRICTNESS=script
    run "$GOO" echotext </dev/null
    [ "$status" -eq 0 ]
    [[ ! "$output" =~ "no subject given" ]]
    [ "$output" = "quiet selection" ]   # resolution intact, stderr silent
}

@test "implicit: GOO_INFER_NO_NUDGE silences the nudge but keeps resolution" {
    export STUB_PRIMARY="hushed selection"
    export GOO_INFER_STRICTNESS=tty
    export GOO_INFER_NO_NUDGE=1
    run "$GOO" echotext </dev/null
    [ "$status" -eq 0 ]
    [[ ! "$output" =~ "no subject given" ]]
    [ "$output" = "hushed selection" ]
}

@test "implicit: an explicit positional does not nudge" {
    export STUB_PRIMARY="should-not-be-used"
    export GOO_INFER_STRICTNESS=tty
    run "$GOO" echotext "literal text" </dev/null
    [ "$status" -eq 0 ]
    [[ ! "$output" =~ "no subject given" ]]
    [ "$output" = "literal text" ]
}

@test "implicit: piped stdin is explicit — no nudge, resolves from stdin" {
    export STUB_PRIMARY="should-not-be-used"
    export GOO_INFER_STRICTNESS=tty
    run bash -c "printf 'piped in' | '$GOO' echotext"
    [ "$status" -eq 0 ]
    [[ ! "$output" =~ "no subject given" ]]
    [ "$output" = "piped in" ]
}

@test "implicit: a long selection is previewed truncated with an ellipsis" {
    export STUB_PRIMARY="this selection is considerably longer than forty characters total"
    export GOO_INFER_STRICTNESS=tty
    run "$GOO" echotext </dev/null
    [ "$status" -eq 0 ]
    # Isolate the nudge line — the full resolved value (stdout) also lands in
    # merged output and legitimately contains the tail, so assert on the snippet.
    nudge="$(printf '%s\n' "$output" | grep 'no subject given')"
    [[ "$nudge" =~ "…" ]]                                   # truncation marker present
    [[ "$nudge" =~ "this selection is considerably" ]]      # head shown
    [[ ! "$nudge" =~ "total" ]]                             # tail dropped from the snippet
    [[ "$output" =~ "this selection is considerably longer than forty characters total" ]]  # full value still resolved
}
