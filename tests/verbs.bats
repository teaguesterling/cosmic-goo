#!/usr/bin/env bats
# Tests for lib/verbs.sh

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"

    # Isolate plugin discovery to a fixture dir.
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg-config"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR"
    cd "$BATS_TEST_TMPDIR" || return 1

    # Build a fixture covering most shapes the dispatcher needs to handle.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/fixture.toml" <<'EOF'
name = "fixture"

[[types]]
name = "application/vnd.fixture.thing"
display = "fixture thing"
kind = "handle"

[[verbs]]
name = "echo-text"
accepts = ["text/*"]
default_for = "text/plain"
cmd = "echo {subject.text}"

[[verbs]]
name = "echo-id"
accepts = ["application/vnd.fixture.thing"]
cmd = "echo {subject.id}"

[[verbs]]
name = "destructive"
accepts = ["application/vnd.fixture.thing"]
cmd = "echo would-delete {subject.id}"
confirm = true

[[verbs]]
name = "two-step"
accepts = ["application/vnd.fixture.thing"]
object_type = "application/vnd.fixture.thing"
cmd = "echo move {subject.id} to {object.id}"

[[verbs]]
name = "critique"
accepts = ["text/*"]
uses_adverbs = ["via"]
fabric_pattern = "analyze_claims"
prompt = "Review:\n{subject.text}"

[[verbs]]
name = "think"
accepts = ["text/*"]
uses_adverbs = ["via", "depth"]
prompt = "{depth_prefix}:\n{subject.text}"

[[adverbs]]
name = "via"
kind = "selector"
default = "clipboard"

[adverbs.values.fabric]
template = "cat <<< '{verb.prompt}' | fabric -p {verb.fabric_pattern}"

[adverbs.values.clipboard]
template = "cat <<< '{verb.prompt}'"

[[adverbs]]
name = "depth"
kind = "selector"
default = "normal"

[adverbs.values.normal]
template_var = { depth_prefix = "Think about" }

[adverbs.values.ultra]
template_var = { depth_prefix = "Ultrathink about" }
EOF

    # shellcheck source=../lib/verbs.sh
    . "$REPO_ROOT/lib/verbs.sh"
    verb_invalidate_cache
}

# ---------------- verb_lookup ----------------

@test "verb_lookup: returns verb JSON for known name" {
    run verb_lookup "echo-text"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.name == "echo-text"' >/dev/null
}

@test "verb_lookup: returns failure for unknown name" {
    run verb_lookup "does-not-exist"
    [ "$status" -ne 0 ]
}

@test "verb_lookup: type filter accepts matching type" {
    run verb_lookup "echo-text" "text/plain"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.name == "echo-text"' >/dev/null
}

@test "verb_lookup: type filter rejects non-matching type" {
    run verb_lookup "echo-text" "application/vnd.fixture.thing"
    [ "$status" -ne 0 ]
}

# ---------------- verb_default_for ----------------

@test "verb_default_for: returns the verb whose default_for matches" {
    run verb_default_for "text/plain"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.name == "echo-text"' >/dev/null
}

@test "verb_default_for: empty result for type with no default" {
    run verb_default_for "image/png"
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

# ---------------- verb_for_subject ----------------

@test "verb_for_subject: lists verbs accepting a text/plain subject" {
    subject='{"type":"text/plain","text":"hi"}'
    run verb_for_subject "$subject"
    [ "$status" -eq 0 ]
    # Should include echo-text, critique, think (all text/*); not echo-id, destructive, two-step.
    names=$(echo "$output" | jq -sr 'map(.name) | sort | join(",")')
    [[ "$names" == *"echo-text"* ]]
    [[ "$names" == *"critique"* ]]
    [[ "$names" == *"think"* ]]
    [[ "$names" != *"echo-id"* ]]
    [[ "$names" != *"destructive"* ]]
}

@test "verb_for_subject: lists verbs for vendor type" {
    subject='{"type":"application/vnd.fixture.thing","id":"x"}'
    run verb_for_subject "$subject"
    [ "$status" -eq 0 ]
    names=$(echo "$output" | jq -sr 'map(.name) | sort | join(",")')
    [[ "$names" == *"echo-id"* ]]
    [[ "$names" == *"destructive"* ]]
    [[ "$names" == *"two-step"* ]]
    [[ "$names" != *"echo-text"* ]]
    [[ "$names" != *"critique"* ]]
}

# ---------------- _substitute (internal, but worth covering) ----------------

@test "_substitute: replaces {a.b} with nested value" {
    vars='{"a":{"b":"deep value"}}'
    run _substitute "got: {a.b}" "$vars"
    [ "$status" -eq 0 ]
    [ "$output" = "got: deep value" ]
}

@test "_substitute: leaves unknown paths as empty" {
    vars='{"a":1}'
    run _substitute "got: '{missing.key}'" "$vars"
    [ "$status" -eq 0 ]
    [ "$output" = "got: ''" ]
}

@test "_substitute: handles multiple substitutions" {
    vars='{"x":"foo","y":"bar"}'
    run _substitute "{x}-{y}-{x}" "$vars"
    [ "$status" -eq 0 ]
    [ "$output" = "foo-bar-foo" ]
}

# ---------------- verb_apply: direct cmd ----------------

@test "verb_apply: executes a direct cmd with subject substitution" {
    verb=$(verb_lookup echo-text)
    subject='{"type":"text/plain","text":"hello world"}'
    run verb_apply "$verb" "$subject"
    [ "$status" -eq 0 ]
    [ "$output" = "hello world" ]
}

@test "verb_apply: handle-typed verb substitutes {subject.id}" {
    verb=$(verb_lookup echo-id)
    subject='{"type":"application/vnd.fixture.thing","id":"abc-123"}'
    run verb_apply "$verb" "$subject"
    [ "$status" -eq 0 ]
    [ "$output" = "abc-123" ]
}

@test "verb_apply: rejects mismatched subject type" {
    verb=$(verb_lookup echo-id)
    subject='{"type":"text/plain","text":"oops"}'
    run verb_apply "$verb" "$subject"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "does not match verb accepts" ]]
}

@test "verb_apply: two-step verb substitutes object" {
    verb=$(verb_lookup two-step)
    subject='{"type":"application/vnd.fixture.thing","id":"src"}'
    object='{"type":"application/vnd.fixture.thing","id":"dst"}'
    run verb_apply "$verb" "$subject" "$object"
    [ "$status" -eq 0 ]
    [ "$output" = "move src to dst" ]
}

@test "verb_apply: two-step verb fails without an object" {
    verb=$(verb_lookup two-step)
    subject='{"type":"application/vnd.fixture.thing","id":"src"}'
    run verb_apply "$verb" "$subject"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "requires object" ]]
}

# ---------------- verb_apply: adverb-routed ----------------

@test "verb_apply: critique with via=clipboard renders prompt through route" {
    verb=$(verb_lookup critique)
    subject='{"type":"text/plain","text":"important text"}'
    adverbs='{"via":"clipboard"}'
    run verb_apply "$verb" "$subject" "null" "$adverbs"
    [ "$status" -eq 0 ]
    # printf %s Review: prints "Review:" then trailing args are extra positional;
    # only the first %s arg is consumed. So we should see "Review:" in output.
    # The rendered prompt contains 'Review:\n{subject.text}' which becomes
    # 'Review:\nimportant text' after substitution. The route template
    # `printf %s {verb.prompt}` then printfs the first whitespace-delimited token.
    # That confirms substitution + routing happened.
    [[ "$output" =~ "Review:" ]]
}

@test "verb_apply: critique with default adverb works (no adverbs JSON given)" {
    verb=$(verb_lookup critique)
    subject='{"type":"text/plain","text":"x"}'
    # default via = clipboard (per fixture)
    run verb_apply "$verb" "$subject"
    [ "$status" -eq 0 ]
}

@test "verb_apply: think with depth=ultra injects template_var" {
    verb=$(verb_lookup think)
    subject='{"type":"text/plain","text":"the thing"}'
    adverbs='{"via":"clipboard","depth":"ultra"}'
    run verb_apply "$verb" "$subject" "null" "$adverbs"
    [ "$status" -eq 0 ]
    # The prompt template is "{depth_prefix}:\n{subject.text}"; depth=ultra
    # injects depth_prefix="Ultrathink about". So the rendered prompt starts
    # with that.
    [[ "$output" =~ "Ultrathink about" ]]
}

@test "verb_apply: think with depth=normal uses default injection" {
    verb=$(verb_lookup think)
    subject='{"type":"text/plain","text":"x"}'
    adverbs='{"via":"clipboard"}'
    run verb_apply "$verb" "$subject" "null" "$adverbs"
    [ "$status" -eq 0 ]
    [[ "$output" =~ "Think about" ]]
}
