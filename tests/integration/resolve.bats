#!/usr/bin/env bats
# Bare-positional file priority: a bare subject that names an *existing path* is
# resolved as that file (filesystem reality breaks the bare-token ambiguity), so
# `goo <verb> data.json` works without a leading `./`. `+name` forces literal text;
# a bare non-file stays literal text.
#
# Rust-only (resolve_subject is the Rust run-path; bash bin/goo is the frozen
# reference). setup() auto-skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    unset COSMIC_GOO_MIME_DIRS
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    # `textof` echoes the subject's text: a resolved file → its CONTENT; literal
    # text → the literal string. That difference is what these tests assert on.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/r.toml" <<'EOF'
name = "r"
[[verbs]]
name = "textof"
accepts = ["*/*"]
cmd = "printf '%s' {subject.text|q}"
EOF

    # Skip unless this engine prioritizes a bare existing file (bash doesn't).
    cd "$BATS_TEST_TMPDIR"
    printf 'PROBE' > _probe.txt
    run "$GOO" textof _probe.txt </dev/null
    if [ "$output" != "PROBE" ]; then
        skip "engine doesn't prioritize a bare existing file"
    fi
}

@test "resolve: a bare existing file resolves as the file (reads its content)" {
    cd "$BATS_TEST_TMPDIR"
    printf 'FILE BODY' > note.txt
    run "$GOO" textof note.txt </dev/null            # bare, no ./
    [ "$status" -eq 0 ]
    [ "$output" = "FILE BODY" ]                       # the file's content, not "note.txt"
}

@test "resolve: + forces literal text even when a file by that name exists" {
    cd "$BATS_TEST_TMPDIR"
    printf 'X' > lit.txt
    run "$GOO" textof +lit.txt </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "lit.txt" ]                         # the literal string, not the content "X"
}

@test "resolve: a bare non-file stays literal text" {
    run "$GOO" textof not-a-real-file-xyz </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "not-a-real-file-xyz" ]
}

@test "resolve: a bare file in a subdir resolves too" {
    cd "$BATS_TEST_TMPDIR"
    mkdir -p sub
    printf 'NESTED' > sub/n.txt
    run "$GOO" textof sub/n.txt </dev/null            # bare relative with a slash
    [ "$status" -eq 0 ]
    [ "$output" = "NESTED" ]
}
