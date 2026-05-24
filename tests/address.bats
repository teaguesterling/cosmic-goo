#!/usr/bin/env bats
# Tests for lib/address.sh

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_RUNTIME_DIR" "$HOME"

    # A fixture source with two items, plus a custom sigil, for tests.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/things.toml" <<'EOF'
name = "things"

[[sources]]
name = "things"
prefix = "thing"
emits = "application/vnd.test.thing"
list_cmd = "echo '[{\"id\":\"alpha\",\"title\":\"Alpha Thing\"},{\"id\":\"beta\",\"title\":\"Beta Thing\"}]'"

[[sigils]]
char = "%"
expands = ":thing:"

[[sigils]]
char = "^"
expands = "+clip:"
EOF

    # shellcheck source=../lib/address.sh
    . "$REPO_ROOT/lib/address.sh"
    plugin_invalidate_cache 2>/dev/null || true
    address_invalidate_sigils 2>/dev/null || true
}

# ---------------- address_is_explicit ----------------

@test "is_explicit: core sigils and native shapes are explicit" {
    address_is_explicit ":app:firefox"
    address_is_explicit "+file:x"
    address_is_explicit "./foo"
    address_is_explicit "../foo"
    address_is_explicit "/abs/foo"
    address_is_explicit "~/foo"
    address_is_explicit "https://example.com"
    address_is_explicit "cosmic-goo:app:x"
    address_is_explicit "cosmic-goo+file:x"
}

@test "is_explicit: registered custom sigils are explicit" {
    address_is_explicit "%alpha"     # % is the fixture's custom sigil
}

@test "is_explicit: bare words, relative paths, unregistered sigils are not" {
    ! address_is_explicit "hello world"
    ! address_is_explicit "docs/foo.md"
    ! address_is_explicit "firefox"
    ! address_is_explicit "@app:firefox"   # @ is undefined by default now
}

# ---------------- address_canonicalize ----------------

@test "canonicalize: : -> cosmic-goo:" {
    run address_canonicalize ":app:firefox"
    [ "$output" = "cosmic-goo:app:firefox" ]
}

@test "canonicalize: custom sigil % expands then canonicalizes" {
    run address_canonicalize "%alpha"
    [ "$output" = "cosmic-goo:thing:alpha" ]
}

@test "canonicalize: undefined @ falls through to text" {
    run address_canonicalize "@app:firefox"
    [ "$output" = "cosmic-goo+text:@app:firefox" ]
}

@test "canonicalize: + -> cosmic-goo+" {
    run address_canonicalize "+file:a.md"
    [ "$output" = "cosmic-goo+file:a.md" ]
}

@test "canonicalize: native URL -> cosmic-goo+scheme://" {
    run address_canonicalize "https://example.com/x"
    [ "$output" = "cosmic-goo+https://example.com/x" ]
}

@test "canonicalize: absolute path -> cosmic-goo+file://abspath" {
    run address_canonicalize "/tmp/foo"
    [ "$output" = "cosmic-goo+file:///tmp/foo" ]
}

@test "canonicalize: bare text -> cosmic-goo+text:" {
    run address_canonicalize "hello world"
    [ "$output" = "cosmic-goo+text:hello world" ]
}

@test "canonicalize: already-canonical passes through" {
    run address_canonicalize "cosmic-goo:app:firefox"
    [ "$output" = "cosmic-goo:app:firefox" ]
}

# ---------------- scheme handlers ----------------

@test "resolve: text scheme yields text subject" {
    run address_resolve "just some words"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.type == "text/plain" and .text == "just some words"' >/dev/null
}

@test "resolve: URL yields text/x-uri" {
    run address_resolve "https://example.com"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.type == "text/x-uri" and .text == "https://example.com"' >/dev/null
}

@test "resolve: existing file reads contents into .text, path into metadata" {
    fixture="$BATS_TEST_TMPDIR/sample.txt"
    printf 'file body here\n' > "$fixture"
    run address_resolve "$fixture"
    [ "$status" -eq 0 ]
    [ "$(echo "$output" | jq -r '.text')" = "file body here" ]
    [ "$(echo "$output" | jq -r '.metadata.path')" = "$fixture" ]
    echo "$output" | jq -e '.type | startswith("text/")' >/dev/null
}

@test "resolve: missing file errors" {
    run address_resolve "$BATS_TEST_TMPDIR/nope.txt"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no such file" ]]
}

@test "resolve: +file: form reads a file" {
    fixture="$BATS_TEST_TMPDIR/viaplus.txt"
    printf 'plus content\n' > "$fixture"
    run address_resolve "+file:$fixture"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.text == "plus content"' >/dev/null
}

@test "resolve: ^name (named clipboard) reports unsupported" {
    run address_resolve "^somebuffer"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not yet supported" ]]
}

# ---------------- source handler ----------------

@test "resolve: :source:query matches an item by id" {
    run address_resolve ":things:alpha"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha" and .type == "application/vnd.test.thing"' >/dev/null
}

@test "resolve: :source:query matches by title substring (case-insensitive)" {
    run address_resolve ":things:beta thing"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "beta"' >/dev/null
}

@test "resolve: :prefix works like :name" {
    run address_resolve ":thing:alpha"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}

@test "resolve: :source with no query returns first item" {
    run address_resolve ":things"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}

@test "resolve: :source:query with no match errors" {
    run address_resolve ":things:zeta"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no item matching" ]]
}

@test "resolve: unknown source errors" {
    run address_resolve ":nosuchsource:x"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no source" ]]
}

@test "resolve: ?params are stripped (reserved, not yet acted on)" {
    run address_resolve ":things:alpha?foo=bar"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}
