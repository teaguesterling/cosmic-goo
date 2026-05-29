#!/usr/bin/env bats
# OS-MIME-DB importer — `COSMIC_GOO_MIME_DIRS` imports shared-mime-info's
# `subclasses` into the type lattice (is_a). Observed via --explain: a text/plain
# verb accepts an image/svg+xml subject ONLY when the imported chain
# (svg → application/xml → text/plain) is present.
#
# Opt-in: with the var unset there is no import (deterministic — this is why the
# host /usr/share/mime never leaks into the suite). Rust-only (bash has no
# --explain and doesn't import); setup() auto-skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    unset COSMIC_GOO_MIME_DIRS   # the baseline is always "no import"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    MIME_FIXTURE="$REPO_ROOT/tests/fixtures/mime"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/t.toml" <<'EOF'
name = "t"
[[verbs]]
name = "eat-text"
accepts = ["text/plain"]
cmd = "cat {subject.metadata.path|q}"
EOF

    # Skip unless this engine has --explain (bash doesn't; it also can't import).
    run "$GOO" --explain eat-text =text/plain </dev/null
    if ! echo "$output" | grep -q "Accept:"; then
        skip "engine has no --explain / OS-MIME importer"
    fi
}

@test "mimedb: imported subclasses let a text verb accept image/svg+xml" {
    run env COSMIC_GOO_MIME_DIRS="$MIME_FIXTURE" "$GOO" --explain eat-text =image/svg+xml </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"eat-text"* ]]
    [[ "$output" == *"text/plain"* ]]
    [[ "$output" != *"415"* ]]
}

@test "mimedb: opt-in — without COSMIC_GOO_MIME_DIRS, svg is not text (415)" {
    run "$GOO" --explain eat-text =image/svg+xml </dev/null   # var unset → no import
    [[ "$output" == *"415"* ]]
}

@test "mimedb: a missing mime dir imports nothing (still 415)" {
    run env COSMIC_GOO_MIME_DIRS="$BATS_TEST_TMPDIR/nope" "$GOO" --explain eat-text =image/svg+xml </dev/null
    [[ "$output" == *"415"* ]]
}
