#!/usr/bin/env bats
# Verb-first and noun-first dispatch must select the impl of a polymorphic verb that
# accepts the RESOLVED subject — not the first-registered impl. Before this, a verb name
# with multiple impls (e.g. `show` across git/clipboard, `connect` across bt/ssh/net)
# 415'd on any impl that wasn't first, even though `goo what` listed it. See
# verbs::lookup_subject.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    cd "$BATS_TEST_TMPDIR" || return 1
    # A verb `poke` with two impls: A (first-registered) and B (second).
    cat > "$BATS_TEST_TMPDIR/poke.toml" <<'EOF'
name = "poke-test"
[[types]]
name = "application/vnd.t.a"
kind = "handle"
[[types]]
name = "application/vnd.t.b"
kind = "handle"
[[sources]]
name = "sa"
prefix = "sa"
emits = "application/vnd.t.a"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "sb"
prefix = "sb"
emits = "application/vnd.t.b"
list_cmd = '''printf '[{"id":"y"}]' '''
[[verbs]]
name = "poke"
accepts = ["application/vnd.t.a"]
cmd = "echo POKE-A"
[[verbs]]
name = "poke"
accepts = ["application/vnd.t.b"]
cmd = "echo POKE-B"
EOF
}

@test "dispatch: verb-first picks the FIRST impl's type correctly" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/poke.toml" poke :sa/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"POKE-A"* ]]
}

@test "dispatch: verb-first picks a NON-first impl by subject type (the fix)" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/poke.toml" poke :sb/y </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"POKE-B"* ]]   # not 415 on the second-registered impl
}

@test "dispatch: noun-first (do <addr> <verb>) also picks the right impl" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/poke.toml" do :sb/y poke </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"POKE-B"* ]]
}

@test "dispatch: a real multi-impl verb — show on a git branch runs git's show" {
    command -v git >/dev/null || skip "git not installed"
    # The `:br` branches source runs `git branch` in cwd, so init the repo here.
    git init -q .
    git config user.email t@e.x
    git config user.name tester
    git commit -q --allow-empty -m "seed commit"
    git branch -M main 2>/dev/null || true
    # `show` is defined by both clipboard-history (first) and git; on a branch it must
    # resolve to git's impl, not 415 on the first-registered one.
    run "$GOO" show :br/main </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"commit"* ]]
}
