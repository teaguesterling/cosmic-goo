# shellcheck shell=bash
# lib/selection.sh — selection and clipboard helpers (portable; Wayland only).
#
# Source this file; do not exec it.
#
# Functions:
#   selection_primary               Echo current PRIMARY selection text (may be empty).
#   selection_clipboard             Echo current CLIPBOARD text (may be empty).
#   selection_clipboard_mimes       Echo list of MIME types on CLIPBOARD, one per line.
#   selection_clipboard_as MIME     Echo CLIPBOARD content rendered as the given MIME.
#
# Behaviour: each function returns 0 with empty output if the tool isn't
# available or the selection is empty. Diagnostics go to stderr. The shape is
# "soft" so callers can use `if [ -n "$(selection_primary)" ]; then ...` without
# error-handling boilerplate.
#
# Note: the "focused window" capability is NOT here — it's compositor-specific
# and lives in a plugin (as an implicit source), keeping this core helper free
# of any window-manager dependency. It needs only wl-clipboard.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/selection.sh: source this file, do not exec it" >&2
    exit 1
fi

selection_primary() {
    command -v wl-paste >/dev/null 2>&1 || return 0
    # `wl-paste -n` suppresses the trailing newline; exit code 1 means empty.
    wl-paste --primary --no-newline 2>/dev/null || true
}

selection_clipboard() {
    command -v wl-paste >/dev/null 2>&1 || return 0
    wl-paste --no-newline 2>/dev/null || true
}

selection_clipboard_mimes() {
    command -v wl-paste >/dev/null 2>&1 || return 0
    wl-paste --list-types 2>/dev/null || true
}

selection_clipboard_as() {
    local mime=$1
    command -v wl-paste >/dev/null 2>&1 || return 0
    wl-paste --type "$mime" --no-newline 2>/dev/null || true
}
