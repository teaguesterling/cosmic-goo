#!/usr/bin/env bats
# Verb-aware entity inference (slice 8 / data-entry-ux.md §3.4). When a bare
# token follows a verb (`goo connect fox`), inference biases toward sources the
# verb ACCEPTS — narrowing the candidate pool and scoring the survivors. This
# is the verb-position counterpart to the noun-first path in
# `entity-inference.bats`; it shares the band model but feeds the verb's
# subject (rather than dispatching a default verb).
#
# Bats runs `goo` non-TTY ⇒ Context::Script: only DEFINITIVE resolves silently.
# Tests needing interactive behavior set GOO_INFER_STRICTNESS=tty.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    # widgets: the verb `poke-widget` accepts this type. `alpha-zenith` sorts
    #   FIRST in the list and substring-matches "zenith", but `zenith` is the
    #   exact-id — so scored inference must beat handle_search's first-match.
    # gizmos: ALSO has an item id `zenith`, but `poke-widget` does NOT accept
    #   the gizmo type — proving the accepts-filter excludes it (else two exact
    #   `zenith` ids would make the match ambiguous, not DEFINITIVE).
    # slow:  `enumerate = false` ⇒ excluded from SCORED inference (§3.6 gate);
    #   reachable only via the ungated handle_search fallback.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-verb-infer.toml" <<'EOF'
name = "test-verb-infer"

[[sources]]
name = "widgets"
prefix = "wid"
emits = "application/vnd.vinfer.widget"
list_cmd = "echo '[{\"id\":\"alpha-zenith\",\"title\":\"Alpha Zenith\"},{\"id\":\"zenith\",\"title\":\"Zenith\",\"metadata\":{\"path\":\"/x/y/zenith.conf\"}},{\"id\":\"zenith-pro\",\"title\":\"Zenith Pro\"}]'"

[[sources]]
name = "gizmos"
prefix = "giz"
emits = "application/vnd.vinfer.gizmo"
list_cmd = "echo '[{\"id\":\"zenith\",\"title\":\"Gizmo Zenith\"}]'"

[[sources]]
name = "slow"
prefix = "slo"
emits = "application/vnd.vinfer.slow"
enumerate = false
list_cmd = "echo '[{\"id\":\"foxslow\",\"title\":\"Foxslow\"}]'"

[[verbs]]
name = "poke-widget"
accepts = ["application/vnd.vinfer.widget"]
cmd = "printf 'widget:%s' {subject.id|q}"

[[sources]]
name = "docs"
prefix = "doc"
emits = "text/plain"
list_cmd = "echo '[{\"id\":\"fox-file\",\"title\":\"fox\"}]'"

[[verbs]]
name = "poke-slow"
accepts = ["application/vnd.vinfer.slow"]
cmd = "printf 'slow:%s' {subject.id|q}"

[[verbs]]
name = "digest"
accepts = ["text/*"]
cmd = "printf 'id=%s text=%s' {subject.id|q} {subject.text|q}"

[[verbs]]
name = "poke-meta"
accepts = ["application/vnd.vinfer.widget"]
cmd = "printf 'path=%s' {subject.metadata.path|q}"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no entity inference (bash legacy)"
    fi
}

@test "verb-infer: scored exact-id beats handle_search first-match + excludes non-accepted source" {
    # "zenith" is exact-id in BOTH widgets and gizmos, but poke-widget only
    # accepts widgets ⇒ unique exact ⇒ DEFINITIVE ⇒ resolves to widgets/zenith.
    # If scoring lost to first-match it'd pick `alpha-zenith` (first in list);
    # if the gizmo weren't filtered it'd be ambiguous (two exact ids) → fall
    # through. Getting exactly `widget:zenith` proves both.
    run "$GOO" poke-widget zenith </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "widget:zenith" ]
}

@test "verb-infer: enumerate=false accepted source still resolves via handle_search fallback" {
    # `slow` is gated out of SCORED inference, so §3.4's flagship examples
    # (connect→:bt, open→:file — all enumerate=false) must not regress. The
    # ungated handle_search fallback catches them.
    run "$GOO" poke-slow foxslow </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "slow:foxslow" ]
}

@test "verb-infer: MEDIUM in TTY surfaces the picker (exit 2), no resolution" {
    # "zen" word-boundary-matches three widget items, none dominant → MEDIUM.
    GOO_INFER_STRICTNESS=tty run "$GOO" poke-widget zen </dev/null
    [ "$status" -eq 2 ]
    [[ "$output" =~ "ambiguous" ]]
}

@test "verb-infer: MEDIUM in script context falls through (no picker), still resolves" {
    # Script context never shows the picker. The bare token falls through to
    # the handle_search fallback, which resolves *something* (first fuzzy) —
    # the point is: no picker, no error, no crash.
    run "$GOO" poke-widget zen </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == widget:* ]]
    [[ ! "$output" =~ "ambiguous" ]]
}

@test "verb-infer: a token matching no accepted source → could not resolve" {
    run "$GOO" poke-widget "qqzzxx" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "could not resolve" ]]
}

# ---------- text-verb reordering (inference runs BEFORE the text fallback) ----------

@test "verb-infer: exact entity match wins for a text/* verb (DEFINITIVE, script-safe)" {
    # `digest` accepts text/*; today a bare token would always become literal
    # text. Slice 8 runs inference first, so an exact-title recent entity
    # (`fox`) resolves to the file — and DEFINITIVE is safe even in script.
    run "$GOO" digest fox </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "id=fox-file text=" ]
}

@test "verb-infer: a non-matching token still falls through to text content" {
    # No entity matches `hello` → inference declines → the verb sees it as
    # literal text/plain, exactly as before slice 8 (no regression).
    run "$GOO" digest hello </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "id= text=hello" ]
}

@test "verb-infer: inferred subject carries full metadata (no loss vs explicit address)" {
    # The engine's build_subject is lossy (id/title/type only); the bin must
    # re-resolve the winner via its `:prefix/id` address to recover metadata a
    # verb template needs. Without that, this regresses vs handle_search, which
    # returned the full item. `zenith` is DEFINITIVE → resolves in script.
    run "$GOO" poke-meta zenith </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "path=/x/y/zenith.conf" ]
    # Identical to the explicit-address path — the re-resolve is faithful.
    run "$GOO" poke-meta :wid/zenith </dev/null
    [ "$output" = "path=/x/y/zenith.conf" ]
}
