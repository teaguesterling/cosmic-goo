#!/usr/bin/env bats
# The extension signal — a file's declared extension types it (authoritatively,
# over libmagic). A verb accepting application/x-goo runs on `sample.goo` ONLY
# because the `.goo` extension types the file as application/x-goo.
#
# Rust-only: bash's resolve_file is libmagic-only (frozen reference), so it types
# sample.goo as text/plain and the verb declines. setup() auto-skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    unset COSMIC_GOO_MIME_DIRS
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/ext.toml" <<'EOF'
name = "ext-test"

[[types]]
name = "application/x-goo"
extensions = [".goo"]

[[verbs]]
name = "eat-goo"
accepts = ["application/x-goo"]
cmd = "cat {subject.metadata.path|q}"
EOF
    printf 'hi from goo' > "$BATS_TEST_TMPDIR/sample.goo"

    # Skip unless this engine types files by extension (bash is libmagic-only).
    run "$GOO" eat-goo "$BATS_TEST_TMPDIR/sample.goo" </dev/null
    if [ "$status" -ne 0 ] || [ "$output" != "hi from goo" ]; then
        skip "engine has no extension-signal file typing"
    fi
}

@test "extsignal: .goo extension types the file → the application/x-goo verb runs" {
    run "$GOO" eat-goo "$BATS_TEST_TMPDIR/sample.goo" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hi from goo" ]
}

@test "extsignal: a wrong-extension file is not application/x-goo (verb declines)" {
    printf 'hi' > "$BATS_TEST_TMPDIR/other.txt"
    run "$GOO" eat-goo "$BATS_TEST_TMPDIR/other.txt" </dev/null
    [ "$status" -ne 0 ]
}

@test "extsignal: --explain annotates the extension source (slice 5)" {
    run "$GOO" --explain eat-goo "$BATS_TEST_TMPDIR/sample.goo" </dev/null
    [[ "$output" == *"subject: application/x-goo (via extension)"* ]]
}
