#!/usr/bin/env bats
# The shipped example providers in providers/ — exercised end-to-end against the
# real plugin set (so the :cwd / text/csv / vnd.git.repo types resolve). Proves
# both the ambient (:cwd) and the subject-aware (list_cmd reads {subject.*})
# shapes, that each example LISTS and RUNS, and that an absent tool is a graceful
# no-op. Rust-only feature (providers); GOO_BIN points at the Rust engine.
#
# See providers/README.md and doc/design/dynamic-verb-providers.md.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    # Real plugins so the example providers' for_type targets exist.
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    cd "$BATS_TEST_TMPDIR" || return 1
}

prov() { echo "$REPO_ROOT/providers/$1"; }

@test "make-targets (ambient :cwd): documented targets list as verbs and run" {
    command -v make >/dev/null || skip "make not installed"
    cat > Makefile <<'EOF'
hello:  ## say hello
	@echo hi-from-make
build:  ## build the thing
	@true
EOF
    run "$GOO" -c "$(prov core-linux/make-targets.toml)" do :cwd </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"hello"* ]]
    [[ "$output" == *"say hello"* ]]      # description carried through
    [[ "$output" == *"build"* ]]
    # noun-first run: the synthesized verb executes `make hello`
    run "$GOO" -c "$(prov core-linux/make-targets.toml)" do :cwd hello </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"hi-from-make"* ]]
}

@test "make-targets: a Makefile with no documented targets yields no verbs (graceful)" {
    command -v make >/dev/null || skip "make not installed"
    printf 'all:\n\t@true\n' > Makefile
    run "$GOO" -c "$(prov core-linux/make-targets.toml)" do :cwd </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" != *"hello"* ]]
}

@test "just-recipes: absent tool is a graceful no-op (no verbs, no error)" {
    if command -v just >/dev/null; then skip "just IS installed — no-op case needs it absent"; fi
    run "$GOO" -c "$(prov dev/just-recipes.toml)" do :cwd </dev/null
    [ "$status" -eq 0 ]                    # never breaks the listing
}

@test "duckdb column-profile (per-subject): this CSV's columns list as verbs and profile runs" {
    command -v duckdb >/dev/null || skip "duckdb not installed"
    printf 'name,age\nalice,30\nbob,25\nalice,40\n' > data.csv
    # ./ form so the listing-time subject carries metadata.path (see README).
    run "$GOO" -c "$(prov duckdb/column-profile.toml)" do ./data.csv </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"name"* ]]            # a column became a verb
    [[ "$output" == *"age"* ]]
    [[ "$output" == *"profile age"* ]]     # subject-aware description
    # run it: value counts for `name` — alice appears twice
    run "$GOO" -c "$(prov duckdb/column-profile.toml)" do ./data.csv name </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"alice"* ]]
    [[ "$output" == *"2"* ]]
}

@test "git branch-log (per-subject): this repo's branches list as verbs and log runs" {
    command -v git >/dev/null || skip "git not installed"
    export GOO_GIT_ROOTS="$BATS_TEST_TMPDIR"
    git init -q "$BATS_TEST_TMPDIR/myrepo"
    git -C "$BATS_TEST_TMPDIR/myrepo" config user.email t@e.x
    git -C "$BATS_TEST_TMPDIR/myrepo" config user.name tester
    git -C "$BATS_TEST_TMPDIR/myrepo" commit -q --allow-empty -m "seed commit"
    git -C "$BATS_TEST_TMPDIR/myrepo" branch feature-x
    run "$GOO" -c "$(prov git/branch-log.toml)" do :repo:myrepo </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"feature-x"* ]]       # a branch became a verb
    [[ "$output" == *"log feature-x"* ]]
    # run it: log of feature-x shows the seed commit
    run "$GOO" -c "$(prov git/branch-log.toml)" do :repo:myrepo feature-x </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"seed commit"* ]]
}

@test "subject-aware list_cmd: a hostile path can't inject (|q in the example shape)" {
    command -v duckdb >/dev/null || skip "duckdb not installed"
    # A directory whose name carries shell metacharacters; the CSV inside it is
    # reached only via {subject.metadata.path|q}. If quoting failed, `touch pwned`
    # in the path would fire.
    mkdir -p 'a; touch pwned/'
    printf 'col\n1\n' > 'a; touch pwned/d.csv'
    run "$GOO" -c "$(prov duckdb/column-profile.toml)" do './a; touch pwned/d.csv' </dev/null
    [ ! -e pwned ]                         # injection did not execute
}
