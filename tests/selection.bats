#!/usr/bin/env bats
# Smoke tests for lib/selection.sh.
#
# The wl-paste wrappers are thin enough that the real validation is the
# behavioural recon (R5 in recon/findings.md) rather than unit tests. These
# tests verify the library sources cleanly, functions are defined, and the
# "tool missing" path doesn't crash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # shellcheck source=../lib/selection.sh
    . "$REPO_ROOT/lib/selection.sh"
}

@test "selection.sh defines expected functions" {
    declare -F selection_primary >/dev/null
    declare -F selection_clipboard >/dev/null
    declare -F selection_clipboard_mimes >/dev/null
    declare -F selection_clipboard_as >/dev/null
    declare -F focused_app >/dev/null
}

@test "selection_primary returns 0 even with empty PATH" {
    PATH="$BATS_TEST_TMPDIR/empty" run selection_primary
    [ "$status" -eq 0 ]
}

@test "selection_clipboard returns 0 even with empty PATH" {
    PATH="$BATS_TEST_TMPDIR/empty" run selection_clipboard
    [ "$status" -eq 0 ]
}

@test "selection_clipboard_mimes returns 0 even with empty PATH" {
    PATH="$BATS_TEST_TMPDIR/empty" run selection_clipboard_mimes
    [ "$status" -eq 0 ]
}

@test "focused_app errors clearly when COS_CLI cannot be resolved" {
    COS_CLI="" PATH="$BATS_TEST_TMPDIR/empty" HOME="$BATS_TEST_TMPDIR" \
        run focused_app
    [ "$status" -ne 0 ]
    [[ "$output" =~ "cos-cli not found" ]]
}

@test "focused_app emits JSON when cos-cli is available" {
    if [ -z "$COS_CLI" ] && ! command -v cos-cli >/dev/null 2>&1 \
        && [ ! -x "$HOME/.cargo/bin/cos-cli" ]; then
        skip "cos-cli not installed in test environment"
    fi
    run focused_app
    [ "$status" -eq 0 ]
    # Output may be empty (no focused window) or one or more JSON objects.
    if [ -n "$output" ]; then
        echo "$output" | jq -e '.app_id' >/dev/null
    fi
}
