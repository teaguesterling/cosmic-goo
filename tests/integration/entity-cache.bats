#!/usr/bin/env bats
# Per-source entity-list cache (slice 7b / data-entry-ux.md §3.3). Entity
# inference enumerates every participating source's `list_cmd`; without a cache
# that's a subprocess fan-out on every keystroke. This file proves the cache
# actually SERVES reads across separate `goo` processes — not just that a cache
# file gets written.
#
# The discriminating instrument is a WITNESS file: each source's `list_cmd`
# appends a tagged line every time it actually runs. A working cache ⇒ two
# `goo` invocations within the TTL run a source's `list_cmd` ONCE (witness has
# 1 tagged line). A broken/disabled cache ⇒ twice. Asserting only that "the
# cache file exists" would NOT catch a cache that's written-but-never-read — so
# we count witness lines instead.
#
# We drive the cache with a NON-MATCHING query (`nomatchxyz`): inference still
# enumerates EVERY source (scoring scans all sources' lists regardless of
# match), but no candidate wins, so it falls through with NO dispatch. That
# isolates the enumerate→cache path from the separate subject-resolution path
# that a DEFINITIVE resolution would also trigger.

QUERY="nomatchxyz"

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    export CACHE_WITNESS="$BATS_TEST_TMPDIR/witness"
    export ENTITY_CACHE_DIR="$XDG_RUNTIME_DIR/cosmic-goo/entities"

    # Three sources, each tagging the witness when its list_cmd runs:
    #   cached    — default TTL (5s): should run once across two invocations
    #   volatile  — cache_ttl = 0: should run every invocation (never cached)
    #   empty     — returns []: a transient-failure stand-in; must NOT be cached
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-cache.toml" <<EOF
name = "test-cache"

[[sources]]
name = "cached-src"
prefix = "cch"
emits = "application/vnd.entitycache.cached"
list_cmd = "printf 'cached\\n' >> '$CACHE_WITNESS'; echo '[{\"id\":\"zenith\",\"title\":\"Zenith\"}]'"

[[sources]]
name = "volatile-src"
prefix = "vol"
emits = "application/vnd.entitycache.volatile"
cache_ttl = 0
list_cmd = "printf 'volatile\\n' >> '$CACHE_WITNESS'; echo '[{\"id\":\"quasar\",\"title\":\"Quasar\"}]'"

[[sources]]
name = "empty-src"
prefix = "emp"
emits = "application/vnd.entitycache.empty"
list_cmd = "printf 'empty\\n' >> '$CACHE_WITNESS'; echo '[]'"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    # Skip on engines without inference (bash legacy).
    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no entity inference (bash legacy)"
    fi
}

# Count how many times a tagged source's list_cmd ran.
witness_count() {
    grep -c "^$1\$" "$CACHE_WITNESS" 2>/dev/null || echo 0
}

# ---------- the core property: cache serves reads across processes ----------

@test "entity-cache: default-TTL source runs list_cmd ONCE across two invocations" {
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    # The proof: two separate `goo` processes, one actual list_cmd run.
    [ "$(witness_count cached)" -eq 1 ]
}

@test "entity-cache: a cache file is written for the cached source" {
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    [ -f "$ENTITY_CACHE_DIR/cached-src.json" ]
    # Stores the cmd alongside items (so a changed list_cmd busts the entry).
    grep -q '"cmd"' "$ENTITY_CACHE_DIR/cached-src.json"
    grep -q '"zenith"' "$ENTITY_CACHE_DIR/cached-src.json"
}

@test "entity-cache: cache_ttl = 0 source runs list_cmd EVERY invocation (never cached)" {
    # The discriminator that proves the witness test above measures the cache
    # and not some other dedup: same two-invocation shape, but ttl=0 ⇒ 2 runs.
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    [ "$(witness_count volatile)" -eq 2 ]
    # ...and no cache file for a never-cache source.
    [ ! -f "$ENTITY_CACHE_DIR/volatile-src.json" ]
}

@test "entity-cache: an empty list_cmd result is NOT cached (transient-failure guard)" {
    # `empty-src` returns [] — the stand-in for a momentarily-failing source
    # (dbus not ready, cos-cli hiccup). Caching [] would pin the source out of
    # inference for the whole TTL; we must re-run instead.
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    [ ! -f "$ENTITY_CACHE_DIR/empty-src.json" ]
    # And because it's never cached, a second run re-executes it.
    "$GOO" "$QUERY" </dev/null 2>/dev/null || true
    [ "$(witness_count empty)" -eq 2 ]
}
