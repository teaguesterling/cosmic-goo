#!/usr/bin/env bats
# Watch-validated entity cache (data-entry-ux.md §3.3 + the cache-staleness fix).
# The hard guarantee: the cache NEVER serves data older than its source's truth.
# A source caches only when it declares `watch` paths; an entry is valid iff its
# `cmd` is unchanged AND every watch path's mtime equals the value observed when
# the entry was written. No `watch` ⇒ never cached (recompute every run).
#
# Instrument: a WITNESS file the list_cmd appends to whenever it actually runs,
# so we can count real executions across separate `goo` processes. The `cached`
# source's list_cmd both reads AND watches the same data file (~/data.json), so
# editing that file is the single, precise invalidation signal.
#
# Bats runs `goo` non-TTY (Script context). A non-matching query enumerates
# every source (the cache path) but resolves nothing; exact-id queries resolve
# DEFINITIVE (safe in scripts) so we can observe WHICH data was served.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    : > "$HOME/witness"
    : > "$HOME/marker"
    echo '[{"id":"alpha","title":"Alpha"}]' > "$HOME/data.json"

    #   cached   — declares watch=[~/data.json]; list_cmd reads that same file,
    #              so the watch is a precise, never-stale invalidator.
    #   volatile — no watch ⇒ never cached ⇒ runs every invocation.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/wc.toml" <<'EOF'
name = "watch-cache"

[[sources]]
name = "cached"
prefix = "cch"
emits = "application/vnd.wc.item"
watch = ["~/data.json"]
list_cmd = "printf 'ran\n' >> \"$HOME/witness\"; cat \"$HOME/data.json\""

[[sources]]
name = "volatile"
prefix = "vol"
emits = "application/vnd.wc.vol"
list_cmd = "printf 'volran\n' >> \"$HOME/witness\"; echo '[{\"id\":\"qux\",\"title\":\"Qux\"}]'"

[[verbs]]
name = "mark"
accepts = ["application/vnd.wc.item"]
default_for = "application/vnd.wc.item"
cmd = "printf '%s' {subject.id|q} > \"$HOME/marker\""
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no entity inference (bash legacy)"
    fi
}

wcount() { grep -c "^$1\$" "$HOME/witness" 2>/dev/null || true; }

# ---------- the cache serves, across processes ----------

@test "entity-cache: watched source runs list_cmd ONCE while its watch file is unchanged" {
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    [ "$(wcount ran)" -eq 1 ]
}

# ---------- mtime invalidation: the no-stale guarantee ----------

@test "entity-cache: touching the watch file re-runs list_cmd (mtime invalidation)" {
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    sleep 0.05; touch "$HOME/data.json"          # new mtime, same content
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    [ "$(wcount ran)" -eq 2 ]
}

@test "entity-cache: edited data is served fresh, never stale" {
    # Warm the cache with alpha.
    "$GOO" alpha </dev/null 2>/dev/null
    [ "$(cat "$HOME/marker")" = "alpha" ]
    # Change the underlying data (and its mtime).
    sleep 0.05; echo '[{"id":"beta","title":"Beta"}]' > "$HOME/data.json"
    # If the cache were stale it would still hold alpha and 'beta' would not
    # resolve; a fresh cache re-reads and resolves beta.
    "$GOO" beta </dev/null 2>/dev/null
    [ "$(cat "$HOME/marker")" = "beta" ]
}

# ---------- no watch ⇒ never cached ----------

@test "entity-cache: a source with no watch runs list_cmd every invocation" {
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    "$GOO" zzqqxx </dev/null 2>/dev/null || true
    [ "$(wcount volran)" -eq 2 ]
}

# ---------- goo reload ----------

@test "entity-cache: goo reload clears the cache (re-runs even with unchanged watch)" {
    "$GOO" zzqqxx </dev/null 2>/dev/null || true   # warm (ran=1)
    "$GOO" reload </dev/null 2>/dev/null
    "$GOO" zzqqxx </dev/null 2>/dev/null || true   # cache gone ⇒ re-run (ran=2)
    [ "$(wcount ran)" -eq 2 ]
}
