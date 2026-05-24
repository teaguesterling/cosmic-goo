#!/usr/bin/env bats
# Tests against the REAL shipped plugins/ — not fixtures.
#
# Loading a plugin only parses TOML (no list_cmd runs), so `goo validate` and
# registry-shape assertions are side-effect-free. We only *execute* verbs that
# are pure and deterministic (text-utilities); we never run power/urls/apps
# verbs here — those have real side effects (lock, browser, focus).

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    GOO="$REPO_ROOT/bin/goo"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"      # no user plugins
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"  # isolate the cache
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
}

# ---------------- load + validate ----------------

@test "real plugins: goo validate passes" {
    run "$GOO" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "OK" ]]
}

@test "real plugins: expected core verbs are present" {
    local verbs
    verbs=$("$GOO" __complete verbs </dev/null)
    for v in critique summarize think draft-response \
             upper lower base64-encode sha256 \
             activate close move-to switch \
             open reveal copy-path \
             lock suspend shutdown notify search open-url; do
        echo "$verbs" | grep -qx "$v" || { echo "missing verb: $v" >&2; return 1; }
    done
}

@test "real plugins: expected sources are present" {
    local sources
    sources=$("$GOO" __complete sources </dev/null)
    for s in selection clipboard apps workspaces files tmux; do
        echo "$sources" | grep -qx "$s" || { echo "missing source: $s" >&2; return 1; }
    done
}

@test "real plugins: ^ sigil ships and maps to +clip:" {
    run "$GOO" __complete sigils </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx '\^'
}

# ---------------- deterministic execution (text-utilities) ----------------

@test "real plugins: upper uppercases" {
    run "$GOO" upper "hello world" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "HELLO WORLD" ]
}

@test "real plugins: lower lowercases" {
    run "$GOO" lower "HELLO" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hello" ]
}

@test "real plugins: base64 round-trips" {
    enc=$("$GOO" base64-encode "ahoj" </dev/null)
    [ "$enc" = "YWhvag==" ]
    dec=$("$GOO" base64-decode "$enc" </dev/null)
    [ "$dec" = "ahoj" ]
}

@test "real plugins: sha256 matches known digest" {
    run "$GOO" sha256 "hello" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824" ]
}

@test "real plugins: url-encode percent-encodes" {
    run "$GOO" url-encode "a b&c" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "a%20b%26c" ]
}

@test "real plugins: text verbs survive hostile content (quotes/parens/subshell)" {
    local hostile="Carys's note (10:40); \$(touch $BATS_TEST_TMPDIR/pwned) \`id\`"
    run "$GOO" upper "$hostile" </dev/null
    [ "$status" -eq 0 ]
    [ ! -e "$BATS_TEST_TMPDIR/pwned" ]   # injection inert
    [[ "$output" =~ "CARYS'S NOTE (10:40)" ]]
}

# ---------------- structural shape of side-effecting verbs ----------------

@test "real plugins: power verbs take no subject and confirm destructive ones" {
    # lock has empty accepts and no confirm; shutdown confirms.
    run "$GOO" describe lock </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "accepts: " ]]            # empty accepts line
    run "$GOO" describe shutdown </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "confirm: true" ]]
}

@test "real plugins: claude-routing via adverb has all four routes" {
    run "$GOO" __complete adverb-values via </dev/null
    [ "$status" -eq 0 ]
    for r in fabric claude-desktop claude-code clipboard; do
        echo "$output" | grep -qx "$r" || { echo "missing route: $r" >&2; return 1; }
    done
}

@test "real plugins: search engine adverb offers expected engines" {
    run "$GOO" __complete adverb-values engine </dev/null
    [ "$status" -eq 0 ]
    for e in ddg google github; do
        echo "$output" | grep -qx "$e" || { echo "missing engine: $e" >&2; return 1; }
    done
}
