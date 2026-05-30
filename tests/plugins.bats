#!/usr/bin/env bats
# Tests against the REAL shipped plugins/ — not fixtures.
#
# Loading a plugin only parses TOML (no list_cmd runs), so `goo validate` and
# registry-shape assertions are side-effect-free. We only *execute* verbs that
# are pure and deterministic (text-utilities); we never run power/urls/apps
# verbs here — those have real side effects (lock, browser, focus).

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
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
    # Many verb names below are NOW POLYMORPHIC (one name, multiple impls per
    # accepts — Rust engine). `__complete verbs` lists every impl, so a `grep -qx`
    # matches any of them; bash sees the single surviving impl (override-by-name)
    # but the grep still passes for the name. Documented bash divergence: only
    # one of (network, bluetooth, ssh-hosts) `connect` actually works there.
    for v in calc \
             play-pause next now-playing \
             volume-up mute-toggle set-default-sink \
             status pull gh-pr-list \
             restart logs \
             connect; do
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

# Non-text handle domains exist to prove the noun→verb model generalizes beyond
# text/LLM. Assert each source→type→verb wiring via the OPTIONS surface, which
# correctly dispatches polymorphic verbs by type (Rust-only; bash uses simple
# override-by-name and `describe` would return the wrong impl for a polymorphic
# name like `info`/`logs`/`connect` — that's the documented bash divergence).
@test "real plugins: handle domains wire source→type→verb" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version \
        || skip "engine has no OPTIONS (polymorphism check needs it)"
    local sources
    sources=$("$GOO" __complete sources </dev/null)

    # source name | vendor type | verbs that should appear in OPTIONS.allow
    local rows=(
        "processes|application/vnd.process|info children"
        "ssh-hosts|application/vnd.ssh.host|connect ssh-copy-id"
        "containers|application/vnd.container|logs shell stop"
        "branches|application/vnd.git.branch|log show"
    )
    for row in "${rows[@]}"; do
        local src="${row%%|*}" rest="${row#*|}"
        local vtype="${rest%%|*}" vlist="${rest#*|}"
        echo "$sources" | grep -qx "$src" || { echo "missing handle source: $src" >&2; return 1; }
        local opts
        opts=$("$GOO" options "=$vtype" </dev/null 2>/dev/null)
        for v in $vlist; do
            echo "$opts" | python3 -c "
import json, sys
d = json.load(sys.stdin)
assert '$v' in d['allow'], f'$v not in allow for $vtype: ' + str(d['allow'])
" || return 1
        done
    done
}

# Content-inspection verbs accept *content* MIME types (structured + non-text
# entities). After the polymorphic-verb sweep, `info` covers image / audio-video
# / processes via type-dispatch, `tree`/`size` are unique to directories.
# OPTIONS-based check (Rust-only; the per-type dispatch is what we're verifying).
@test "real plugins: content verbs accept their content types" {
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version \
        || skip "engine has no OPTIONS"
    # verb | type that should resolve to this impl
    local rows=(
        "json-pretty|application/json"
        "json-keys|application/json"
        "info|image/png"
        "info|audio/mpeg"
        "tree|inode/directory"
        "size|inode/directory"
    )
    for row in "${rows[@]}"; do
        local v="${row%%|*}" vtype="${row#*|}"
        "$GOO" options "=$vtype" </dev/null 2>/dev/null \
            | python3 -c "
import json, sys
d = json.load(sys.stdin)
assert '$v' in d['allow'], f'$v not in allow for $vtype: ' + str(d['allow'])
" || return 1
    done
}

# json verbs on a real file resolve the same on both engines (no inference
# needed — resolve_file types the path application/json via libmagic).
@test "real plugins: json-pretty/json-keys work on a JSON file" {
    local f="$BATS_TEST_TMPDIR/data.json"
    printf '{"b":2,"a":1}' > "$f"
    run "$GOO" json-pretty "$f" </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.a == 1 and .b == 2' >/dev/null
    run "$GOO" json-keys "$f" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "$(printf 'a\nb')" ]
}

# directory verbs — polymorphic names after the sweep (`tree`/`size`); bash
# would dispatch by override-by-name, Rust by accepts. Both engines resolve a
# native path to inode/directory, so the verb fires either way.
@test "real plugins: size/tree work on a directory" {
    local d="$BATS_TEST_TMPDIR/tree"
    mkdir -p "$d/sub"
    printf 'hi' > "$d/a.txt"
    run "$GOO" size "$d" </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ [0-9] ]]                    # a size like "12K"
    if command -v tree >/dev/null 2>&1; then
        run "$GOO" tree "$d" </dev/null
        [ "$status" -eq 0 ]
        [[ "$output" == *"a.txt"* ]]
    fi
}

# `info` polymorphic across image/audio-video/processes — for an image subject,
# the Rust engine dispatches to the image-info impl. Bash has only one `info`
# survivor (override-by-name) which depends on load order; skip on bash.
@test "real plugins: info on an image reports dimensions" {
    command -v identify >/dev/null 2>&1 || skip "identify (ImageMagick) not installed"
    command -v convert  >/dev/null 2>&1 || skip "convert (ImageMagick) not installed"
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version \
        || skip "engine has no OPTIONS (proxy for polymorphic-verb support)"
    local p="$BATS_TEST_TMPDIR/x.png"
    convert -size 4x3 xc:red "$p" 2>/dev/null || skip "convert failed"
    run "$GOO" info "$p" </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"4x3"* ]]
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
    # `status` is polymorphic (services + git) — Rust dispatches by accepts to
    # git's impl for a vnd.git.repo subject. Bash override-by-name keeps only
    # one impl (services'), which rejects the repo type → skip on bash.
    "$GOO" options =text/plain </dev/null 2>/dev/null | grep -q schema_version \
        || skip "engine has no polymorphic verb dispatch (Rust only)"
    run "$GOO" status ":repo:cosmic-goo" </dev/null
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
    # `paste` must accept a vnd.cliphist.entry handle (not just plain text).
    run "$GOO" __complete verb-accepts-handle paste </dev/null
    [ "$output" = "yes" ]
    for v in paste show delete wipe; do
        "$GOO" __complete verbs </dev/null | grep -qx "$v" \
            || { echo "missing clipboard-history verb: $v" >&2; return 1; }
    done
    # `delete` / `wipe` are destructive -> confirm. Use the registry: `wipe`
    # is unique to clipboard-history; `delete` is currently only here too.
    "$GOO" describe delete </dev/null | grep -q "confirm: true"
    "$GOO" describe wipe </dev/null | grep -q "confirm: true"
}

@test "real plugins: calc evaluates an expression" {
    run "$GOO" calc "2+2*10" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "22" ]
}

@test "real plugins: destructive tier-2 verbs require confirmation" {
    # Polymorphic verbs after the sweep: `pull` (git, unique), `restart` (services,
    # unique), `stop` (services + containers; first-loaded wins on `describe` —
    # both have confirm:true, so the check holds on either dispatch).
    for v in pull restart stop; do
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

@test "real plugins: ^ clipboard sigil ships (discoverable in completion)" {
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

# ---- new launcher plugins: recent, emoji, mounts ----
# `list_cmd` actually runs here, so each test guards on its tool (python3 /
# findmnt). Side-effect verbs (emoji `copy` → wl-copy, `unmount`, polymorphic
# `open` for mounts → file manager) are NEVER executed — we only assert the
# data shape and the OPTIONS projection.

@test "recent: list_cmd runs and emits valid JSON (empty on a fresh HOME)" {
    command -v python3 >/dev/null || skip "python3 not installed"
    # Force an empty HOME so the result is deterministic and the source proves
    # it handles the missing-xbel case rather than crashing.
    run env HOME="$BATS_TEST_TMPDIR/empty" "$GOO" list recent </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "[]" ]
}

@test "emoji: source ships a non-empty curated list with the documented shape" {
    command -v python3 >/dev/null || skip "python3 not installed"
    run "$GOO" list emoji </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | python3 -c "
import json,sys
d = json.load(sys.stdin)
assert len(d) >= 100, f'expected >=100 emoji, got {len(d)}'
for e in d:
    for k in ('id','title','subtitle','text'):
        assert k in e, f'missing key {k} in {e}'
"
}

@test "emoji: OPTIONS for an emoji subject is clean (vendor type keeps text-verbs out)" {
    "$GOO" options ':emo/😀' </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" options ':emo/😀' </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *'"copy"'* ]]
    [[ "$output" == *'"default": "copy"'* ]]
    [[ "$output" != *'"critique"'* ]]      # text-verb pollution would put these in
    [[ "$output" != *'"base64-encode"'* ]]
}

@test "mounts: lists real mounts (at least the root '/' is always there)" {
    command -v findmnt >/dev/null || skip "findmnt not installed"
    run "$GOO" list mounts </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | python3 -c "
import json,sys
d = json.load(sys.stdin)
assert len(d) >= 1, 'expected at least one real mount'
assert '/' in [m['id'] for m in d], 'root mount missing'
"
}

@test "mounts: OPTIONS for a mount subject includes unmount + polymorphic open" {
    "$GOO" options ':mnt/' </dev/null 2>/dev/null | grep -q schema_version || skip "engine has no OPTIONS"
    run "$GOO" options ':mnt/' </dev/null
    [ "$status" -eq 0 ]
    # `unmount` is mount-scoped; `open` is the polymorphic verb from files.toml,
    # reaching mounts via the `is_a = ["inode/directory"]` lattice edge.
    [[ "$output" == *'"unmount"'* ]]
    [[ "$output" == *'"open"'* ]]
    [[ "$output" == *'"usage"'* ]]
    # No `default` for mounts (avoiding destructive default). Lattice-walk in
    # default_for to inherit `open` is a future engine refinement.
    [[ "$output" == *'"default": null'* ]]
}
