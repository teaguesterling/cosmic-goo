#!/usr/bin/env bats
# Tests for completions/goo.bash — the _goo function's COMPREPLY assembly.
#
# We can't press TAB in a test, so we replicate what readline does: set
# COMP_WORDS / COMP_CWORD / COMP_LINE / COMP_POINT for a given command line,
# call _goo, and inspect COMPREPLY. `goo` must be on PATH (the function shells
# to `goo __complete`), pointed at a fixture plugin set.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # Make `goo` resolve to the engine the harness was launched against. When
    # GOO_BIN is set (the canonical `make test` path → Rust debug binary), we
    # symlink it into BATS_TEST_TMPDIR and put that dir FIRST on PATH so the
    # _goo function's shell-outs hit the same engine as the other bats files.
    # Falling back to bin/goo (bash legacy, feature-frozen) lets the suite
    # still run for the parity-checking tests, but Rust-only stages
    # (options-*, verb-needs-subject, …) will naturally fail unless gated.
    if [ -n "${GOO_BIN:-}" ] && [ -x "${GOO_BIN:-}" ]; then
        mkdir -p "$BATS_TEST_TMPDIR/bin"
        ln -sf "$GOO_BIN" "$BATS_TEST_TMPDIR/bin/goo"
        export PATH="$BATS_TEST_TMPDIR/bin:$PATH"
    else
        export PATH="$REPO_ROOT/bin:$PATH"      # so `goo` resolves in _goo (bash legacy)
    fi
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/fix.toml" <<'EOF'
name = "fix"

[[sources]]
name = "widgets"
prefix = "wid"
emits = "application/vnd.fix.widget"
list_cmd = "echo '[{\"id\":\"red\",\"title\":\"Red\"},{\"id\":\"rose\",\"title\":\"Rose\"},{\"id\":\"blue\",\"title\":\"Blue\"}]'"

[[verbs]]
name = "poke"
accepts = ["application/vnd.fix.widget"]
cmd = "true"

[[verbs]]
name = "shout"
accepts = ["text/*"]
uses_adverbs = ["tone"]
prompt = "x"

# Subjectless verb fixture for slice-2 hint tests (accepts = []). The completion
# script should detect needs_subject == no and surface a stderr hint without
# emitting any COMPREPLY entries that would be inserted on TAB.
[[verbs]]
name = "ping"
accepts = []
cmd = "true"

# Catch-all (`*/*`) — DOES take a subject (xdg-open is the production example).
# Distinct from subjectless; the bash completion must treat this as needs_subject.
[[verbs]]
name = "any-open"
accepts = ["*/*"]
cmd = "true"

[[adverbs]]
name = "tone"
kind = "selector"
applies_to_verbs = ["shout"]
default = "loud"
[adverbs.values.loud]
template_var = { p = "L" }
[adverbs.values.soft]
template_var = { p = "s" }
EOF

    # shellcheck source=../completions/goo.bash
    . "$REPO_ROOT/completions/goo.bash"
}

# Drive _goo for a command line. A trailing space means "completing a fresh
# empty word"; otherwise the last word is the one being completed.
complete_line() {
    local line=$1
    # shellcheck disable=SC2206
    local -a words=($line)
    [[ "$line" =~ [[:space:]]$ ]] && words+=("")
    COMP_WORDS=("${words[@]}")
    COMP_CWORD=$(( ${#words[@]} - 1 ))
    COMP_LINE=$line
    COMP_POINT=${#line}
    COMPREPLY=()
    # `compopt` returns non-zero outside a real completion context; that's
    # harmless here and must not fail the harness.
    _goo || true
}

# True if COMPREPLY contains an exact element.
reply_has() {
    local want=$1 c
    for c in "${COMPREPLY[@]}"; do [ "$c" = "$want" ] && return 0; done
    return 1
}

@test "glue: first word lists subcommands + verbs" {
    complete_line "goo "
    reply_has "list"
    reply_has "validate"
    reply_has "poke"
    reply_has "shout"
}

@test "glue: first-word prefix filters" {
    complete_line "goo sh"
    reply_has "shout"
    ! reply_has "poke"
}

@test "glue: describe arg completes verb names" {
    complete_line "goo describe "
    reply_has "poke"
    reply_has "shout"
    ! reply_has "list"
}

@test "glue: list arg completes source names" {
    complete_line "goo list "
    reply_has "widgets"
}

@test "glue: --<TAB> completes adverb flags for the verb" {
    complete_line "goo shout --"
    reply_has "--tone="
}

@test "glue: --flag=<TAB> completes adverb values (re-prefixed)" {
    complete_line "goo shout --tone="
    reply_has "--tone=loud"
    reply_has "--tone=soft"
}

@test "glue: --flag=prefix filters values" {
    complete_line "goo shout --tone=s"
    reply_has "--tone=soft"
    ! reply_has "--tone=loud"
}

@test "glue: :<TAB> completes source prefixes" {
    complete_line "goo poke :"
    reply_has ":wid:"
}

@test "glue: :source:<TAB> completes items" {
    complete_line "goo poke :wid:"
    reply_has ":wid:red"
    reply_has ":wid:blue"
}

@test "glue: :source:prefix filters items" {
    complete_line "goo poke :wid:r"
    reply_has ":wid:red"
    reply_has ":wid:rose"
    ! reply_has ":wid:blue"
}

@test "glue: bare positional for a handle verb lists items" {
    complete_line "goo poke "
    reply_has "red"
    reply_has "blue"
}

@test "glue: bare positional for a text verb yields nothing" {
    complete_line "goo shout "
    [ "${#COMPREPLY[@]}" -eq 0 ]
}

@test "glue: plugins/validate take no argument completion" {
    complete_line "goo validate "
    [ "${#COMPREPLY[@]}" -eq 0 ]
}

# Slice 2: subjectless verb hint. The contract is two-fold:
#   1. COMPREPLY stays empty (nothing typed when TAB is pressed).
#   2. A hint is emitted to STDERR (visible to the user, never inserted).
# We capture stderr by redirecting at the _goo call site. See
# doc/design/completion-polish.md §6 slice 2 for the design (and §3 D3 for the
# bash display-vs-insert constraint that drives the stderr-only approach).
@test "subjectless verb: emits stderr hint, no COMPREPLY" {
    local err
    # Run _goo in a subshell so we can capture stderr while still inspecting
    # COMPREPLY in the same shell — accomplished by writing COMPREPLY to a
    # tempfile (subshell loses array assignments otherwise).
    local count
    err=$(complete_line "goo ping " 2>&1 >/dev/null; printf '%d' "${#COMPREPLY[@]}" > "$BATS_TEST_TMPDIR/n")
    count=$(cat "$BATS_TEST_TMPDIR/n")
    [ "$count" -eq 0 ]
    [[ "$err" =~ "ping takes no subject" ]]
    [[ "$err" =~ "Enter to execute" ]]
}

@test "subjectless verb: catch-all (*/*) is NOT subjectless" {
    # `any-open` has accepts = ["*/*"] — catch-all, still wants a subject. The
    # completion must not emit the subjectless hint for it. (xdg-open is the
    # production example of this case.)
    local err
    err=$(complete_line "goo any-open " 2>&1 >/dev/null)
    [[ ! "$err" =~ "takes no subject" ]]
}

@test "subjectless verb: text verb with non-empty accepts is NOT subjectless" {
    # `shout` has accepts = ["text/*"]. Sanity check that the hint only fires
    # for genuinely subjectless verbs, not anything where the bare-positional
    # completion happens to yield nothing.
    local err
    err=$(complete_line "goo shout " 2>&1 >/dev/null)
    [[ ! "$err" =~ "takes no subject" ]]
}
