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
    # A fixture mirroring the structural shapes the real polymorphic families have that
    # the two-impl `poke` doesn't: a THREE-way family (a reachable MIDDLE impl), a
    # GLOB-vs-EXACT pair (more-specific must win over registration order, like `info`'s
    # image/* beside specific impls), and an EMPTY-ACCEPTS impl that must never shadow a
    # typed subject (like media's `stop`). Stub names/markers — the matcher treats the
    # type strings opaquely, so the shapes are what's covered, not the live backends.
    cat > "$BATS_TEST_TMPDIR/families.toml" <<'EOF'
name = "families-test"
[[types]]
name = "t/a"
kind = "handle"
[[types]]
name = "t/b"
kind = "handle"
[[types]]
name = "t/c"
kind = "handle"
[[types]]
name = "t/png"
kind = "handle"
[[types]]
name = "t/gif"
kind = "handle"
[[types]]
name = "t/unit"
kind = "handle"
[[sources]]
name = "pa"
prefix = "pa"
emits = "t/a"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "pb"
prefix = "pb"
emits = "t/b"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "pc"
prefix = "pc"
emits = "t/c"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "ppng"
prefix = "ppng"
emits = "t/png"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "pgif"
prefix = "pgif"
emits = "t/gif"
list_cmd = '''printf '[{"id":"x"}]' '''
[[sources]]
name = "punit"
prefix = "punit"
emits = "t/unit"
list_cmd = '''printf '[{"id":"x"}]' '''
# Three-way family: the middle impl (t/b) is preceded AND followed by non-matching ones.
[[verbs]]
name = "ping"
accepts = ["t/a"]
cmd = "echo PING-A"
[[verbs]]
name = "ping"
accepts = ["t/b"]
cmd = "echo PING-B"
[[verbs]]
name = "ping"
accepts = ["t/c"]
cmd = "echo PING-C"
# Glob registered FIRST, exact second: selection must rank by specificity, not order.
[[verbs]]
name = "view"
accepts = ["t/*"]
cmd = "echo VIEW-GLOB"
[[verbs]]
name = "view"
accepts = ["t/png"]
cmd = "echo VIEW-EXACT"
# A typed impl beside a subjectless empty-accepts sibling — media's `stop` shape
# (`accepts=[]`, `playerctl stop`). Real plugins order the typed impl first and the
# empty-accepts one later (media's `stop` is 2nd, after containers'); a subject must
# reach the typed impl, and the empty sibling must be skipped in selection, not chosen.
[[verbs]]
name = "halt"
accepts = ["t/unit"]
cmd = "echo HALT-TYPED"
[[verbs]]
name = "halt"
accepts = []
cmd = "echo HALT-GLOBAL"
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

# --- three-way family: the MIDDLE impl must be reachable, not just first/last ---

@test "dispatch: three-way family resolves the first impl" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" ping :pa/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"PING-A"* ]]
}

@test "dispatch: three-way family resolves the MIDDLE impl (verb-first)" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" ping :pb/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"PING-B"* ]]   # preceded AND followed by non-matching impls
}

@test "dispatch: three-way family resolves the MIDDLE impl (noun-first)" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" do :pb/x ping </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"PING-B"* ]]
}

@test "dispatch: three-way family resolves the last impl" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" ping :pc/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"PING-C"* ]]
}

# --- glob vs exact: more-specific wins over registration order ---

@test "dispatch: exact impl beats a glob registered before it" {
    # `view` registers the glob (t/*) FIRST, the exact (t/png) second. A t/png subject
    # must select the exact impl — specificity, not order. Without the s>=b ranking in
    # lookup_subject this would pick the first-registered glob.
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" view :ppng/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"VIEW-EXACT"* ]]
}

@test "dispatch: a type only the glob matches falls to the glob impl" {
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" view :pgif/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"VIEW-GLOB"* ]]
}

# --- an empty-accepts (subjectless) sibling must not be chosen for a typed subject ---

@test "dispatch: a subjectless empty-accepts sibling is skipped for a typed subject" {
    # `halt` pairs a typed t/unit impl with an empty-accepts (subjectless) sibling, the
    # media-`stop` shape. A t/unit subject must reach HALT-TYPED; the empty-accepts impl
    # matches no non-empty type (verbs.rs `accepts_type`) so it must never be selected —
    # verb_specificity yields None for it, so lookup_subject skips it (see the unit test
    # lookup_subject_skips_an_empty_accepts_impl_for_a_typed_subject).
    run "$GOO" -c "$BATS_TEST_TMPDIR/families.toml" halt :punit/x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"HALT-TYPED"* ]]
    [[ "$output" != *"HALT-GLOBAL"* ]]
}
