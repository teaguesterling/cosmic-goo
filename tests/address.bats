#!/usr/bin/env bats
# Tests for lib/address.sh — the goo:// domain model (mirrors the Rust
# crates/goo-engine/src/address.rs test suite).

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/builtin"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_RUNTIME_DIR" "$HOME"

    # A fixture source with two items, plus a custom (search) sigil.
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
EOF

    # shellcheck source=../lib/address.sh
    . "$REPO_ROOT/lib/address.sh"
    plugin_invalidate_cache 2>/dev/null || true
    address_invalidate_sigils 2>/dev/null || true
}

# ---------------- address_is_explicit ----------------

@test "is_explicit: sigils and native shapes are explicit" {
    address_is_explicit ":app/firefox"
    address_is_explicit ":app:firefox"
    address_is_explicit "+foo"
    address_is_explicit "^"
    address_is_explicit "^buf"
    address_is_explicit "./foo"
    address_is_explicit "../foo"
    address_is_explicit "/abs/foo"
    address_is_explicit "~/foo"
    address_is_explicit "https://example.com"
    address_is_explicit "goo://app/x"
    address_is_explicit "%alpha"   # custom sigil
}

@test "is_explicit: bare words / relative paths / undefined sigils are not" {
    ! address_is_explicit "hello world"
    ! address_is_explicit "docs/foo.md"
    ! address_is_explicit "firefox"
    ! address_is_explicit "@app:firefox"   # @ is undefined by default
}

# ---------------- address_canonicalize ----------------

@test "canonicalize: :dom/path -> value; :dom:query -> ;q= search" {
    run address_canonicalize ":app/firefox"
    [ "$output" = "goo://app/firefox" ]
    run address_canonicalize ":app:firefox"
    [ "$output" = "goo://app/;q=firefox" ]
}

@test "canonicalize: :dom alone -> domain default" {
    run address_canonicalize ":things"
    [ "$output" = "goo://things/" ]
}

@test "canonicalize: value path keeps embedded colons" {
    run address_canonicalize ":ws/0:1"
    [ "$output" = "goo://ws/0:1" ]
}

@test "canonicalize: ?refine rides along (search and bare)" {
    run address_canonicalize ":things:thing?title=beta"
    [ "$output" = "goo://things/;q=thing?title=beta" ]
    run address_canonicalize ":things?title=beta"
    [ "$output" = "goo://things/?title=beta" ]
}

@test "canonicalize: custom sigil % expands then canonicalizes (to search)" {
    run address_canonicalize "%alpha"
    [ "$output" = "goo://thing/;q=alpha" ]
}

@test "canonicalize: +foo -> text; bare -> text; undefined @ -> text" {
    run address_canonicalize "+FOO=="
    [ "$output" = "goo://text/FOO==" ]
    run address_canonicalize "hello world"
    [ "$output" = "goo://text/hello world" ]
    run address_canonicalize "@app:firefox"
    [ "$output" = "goo://text/@app:firefox" ]
}

@test "canonicalize: ^ -> clip; ^name -> clip/name" {
    run address_canonicalize "^"
    [ "$output" = "goo://clip/" ]
    run address_canonicalize "^buf"
    [ "$output" = "goo://clip/buf" ]
}

@test "canonicalize: native URL -> goo://url/...; abs path -> goo://file/..." {
    run address_canonicalize "https://example.com/x"
    [ "$output" = "goo://url/https://example.com/x" ]
    run address_canonicalize "/tmp/foo"
    [ "$output" = "goo://file//tmp/foo" ]
}

@test "canonicalize: already-canonical passes through" {
    run address_canonicalize "goo://app/firefox"
    [ "$output" = "goo://app/firefox" ]
}

# ---------------- value domains ----------------

@test "resolve: bare text -> text subject; +x forces text" {
    run address_resolve "just some words"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.type == "text/plain" and .text == "just some words"' >/dev/null
    run address_resolve "+./not-a-path"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.type == "text/plain" and .text == "./not-a-path"' >/dev/null
}

@test "resolve: URL yields text/x-uri with .id" {
    run address_resolve "https://example.com"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.type == "text/x-uri" and .id == "https://example.com"' >/dev/null
}

@test "resolve: native file path reads contents + metadata.path" {
    fixture="$BATS_TEST_TMPDIR/sample.txt"
    printf 'file body here\n' > "$fixture"
    run address_resolve "$fixture"
    [ "$status" -eq 0 ]
    [ "$(echo "$output" | jq -r '.text')" = "file body here" ]
    [ "$(echo "$output" | jq -r '.metadata.path')" = "$fixture" ]
    echo "$output" | jq -e '.type | startswith("text/")' >/dev/null
}

@test "resolve: :file/<path> value form reads a file" {
    fixture="$BATS_TEST_TMPDIR/viafile.txt"
    printf 'file content\n' > "$fixture"
    run address_resolve ":file/$fixture"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.text == "file content"' >/dev/null
}

@test "resolve: missing file errors" {
    run address_resolve "$BATS_TEST_TMPDIR/nope.txt"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no such file" ]]
}

@test "resolve: ^name (named clipboard) reports unsupported" {
    run address_resolve "^somebuffer"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not yet supported" ]]
}

# ---------------- source domains (value=exact, search=fuzzy) ----------------

@test "resolve: :dom/id is an EXACT value match" {
    run address_resolve ":things/alpha"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha" and .type == "application/vnd.test.thing"' >/dev/null
    # prefix as domain
    run address_resolve ":thing/beta"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "beta"' >/dev/null
    # not an exact id -> no value
    run address_resolve ":things/alph"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no item with id" ]]
}

@test "resolve: :dom:query is a FUZZY search" {
    run address_resolve ":things:alph"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
    run address_resolve ":things:beta thing"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "beta"' >/dev/null
    run address_resolve "%alpha"      # custom sigil → search
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}

@test "resolve: :dom with no query returns the first item" {
    run address_resolve ":things"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}

@test "resolve: search miss / unknown domain error" {
    run address_resolve ":things:zeta"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no item matching" ]]
    run address_resolve ":nosuchdomain:x"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "no domain or source" ]]
}

# ---------------- ?refine ----------------

@test "resolve: ?refine filters by field (no query = first match)" {
    run address_resolve ":things?title=beta"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "beta"' >/dev/null
    run address_resolve ":things?title=*Alpha*"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "alpha"' >/dev/null
}

@test "resolve: ?refine combines with a search match" {
    run address_resolve ":things:thing?title=beta"
    [ "$status" -eq 0 ]
    echo "$output" | jq -e '.id == "beta"' >/dev/null
}

@test "resolve: unknown ?refine field excludes everything" {
    run address_resolve ":things/alpha?foo=bar"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "?refine" ]]
}
