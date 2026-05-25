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
}

@test "selection.sh has no cos-cli / COSMIC dependency (portable core)" {
    # The engine must stay compositor-agnostic; the focused-window capability
    # lives in the apps plugin (implicit source), not here.
    ! grep -qi "cos-cli\|COS_CLI\|focused_app" "$REPO_ROOT/lib/selection.sh"
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
