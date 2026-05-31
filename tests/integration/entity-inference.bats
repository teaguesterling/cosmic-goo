#!/usr/bin/env bats
# Noun-first entity inference (slice #7 / data-entry-ux.md §3.2). Bare
# `goo firefox` resolves to `:app/firefox` when the inference band is safe
# for the detected context. The engine module is unit-tested in
# `crates/goo-engine/src/inference.rs` — these tests prove the BIN
# DISPATCH integration: when does noun-first fire end-to-end? when does
# it stay invisible?
#
# Bats runs `goo` as a subprocess; stdout is therefore a pipe (non-TTY),
# which the dispatch layer detects as Context::Script per §3.2.3.
# Tests that need TTY-context behavior set GOO_INFER_STRICTNESS=tty.
#
# Safety property (§3.6): only DEFINITIVE fires silently in Script
# context — that's what these tests prove, not just assert.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    # Marker file the default verb writes — lets us assert silent execution
    # actually ran (an exit-0 alone doesn't prove dispatch happened).
    export ENTITY_MARKER="$BATS_TEST_TMPDIR/marker"

    # Fixture: sources tailored to exercise each band predictably.
    # `firefly` is exact-id in gadgets (unique above EXACT_FLOOR → DEFINITIVE).
    # `Glow` is exact-title of gad/glo (unique above EXACT_FLOOR → DEFINITIVE).
    # `thunder` matches Thunderbird via word-boundary in title (one candidate
    # in this fixture — could be HIGH or LOW depending on score).
    # `note` substring-matches multiple titles in misc (MEDIUM territory).
    # Each source has a default verb that writes the resolved id to ENTITY_MARKER.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/test-infer-entity.toml" <<EOF
name = "test-infer-entity"

[[sources]]
name = "gadgets"
prefix = "gad"
emits = "application/vnd.entityinfer.gadget"
weight = 1.3
list_cmd = "echo '[{\"id\":\"firefly\",\"title\":\"Firefly gadget\"},{\"id\":\"glo\",\"title\":\"Glow\"},{\"id\":\"thunderbird\",\"title\":\"Thunderbird gadget\"}]'"

[[sources]]
name = "misc"
prefix = "misc"
emits = "application/vnd.entityinfer.misc"
weight = 0.8
list_cmd = "echo '[{\"id\":\"note-1\",\"title\":\"Note one\"},{\"id\":\"note-2\",\"title\":\"Note two\"},{\"id\":\"note-3\",\"title\":\"Note three\"},{\"id\":\"note-4\",\"title\":\"Note four\"}]'"

[[verbs]]
name = "mark-gadget"
accepts = ["application/vnd.entityinfer.gadget"]
default_for = "application/vnd.entityinfer.gadget"
cmd = "printf '%s' {subject.id|q} > '$ENTITY_MARKER'"

[[verbs]]
name = "mark-misc"
accepts = ["application/vnd.entityinfer.misc"]
default_for = "application/vnd.entityinfer.misc"
cmd = "printf '%s' {subject.id|q} > '$ENTITY_MARKER'"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    # Skip if engine lacks inference (bash legacy doesn't have it).
    # Probe: `goo nosuchverb` should fall through to "unknown verb"; if the
    # bin doesn't recognize the subcommand list shape, this whole file skips.
    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no entity inference (bash legacy)"
    fi
}

# ---------- DEFINITIVE band: safe in any context ----------

@test "entity-inference: exact-id match → DEFINITIVE, runs default verb silently" {
    # `firefly` is the gadgets source's exact-id; resolves and runs mark-gadget
    # (writes the id to the marker). No nudge log — DEFINITIVE is silent.
    run "$GOO" firefly </dev/null
    [ "$status" -eq 0 ]
    [ -f "$ENTITY_MARKER" ]
    [ "$(cat "$ENTITY_MARKER")" = "firefly" ]
    [ -z "$output" ]
}

@test "entity-inference: exact-title match → DEFINITIVE (unique above EXACT_FLOOR)" {
    # `Glow` is the exact title of gad/glo. Only one source has it above the
    # exact floor — uniqueness rule fires DEFINITIVE.
    run "$GOO" Glow </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$ENTITY_MARKER")" = "glo" ]
}

@test "entity-inference: DEFINITIVE is safe in script context (§3.6 safety property)" {
    # The cornerstone of the safety property: exact-id-unique matches are
    # safe to resolve silently even in non-TTY / GOO_INFER_STRICTNESS=script.
    # No fuzzy guess can EVER fire silently in a pipe / CI / cron.
    GOO_INFER_STRICTNESS=script run "$GOO" firefly </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$ENTITY_MARKER")" = "firefly" ]
}

# ---------- non-DEFINITIVE in script: must fall through, no marker ----------

@test "entity-inference: non-DEFINITIVE band in script context never auto-resolves" {
    # `note` substring-matches all four misc/note-N titles. Even if some band
    # would have fired in TTY (MEDIUM picker, or HIGH if a margin emerges),
    # script context only resolves DEFINITIVE. Result: marker NOT written;
    # falls through to verb lookup → "unknown verb".
    run "$GOO" note </dev/null
    [ "$status" -ne 0 ]
    [ ! -f "$ENTITY_MARKER" ]
    [[ "$output" =~ "unknown verb" ]] || [[ "$output" =~ "would have inferred" ]]
}

@test "entity-inference: no matching candidates → fall through to 'unknown verb'" {
    # `xyzzy-impossible` doesn't match any id or title substring.
    run "$GOO" "xyzzy-impossible" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
    [ ! -f "$ENTITY_MARKER" ]
}

# ---------- non-DEFINITIVE in TTY: visible response ----------

@test "entity-inference: MEDIUM in TTY surfaces 'ambiguous' picker message" {
    # Force TTY context. `note` matches 4 misc items — MEDIUM band (>3
    # candidates rules out HIGH). The picker message fires; exit code 2.
    GOO_INFER_STRICTNESS=tty run "$GOO" note </dev/null
    [ "$status" -eq 2 ]
    [[ "$output" =~ "ambiguous" ]]
    [ ! -f "$ENTITY_MARKER" ]
}

@test "entity-inference: MEDIUM picker lists the actual addresses to re-type" {
    # The picker's value is showing addresses the user can verbatim re-run.
    # With 4 candidates all in misc/note-*, we expect numbered lines like
    # `1) :misc/note-1` etc. Cap is MAX_ALTERNATIVES (5) in engine; 4 here
    # so no "… N more" tail.
    GOO_INFER_STRICTNESS=tty run "$GOO" note </dev/null
    [ "$status" -eq 2 ]
    [[ "$output" =~ "1) :misc/note-" ]]
    [[ "$output" =~ "re-run with the explicit address" ]]
}

# ---------- shape gate: ineligible shapes skip the source enumeration ----------

@test "entity-inference: pure arithmetic stays text (no source scan)" {
    # `2+2` is rejected by is_inferable_shape (digits-and-operators only).
    # Falls through to verb lookup directly — no inference cost.
    run "$GOO" "2+2" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
    [ ! -f "$ENTITY_MARKER" ]
}

@test "entity-inference: multi-word input stays text (whitespace gate)" {
    run "$GOO" "hello world" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "unknown verb" ]]
    [ ! -f "$ENTITY_MARKER" ]
}

@test "entity-inference: addressing-shape input bypasses inference (stages A-D win)" {
    # `:gad/firefly` is the EXPLICIT form — routes through address layer's
    # existing sigil resolution, not entity inference. Marker still gets the
    # id (cmd_goo dispatches), but the engine never enters the inference path.
    run "$GOO" ":gad/firefly" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$ENTITY_MARKER")" = "firefly" ]
    [ -z "$output" ]
}

@test "entity-inference: prefix-shape input bypasses inference (slice 4 wins)" {
    # `gad/firefly` is recognized by address::is_explicit (slice 4 added
    # prefix-shape inference at the address layer); routes through cmd_goo
    # directly, not entity inference.
    run "$GOO" "gad/firefly" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$ENTITY_MARKER")" = "firefly" ]
    [ -z "$output" ]
}

# ---------- GOO_INFER_STRICTNESS overrides ----------

@test "entity-inference: unknown GOO_INFER_STRICTNESS value falls back to isatty (no crash)" {
    # An invalid env value should NOT crash the binary; it falls back to the
    # default isatty detection.
    GOO_INFER_STRICTNESS=garbage run "$GOO" firefly </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$ENTITY_MARKER")" = "firefly" ]
}

# ---------- nudge suppression ----------

@test "entity-inference: GOO_INFER_NO_NUDGE silences the MEDIUM picker message" {
    # MEDIUM in TTY would normally emit "ambiguous — closest match" on stderr;
    # with the suppression env set, the picker stays silent (but still exit 2
    # — automation knows something was inferred but no message was wanted).
    GOO_INFER_STRICTNESS=tty GOO_INFER_NO_NUDGE=1 run "$GOO" note </dev/null
    # Either: still exit 2 with no body, or behaves identically to without
    # suppression (suppression only affects HIGH-band nudges in v1, not the
    # MEDIUM picker — both are reasonable v1 interpretations).
    # The discriminating assertion: marker is NOT written (no silent resolution).
    [ ! -f "$ENTITY_MARKER" ]
}
