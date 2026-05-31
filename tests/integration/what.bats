#!/usr/bin/env bats
# `goo what <addr>` — applicable-verbs listing surface (slice 3 of the
# completion-polish bundle). The single-source-of-truth gate for the bundle:
# the triple-equality test below proves that THREE different code paths
# reading the same projection produce identical orderings. Divergence is a
# bug in the projection, not a UI preference.
#
# See doc/design/completion-polish.md §6 slice 3 + Gate 4.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    # Fixture: a `text/x-noisy` type with NO `default_for` and SIX applicable
    # verbs (so top-5 != all). Three verbs flagged for chip-rendering coverage.
    # Order matters — `verbs::for_subject` returns registry order, which the
    # OPTIONS.allow projection preserves.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-what.toml" <<'EOF'
name = "test-what"

[[types]]
name = "text/x-noisy"
is_a = ["text/plain"]

[[verbs]]
name = "alpha"
accepts = ["text/x-noisy"]
description = "first applicable"
cmd = "true"

[[verbs]]
name = "bravo"
accepts = ["text/x-noisy"]
description = "second applicable"
confirm = true
cmd = "true"

[[verbs]]
name = "charlie"
accepts = ["text/x-noisy"]
description = "third applicable"
destructive = true
cmd = "true"

[[verbs]]
name = "delta"
accepts = ["text/x-noisy"]
description = "fourth applicable"
cmd = "true"

[[verbs]]
name = "echo"
accepts = ["text/x-noisy"]
description = "fifth applicable"
cmd = "true"

[[verbs]]
name = "foxtrot"
accepts = ["text/x-noisy"]
description = "sixth applicable (NOT in top 5)"
cmd = "true"

# A handle source so `:what:foo` is resolvable to text/x-noisy.
[[sources]]
name = "what-src"
prefix = "what"
emits = "text/x-noisy"
list_cmd = "echo '[{\"id\":\"foo\",\"title\":\"Foo\"}]'"
EOF

    # Skip if engine lacks GOO dispatch / OPTIONS (bash legacy).
    if ! "$GOO" options =text/x-noisy </dev/null 2>/dev/null | grep -q schema_version; then
        skip "engine has no OPTIONS"
    fi
}

# Extract the verb names in order from `goo what` output. The format is
# `    <name>  [chips]    <description>` — first whitespace-trimmed token
# of each verb line is the name. Lines that start with "applicable verbs"
# are skipped (the header).
extract_what_verbs() {
    printf '%s\n' "$1" | awk '
        /^applicable verbs/ { next }
        /^[[:space:]]+[^[:space:]]/ {
            sub(/^[[:space:]]+/, "")
            sub(/[[:space:]].*/, "")
            print
        }
    '
}

# Extract the verb names from the "top 5 applicable verbs" block of a GOO
# dispatch error message. Format mirrors `goo what` — same listing helper.
extract_error_verbs() {
    printf '%s\n' "$1" | awk '
        /top [0-9]+ applicable verbs:/ { in_list=1; next }
        /full list:/ { in_list=0 }
        in_list && /^[[:space:]]+[^[:space:]]/ {
            sub(/^[[:space:]]+/, "")
            sub(/[[:space:]].*/, "")
            print
        }
    '
}

# Extract verb names from `goo options <subj>`'s allow array (JSON). Uses jq
# if available, else a regex over the indented JSON output.
extract_options_verbs() {
    if command -v jq >/dev/null 2>&1; then
        printf '%s\n' "$1" | jq -r '.allow[]'
    else
        printf '%s\n' "$1" | awk '
            /"allow"/ { in_allow=1; next }
            in_allow && /\]/ { in_allow=0 }
            in_allow && /"/ {
                gsub(/^[[:space:]]*"|"[[:space:]]*,?[[:space:]]*$/, "")
                print
            }
        '
    fi
}

@test "what: lists applicable verbs for a subject" {
    run "$GOO" what =text/x-noisy </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "applicable verbs for =text/x-noisy" ]]
    [[ "$output" =~ "type: text/x-noisy" ]]
    [[ "$output" =~ "alpha" ]]
    [[ "$output" =~ "foxtrot" ]]                # ALL applicable, not just top-5
}

@test "what: renders [!] chip for confirm-flagged verbs in the listing" {
    run "$GOO" what =text/x-noisy </dev/null
    [ "$status" -eq 0 ]
    # `bravo` declared confirm = true in the fixture.
    [[ "$output" =~ bravo[[:space:]]+\[!\] ]]
}

@test "what: renders [!!] chip for destructive verbs (stronger wins)" {
    run "$GOO" what =text/x-noisy </dev/null
    [ "$status" -eq 0 ]
    # `charlie` declared destructive = true in the fixture.
    [[ "$output" =~ charlie[[:space:]]+\[!!\] ]]
}

@test "what: missing addr fails cleanly" {
    run "$GOO" what </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "usage" ]]
}

@test "what: zero applicable verbs prints message and exits 0 (informational, not error)" {
    # A type no plugin accepts. `goo what` is informational — empty is a
    # zero-result query, not an error. Catches a future refactor changing the
    # exit code to non-zero.
    run "$GOO" what =application/vnd.never.exists </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "no applicable verbs" ]]
}

@test "GOO error: >5 applicable verbs shows top-5 header + pointer to \`goo what\`" {
    # Six applicable verbs in this fixture — the count-aware header should say
    # "top 5 applicable verbs:" AND point at `goo what` for the rest. Mirror
    # of goo-dispatch.bats's ≤5 case (which suppresses both).
    run "$GOO" =text/x-noisy </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no default verb" ]]
    [[ "$output" =~ "top 5 applicable verbs:" ]]
    [[ "$output" =~ "full list:  goo what =text/x-noisy" ]]
    # The 6th verb is hidden in the error but visible via `goo what`.
    [[ ! "$output" =~ "foxtrot" ]]
}

# ---- THE Gate 4 triple-equality test ----
#
# Three different code paths must produce identical orderings for the same
# subject. If any pair diverges, the OPTIONS projection has a bug — not a UI
# preference. This locks the SSOT property across the bundle: slice 3's error
# format, `goo what`, and `goo options` all read one source.

@test "SSOT triple-equality: error top-5 == \`goo what\` first-5 == OPTIONS.allow first-5" {
    # 1. The GOO dispatch error's top-5 listing.
    run "$GOO" =text/x-noisy </dev/null
    [ "$status" -ne 0 ]
    local err_output="$output"
    local err_verbs
    err_verbs=$(extract_error_verbs "$err_output" | head -5)

    # 2. `goo what`'s first-5 verbs.
    run "$GOO" what =text/x-noisy </dev/null
    [ "$status" -eq 0 ]
    local what_verbs
    what_verbs=$(extract_what_verbs "$output" | head -5)

    # 3. OPTIONS.allow's first-5 (the canonical projection).
    run "$GOO" options =text/x-noisy </dev/null
    [ "$status" -eq 0 ]
    local options_verbs
    options_verbs=$(extract_options_verbs "$output" | head -5)

    # Sanity: all three extractors found verbs (the extractors aren't silently
    # empty — that would make the equality check tautologically true).
    local n_err n_what n_options
    n_err=$(printf '%s\n' "$err_verbs" | grep -c .)
    n_what=$(printf '%s\n' "$what_verbs" | grep -c .)
    n_options=$(printf '%s\n' "$options_verbs" | grep -c .)
    [ "$n_err" -eq 5 ] || { echo "expected 5 error verbs, got $n_err:" >&2; echo "$err_verbs" >&2; return 1; }
    [ "$n_what" -eq 5 ] || { echo "expected 5 what verbs, got $n_what:" >&2; echo "$what_verbs" >&2; return 1; }
    [ "$n_options" -eq 5 ] || { echo "expected 5 options verbs, got $n_options:" >&2; echo "$options_verbs" >&2; return 1; }

    # The actual SSOT proof: all three sequences identical.
    [ "$err_verbs" = "$what_verbs" ] \
        || { echo "ERROR LIST diverges from \`goo what\`:" >&2; diff <(echo "$err_verbs") <(echo "$what_verbs") >&2; return 1; }
    [ "$what_verbs" = "$options_verbs" ] \
        || { echo "\`goo what\` diverges from OPTIONS.allow:" >&2; diff <(echo "$what_verbs") <(echo "$options_verbs") >&2; return 1; }

    # And: the first-5 are the registry-order first 5 (alpha…echo), NOT
    # foxtrot — proves the truncation in cmd_goo is "first 5" not "any 5".
    local expected="alpha
bravo
charlie
delta
echo"
    [ "$err_verbs" = "$expected" ] \
        || { echo "top-5 order wrong; expected:" >&2; echo "$expected" >&2; echo "got:" >&2; echo "$err_verbs" >&2; return 1; }
}
