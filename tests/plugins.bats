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
             lock suspend shutdown notify search; do
        echo "$verbs" | grep -qx "$v" || { echo "missing verb: $v" >&2; return 1; }
    done
}

@test "real plugins: tier-2 verbs are present" {
    local verbs
    verbs=$("$GOO" __complete verbs </dev/null)
    for v in calc \
             play-pause next now-playing \
             volume-up mute-toggle set-default-sink \
             open-repo git-status git-pull gh-pr-list \
             service-status service-restart service-journal \
             bt-connect net-up; do
        echo "$verbs" | grep -qx "$v" || { echo "missing tier-2 verb: $v" >&2; return 1; }
    done
}

@test "real plugins: expected sources are present" {
    local sources
    sources=$("$GOO" __complete sources </dev/null)
    for s in selection clipboard apps workspaces files tmux \
             sinks services repos bluetooth connections clipboard-history; do
        echo "$sources" | grep -qx "$s" || { echo "missing source: $s" >&2; return 1; }
    done
}

@test "real plugins: clipboard-history source is graceful when empty/unset" {
    # cliphist may have no store daemon (esp. on COSMIC without
    # COSMIC_DATA_CONTROL_ENABLED); the source must yield valid JSON, never
    # the 'please store something first' error.
    run "$GOO" list clipboard-history </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | jq -e 'type == "array"' >/dev/null
}

@test "real plugins: enumerate=false sources are skipped in bulk completion" {
    # `open` accepts inode/*, emitted only by the files source, which is
    # enumerate=false. So bare-positional completion must NOT bulk-list files.
    run "$GOO" __complete verb-subject-items open </dev/null
    [ "$status" -eq 0 ]
    [ -z "$output" ]
}

@test "real plugins: enumerate=false source still resolves on demand" {
    # repos is non-enumerable, but :repo: must still resolve. Point the repo
    # search at this checkout's parent (setup() overrides HOME, so the default
    # ~/Projects root wouldn't find anything).
    export GOO_GIT_ROOTS="$(dirname "$REPO_ROOT")"
    run "$GOO" git-status ":repo:cosmic-goo" </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "##" ]]
}

@test "real plugins: screenshots/OCR/QR verbs are present" {
    local verbs
    verbs=$("$GOO" __complete verbs </dev/null)
    for v in screenshot capture-region capture-file \
             ocr-region ocr-image scan-qr scan-qr-image \
             qr-encode qr-save; do
        echo "$verbs" | grep -qx "$v" || { echo "missing verb: $v" >&2; return 1; }
    done
}

@test "real plugins: qr-encode produces output" {
    run "$GOO" qr-encode "https://example.com" </dev/null
    [ "$status" -eq 0 ]
    [ -n "$output" ]
}

@test "real plugins: QR round-trips (qr-save -> scan-qr-image)" {
    local payload="cosmic-goo round trip 42"
    local png
    png=$("$GOO" qr-save "$payload" </dev/null)
    [ -f "$png" ]
    run "$GOO" scan-qr-image "$png" </dev/null
    rm -f "$png"
    [ "$status" -eq 0 ]
    [ "$output" = "$payload" ]
}

@test "real plugins: capture verbs take no subject" {
    for v in screenshot capture-region ocr-region scan-qr; do
        run "$GOO" describe "$v" </dev/null
        [ "$status" -eq 0 ]
        echo "$output" | grep -q "^accepts: *$" || { echo "$v should have empty accepts" >&2; return 1; }
    done
}

@test "real plugins: image verbs accept image/*" {
    for v in ocr-image scan-qr-image; do
        run "$GOO" describe "$v" </dev/null
        [ "$status" -eq 0 ]
        [[ "$output" =~ "accepts: image/*" ]]
    done
}

@test "real plugins: clipboard-history verbs are scoped to the vendor type" {
    # clip-paste must NOT be offered for a plain-text subject.
    run "$GOO" __complete verb-accepts-handle clip-paste </dev/null
    [ "$output" = "yes" ]
    for v in clip-paste clip-show clip-delete clip-wipe; do
        "$GOO" __complete verbs </dev/null | grep -qx "$v" \
            || { echo "missing clip verb: $v" >&2; return 1; }
    done
    # clip-delete / clip-wipe are destructive -> confirm.
    "$GOO" describe clip-delete </dev/null | grep -q "confirm: true"
    "$GOO" describe clip-wipe </dev/null | grep -q "confirm: true"
}

@test "real plugins: calc evaluates an expression" {
    run "$GOO" calc "2+2*10" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "22" ]
}

@test "real plugins: destructive tier-2 verbs require confirmation" {
    for v in git-pull service-restart service-stop; do
        run "$GOO" describe "$v" </dev/null
        [ "$status" -eq 0 ]
        [[ "$output" =~ "confirm: true" ]] || { echo "$v missing confirm" >&2; return 1; }
    done
}

@test "real plugins: media/audio transport verbs take no subject" {
    for v in play-pause volume-up mute-toggle; do
        run "$GOO" describe "$v" </dev/null
        [ "$status" -eq 0 ]
        # empty accepts renders as "accepts: " with nothing after
        echo "$output" | grep -q "^accepts: *$" || { echo "$v should have empty accepts" >&2; return 1; }
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

@test "real plugins: sha256 of a file matches sha256sum (incl. trailing newline)" {
    printf 'hello\n' > "$BATS_TEST_TMPDIR/f.txt"   # trailing newline
    run "$GOO" sha256 "$BATS_TEST_TMPDIR/f.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "$(sha256sum "$BATS_TEST_TMPDIR/f.txt" | awk '{print $1}')" ]
}

@test "real plugins: sha256 of a binary file matches sha256sum" {
    head -c 256 /dev/urandom > "$BATS_TEST_TMPDIR/b.dat"
    run "$GOO" sha256 "$BATS_TEST_TMPDIR/b.dat" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "$(sha256sum "$BATS_TEST_TMPDIR/b.dat" | awk '{print $1}')" ]
}

@test "real plugins: md5 of a binary file matches md5sum" {
    head -c 256 /dev/urandom > "$BATS_TEST_TMPDIR/b.dat"
    run "$GOO" md5 "$BATS_TEST_TMPDIR/b.dat" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "$(md5sum "$BATS_TEST_TMPDIR/b.dat" | awk '{print $1}')" ]
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
