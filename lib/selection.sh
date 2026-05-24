# shellcheck shell=bash
# lib/selection.sh — selection, clipboard, and focused-window helpers.
#
# Source this file; do not exec it.
#
# Functions:
#   selection_primary               Echo current PRIMARY selection text (may be empty).
#   selection_clipboard             Echo current CLIPBOARD text (may be empty).
#   selection_clipboard_mimes       Echo list of MIME types on CLIPBOARD, one per line.
#   selection_clipboard_as MIME     Echo CLIPBOARD content rendered as the given MIME.
#   focused_app                     Echo JSON of currently focused window(s) via cos-cli.
#
# Behaviour: each function returns 0 with empty output if the tool isn't
# available or the selection is empty. Diagnostics go to stderr. The shape is
# "soft" so callers can use `if [ -n "$(selection_primary)" ]; then ...` without
# error-handling boilerplate.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/selection.sh: source this file, do not exec it" >&2
    exit 1
fi

# cos-cli often lives at ~/.cargo/bin which isn't in non-interactive PATH.
# Resolve once at source time; callers can override with COS_CLI.
COS_CLI="${COS_CLI:-}"
if [ -z "$COS_CLI" ]; then
    if command -v cos-cli >/dev/null 2>&1; then
        COS_CLI=$(command -v cos-cli)
    elif [ -x "$HOME/.cargo/bin/cos-cli" ]; then
        COS_CLI="$HOME/.cargo/bin/cos-cli"
    fi
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

focused_app() {
    if [ -z "$COS_CLI" ]; then
        echo "focused_app: cos-cli not found (set COS_CLI or add to PATH)" >&2
        return 1
    fi
    "$COS_CLI" info --json 2>/dev/null \
        | jq -c '.apps[] | select(.state[]? == "activated")'
}
