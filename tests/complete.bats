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

# Subjectless verb fixture (accepts = []) — drives the slice-2 needs_subject
# stage tests. The catch-all `*/*` case is tested via shout/poke (they have
# concrete types) since `verb-needs-subject` shouldn't need its own catch-all
# fixture — the logic is "empty ↔ subjectless; non-empty ↔ wants subject".
[[verbs]]
name = "ring"
accepts = []
cmd = "true"

[[verbs]]
name = "any-thing"
accepts = ["*/*"]
cmd = "true"

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

# Slice 2: `__complete verb-needs-subject <verb>` — `no` iff the verb's accepts
# is empty (subjectless), `yes` otherwise. Powers the bash completion's
# subjectless-verb hint. See doc/design/completion-polish.md §6 slice 2.
@test "complete verb-needs-subject: subjectless verb returns no" {
    run "$GOO" __complete verb-needs-subject ring </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "no" ]
}

@test "complete verb-needs-subject: text verb returns yes" {
    run "$GOO" __complete verb-needs-subject shout </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "yes" ]
}

@test "complete verb-needs-subject: catch-all (*/*) returns yes (xdg-open shape)" {
    # `accepts = ["*/*"]` means the verb wants a subject and doesn't care
    # about the type — not subjectless. Distinct from accepts = [].
    run "$GOO" __complete verb-needs-subject any-thing </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "yes" ]
}

@test "complete verb-needs-subject: unknown verb returns yes (conservative)" {
    # Typo / unknown verb name — return yes so a stray completion doesn't
    # claim "no subject needed" for something we don't recognize.
    run "$GOO" __complete verb-needs-subject totally-fake-name </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "yes" ]
}

@test "complete verb-needs-subject: empty arg is silent (no answer)" {
    # Shell-safety pattern from `options-allow`: degrade to empty output rather
    # than guessing. The shell script never calls this stage with empty arg,
    # but the engine must not panic if it does.
    run "$GOO" __complete verb-needs-subject "" </dev/null
    [ "$status" -eq 0 ]
    [ -z "$output" ]
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

# ---- OPTIONS-backed stages (goo-protocol §7) ----
# Subject-aware completion: same `options::options_for` projection `goo options`
# and the compose-gui consume. Drives the bash script when a subject is on the
# line at the `--<TAB>` position so the offered keys match the run-path
# `uses_adverbs` gate.

@test "complete options-allow: lists subject-applicable verbs (text → shout)" {
    # Engine-level options-allow is Rust-only; bash bin has no options module.
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" __complete options-allow =text/plain </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "shout"      # accepts text/*
    ! echo "$output" | grep -qx "poke"     # accepts widget/*, NOT text
}

@test "complete options-allow: handle subject (widget → poke, not shout)" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" __complete options-allow =application/vnd.fix.widget </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "poke"
    ! echo "$output" | grep -qx "shout"
}

@test "complete options-with: lists the verb's With: keys (shout → tone)" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" __complete options-with shout =text/plain </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "tone"
}

@test "complete options-with: a verb with no adverbs has empty output" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" __complete options-with poke =application/vnd.fix.widget </dev/null
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

# Robustness: completion must never crash the shell on bad input — bad subjects
# or missing args degrade silently to "no candidates".
@test "complete options-* with empty/bad args: silent, exit 0" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" __complete options-allow </dev/null
    [ "$status" -eq 0 ]; [ -z "$output" ]
    run "$GOO" __complete options-with shout </dev/null      # subject missing
    [ "$status" -eq 0 ]; [ -z "$output" ]
    run "$GOO" __complete options-allow ':nope/no-such-source' </dev/null
    [ "$status" -eq 0 ]; [ -z "$output" ]
}
