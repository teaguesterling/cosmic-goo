#!/usr/bin/env bats
# Tests for lib/types.sh

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # shellcheck source=../lib/types.sh
    . "$REPO_ROOT/lib/types.sh"
}

# ---------------- mime_matches ----------------

@test "mime_matches: exact match" {
    mime_matches "text/plain" "text/plain"
}

@test "mime_matches: exact non-match" {
    ! mime_matches "text/plain" "text/markdown"
}

@test "mime_matches: suffix wildcard text/* matches text/markdown" {
    mime_matches "text/*" "text/markdown"
}

@test "mime_matches: suffix wildcard text/* matches text/plain" {
    mime_matches "text/*" "text/plain"
}

@test "mime_matches: suffix wildcard does not cross supertype" {
    ! mime_matches "text/*" "application/json"
}

@test "mime_matches: prefix wildcard */json matches application/json" {
    mime_matches "*/json" "application/json"
}

@test "mime_matches: prefix wildcard */json does not match application/xml" {
    ! mime_matches "*/json" "application/xml"
}

@test "mime_matches: vendor wildcard matches vendor subtype" {
    mime_matches "application/vnd.tmux-use.*" "application/vnd.tmux-use.session"
}

@test "mime_matches: vendor wildcard does not match different vendor" {
    ! mime_matches "application/vnd.tmux-use.*" "application/vnd.cos-cli.app"
}

@test "mime_matches: text/* matches MIME with charset parameter" {
    mime_matches "text/*" "text/plain;charset=utf-8"
}

@test "mime_matches: empty pattern returns no match" {
    ! mime_matches "" "text/plain"
}

@test "mime_matches: empty mime returns no match" {
    ! mime_matches "text/*" ""
}

# ---------------- mime_detect_path ----------------

@test "mime_detect_path: identifies a plain text file" {
    fixture="$BATS_TEST_TMPDIR/sample.txt"
    printf 'hello world\n' > "$fixture"
    run mime_detect_path "$fixture"
    [ "$status" -eq 0 ]
    [[ "$output" =~ ^text/ ]]
}

@test "mime_detect_path: missing file fails clearly" {
    run mime_detect_path "$BATS_TEST_TMPDIR/does-not-exist"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not found" ]]
}

# ---------------- mime_detect_content ----------------

@test "mime_detect_content: https URL -> text/x-uri" {
    run mime_detect_content "https://example.com"
    [ "$status" -eq 0 ]
    [ "$output" = "text/x-uri" ]
}

@test "mime_detect_content: http URL -> text/x-uri" {
    run mime_detect_content "http://example.com/path?q=1"
    [ "$status" -eq 0 ]
    [ "$output" = "text/x-uri" ]
}

@test "mime_detect_content: custom scheme -> text/x-uri" {
    run mime_detect_content "claude://claude.ai/new?q=hi"
    [ "$status" -eq 0 ]
    [ "$output" = "text/x-uri" ]
}

@test "mime_detect_content: plain text -> text/plain" {
    run mime_detect_content "just some words here"
    [ "$status" -eq 0 ]
    [[ "$output" =~ ^text/ ]]
}

@test "mime_detect_content: existing path -> path mime" {
    fixture="$BATS_TEST_TMPDIR/sample.txt"
    printf 'hello\n' > "$fixture"
    run mime_detect_content "$fixture"
    [ "$status" -eq 0 ]
    [[ "$output" =~ ^text/ ]]
}

@test "mime_detect_content: multi-line text does not match URL or path" {
    run mime_detect_content "line one"$'\n'"line two"
    [ "$status" -eq 0 ]
    [[ "$output" =~ ^text/ ]]
}
