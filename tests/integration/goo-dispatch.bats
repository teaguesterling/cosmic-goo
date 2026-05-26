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

@test "GOO: a bare word is still an unknown verb (not a GOO subject)" {
    run "$GOO" "definitely-not-a-verb" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
}
