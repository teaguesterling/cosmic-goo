#!/usr/bin/env bats
# Tests for the `goo __complete` backend (drives shell completion).
# Uses a fixture plugin set so candidates are deterministic and no external
# tools are invoked.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
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
list_cmd = "echo '[{\"id\":\"red\",\"title\":\"Red Widget\"},{\"id\":\"blue\",\"title\":\"Blue Widget\"}]'"

[[verbs]]
name = "poke"
accepts = ["application/vnd.fix.widget"]
cmd = "true"

[[verbs]]
name = "shout"
accepts = ["text/*"]
uses_adverbs = ["tone"]
prompt = "{tone_prefix}: {subject.text}"

[[adverbs]]
name = "tone"
kind = "selector"
applies_to_verbs = ["shout"]
default = "loud"

[adverbs.values.loud]
template_var = { tone_prefix = "LOUD" }

[adverbs.values.soft]
template_var = { tone_prefix = "soft" }

[[sigils]]
char = "%"
expands = ":wid:"
EOF
}

@test "complete subcommands: includes static + verbs" {
    run "$GOO" __complete subcommands </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "list"
    echo "$output" | grep -qx "validate"
    echo "$output" | grep -qx "poke"
    echo "$output" | grep -qx "shout"
}

@test "complete verbs: only verb names" {
    run "$GOO" __complete verbs </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "poke"
    echo "$output" | grep -qx "shout"
    ! echo "$output" | grep -qx "list"
}

@test "complete sources: source names" {
    run "$GOO" __complete sources </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "widgets"
}

@test "complete adverbs: verb's declared adverbs" {
    run "$GOO" __complete adverbs shout </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "tone"
}

@test "complete adverbs: verb with none yields nothing" {
    run "$GOO" __complete adverbs poke </dev/null
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

@test "complete adverb-values: selector values" {
    run "$GOO" __complete adverb-values tone </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "loud"
    echo "$output" | grep -qx "soft"
}

@test "complete source-prefixes: emits :prefix:" {
    run "$GOO" __complete source-prefixes </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx ":wid:"
}

@test "complete source-items: ids from a source by name" {
    run "$GOO" __complete source-items widgets </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "red"
    echo "$output" | grep -qx "blue"
}

@test "complete source-items: works by prefix too" {
    run "$GOO" __complete source-items wid </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "red"
}

@test "complete verb-accepts-handle: yes for handle verb, no for text verb" {
    run "$GOO" __complete verb-accepts-handle poke </dev/null
    [ "$output" = "yes" ]
    run "$GOO" __complete verb-accepts-handle shout </dev/null
    [ "$output" = "no" ]
}

@test "complete verb-subject-items: items for a handle verb" {
    run "$GOO" __complete verb-subject-items poke </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "red"
    echo "$output" | grep -qx "blue"
}

@test "complete sigils: registered sigil chars" {
    run "$GOO" __complete sigils </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "%"
}

@test "complete unknown stage: quiet, exit 0" {
    run "$GOO" __complete bogus-stage </dev/null
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}
