#!/usr/bin/env bats
# Tests for completions/goo.bash — the _goo function's COMPREPLY assembly.
#
# We can't press TAB in a test, so we replicate what readline does: set
# COMP_WORDS / COMP_CWORD / COMP_LINE / COMP_POINT for a given command line,
# call _goo, and inspect COMPREPLY. `goo` must be on PATH (the function shells
# to `goo __complete`), pointed at a fixture plugin set.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    export PATH="$REPO_ROOT/bin:$PATH"          # so `goo` resolves in _goo
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
