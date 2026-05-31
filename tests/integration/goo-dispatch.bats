#!/usr/bin/env bats
# GOO default-verb dispatch: `goo <address>` with no verb resolves the subject
# and runs its type's default_for verb (the protocol's GOO verb, CLI form).
#
# This is a Rust-bin feature; the bash bin/goo treats a leading address as an
# unknown verb. setup() auto-skips on any engine without it, so `make test`
# (bash) skips cleanly and `GOO_BIN=…/release/goo bats …` exercises it.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-gd.toml" <<'EOF'
name = "test-gd"

[[sources]]
name = "gadgets"
prefix = "gad"
emits = "application/vnd.test.gadget"
list_cmd = "echo '[{\"id\":\"sprocket\",\"title\":\"Sprocket\"},{\"id\":\"cog\",\"title\":\"Cog\"}]'"

# A handle type WITH a default verb — GOO dispatch runs this.
[[verbs]]
name = "name-of"
accepts = ["application/vnd.test.gadget"]
default_for = "application/vnd.test.gadget"
cmd = "printf '%s' {subject.id|q}"

# A text verb with NO default_for — text has no default action.
[[verbs]]
name = "echo-text"
accepts = ["text/*"]
cmd = "printf '%s' {subject.text|q}"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    # Skip unless this engine actually does GOO dispatch (bash bin/goo doesn't).
    # (Use `if`, not `&&` — a false `[[ ]]` as setup's last line would fail it.)
    run "$GOO" "goo://gad/sprocket" </dev/null
    if [[ "$output" =~ "unknown verb" ]]; then
        skip "engine has no GOO default-verb dispatch"
    fi
}

@test "GOO: goo:// value address runs the type's default verb" {
    run "$GOO" "goo://gad/sprocket" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "sprocket" ]
}

@test "GOO: :dom/id value sigil runs the default verb" {
    run "$GOO" ":gad/cog" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "cog" ]
}

@test "GOO: :dom:query search sigil runs the default verb on the match" {
    run "$GOO" ":gad:sprocket" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "sprocket" ]
}

@test "GOO: a type with no default_for is a clean error" {
    run "$GOO" "+just some text" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no default verb" ]]
}

# Slice 3 of completion-polish: the "no default verb" error gains a helpful
# listing of applicable verbs. Same OPTIONS.allow projection `goo what`
# consumes — see tests/integration/what.bats for the triple-equality SSOT
# proof. The error format is count-aware: with ≤5 applicable verbs the header
# says "applicable verbs:" (no truncation suggested) and there's no pointer to
# `goo what` (nothing additional to see). With >5 the header says "top 5
# applicable verbs:" and points to `goo what` for the rest.
@test "GOO: no default_for prints applicable verbs (count-aware header)" {
    # The test-gd fixture has ONE text-applicable verb (echo-text). Header
    # therefore reads "applicable verbs:" (no truncation), no `goo what` pointer.
    run "$GOO" "+just some text" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no default verb" ]]
    [[ "$output" =~ "applicable verbs:" ]]
    [[ "$output" =~ "echo-text" ]]
    # Single-applicable case must NOT promise "top 5" or a fuller list.
    [[ ! "$output" =~ "top 5" ]]
    [[ ! "$output" =~ "full list:" ]]
}

@test "GOO: a bare word is still an unknown verb (not a GOO subject)" {
    run "$GOO" "definitely-not-a-verb" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
}

# Roadmap slice #4: prefix-shape inference (§3.1 of data-entry-ux.md).
# Bare `<known-prefix>/<rest>` resolves as if the user had typed `:<prefix>/<rest>`
# — same canonical form, same dispatch path, same downstream behavior. The
# fixture has `gadgets` source with `prefix = "gad"` and `name-of` as its
# default_for verb. So `goo gad/sprocket` ⇄ `goo :gad/sprocket`.
@test "GOO: prefix-shape — bare gad/sprocket dispatches like :gad/sprocket" {
    run "$GOO" "gad/sprocket" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "sprocket" ]
}

@test "GOO: prefix-shape — unknown prefix still surfaces as 'unknown verb'" {
    # `nosuch` is not a registered source prefix; falls through is_explicit
    # (false), routes to verb lookup, which fails cleanly.
    run "$GOO" "nosuch/foo" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
}

@test "GOO: prefix-shape — native paths are NEVER intercepted" {
    # `/tmp/...` and `./gad/foo` start with native path characters; the address
    # layer's starts_native check fires BEFORE prefix-shape inference, so file
    # resolution wins (and errors on a missing file — that's the test's signal
    # that we routed through the file domain, not the gad source).
    run "$GOO" "/tmp/nonexistent-goo-test-file-xyz" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no such file" ]] || [[ "$output" =~ "file" ]]
}
