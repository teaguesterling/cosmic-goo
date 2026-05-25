#!/usr/bin/env bats
# Tests for `goo compose` — driven non-interactively via the GOO_COMPOSE_ANSWERS
# answer-queue mock in lib/dialog.sh (one pre-seeded selection per picker call).

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    DUMP="$BATS_TEST_TMPDIR/dump.out"
    export DUMP

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/fix.toml" <<EOF
name = "fix"

[[sources]]
name = "gadgets"
prefix = "gad"
emits = "application/vnd.fix.gadget"
list_cmd = "echo '[{\"id\":\"sprocket\",\"title\":\"Sprocket\"},{\"id\":\"cog\",\"title\":\"Cog\"}]'"

[[sources]]
name = "slots"
prefix = "slot"
emits = "application/vnd.fix.slot"
list_cmd = "echo '[{\"id\":\"one\",\"title\":\"Slot One\"}]'"

[[verbs]]
name = "wrap"
accepts = ["text/*"]
uses_adverbs = ["via"]
prompt = "W:{subject.text}:W"

[[verbs]]
name = "name-of"
accepts = ["application/vnd.fix.gadget"]
cmd = "printf '%s' {subject.id|q} > '$DUMP'"

[[verbs]]
name = "put"
accepts = ["application/vnd.fix.gadget"]
object_type = "application/vnd.fix.slot"
cmd = "printf '%s->%s' {subject.id|q} {object.id|q} > '$DUMP'"

[[adverbs]]
name = "via"
kind = "selector"
applies_to = ["text/*"]
default = "dump"
[adverbs.values.dump]
template = "printf '%s' {verb.prompt|q} > '$DUMP'"
EOF
}

# Write the answer queue (one selection per line) and run compose.
compose_with() {
    local ans="$BATS_TEST_TMPDIR/answers"
    printf '%s\n' "$@" > "$ans"
    GOO_COMPOSE_ANSWERS="$ans" run "$GOO" compose
}

@test "compose: text verb through an adverb route writes the rendered prompt" {
    printf 'hello compose' | wl-copy 2>/dev/null || skip "no clipboard in test env"
    # subject :clip:, verb wrap, via dump, confirm yes
    compose_with ":clip:" "wrap" "dump" "yes"
    [ "$status" -eq 0 ]
    [ -f "$DUMP" ]
    [[ "$(cat "$DUMP")" == W:*:W ]]
}

@test "compose: handle verb on a source item" {
    # subject :gad:sprocket, verb name-of, confirm yes
    compose_with ":gad:sprocket" "name-of" "yes"
    [ "$status" -eq 0 ]
    [ "$(cat "$DUMP")" = "sprocket" ]
}

@test "compose: two-step verb resolves an object" {
    # subject :gad:cog, verb put, object :slot:one, confirm yes
    compose_with ":gad:cog" "put" ":slot:one" "yes"
    [ "$status" -eq 0 ]
    [ "$(cat "$DUMP")" = "cog->one" ]
}

@test "compose: cancel at subject (empty answer) exits 130, runs nothing" {
    compose_with ""
    [ "$status" -eq 130 ]
    [ ! -e "$DUMP" ]
}

@test "compose: cancel at confirm (no) exits 130, runs nothing" {
    compose_with ":gad:sprocket" "name-of" "no"
    [ "$status" -eq 130 ]
    [ ! -e "$DUMP" ]
}

@test "compose: only verbs accepting the subject's type are offered" {
    # Resolve a gadget subject, then assert verb_for_subject would not include
    # the text-only `wrap`. We check via the same backend the dialog uses.
    . "$REPO_ROOT/lib/verbs.sh"
    verb_invalidate_cache
    subject='{"type":"application/vnd.fix.gadget","id":"sprocket"}'
    names=$(verb_for_subject "$subject" | jq -r '.name' | sort | tr '\n' ',')
    [[ "$names" == *"name-of"* ]]
    [[ "$names" == *"put"* ]]
    [[ "$names" != *"wrap"* ]]
}
