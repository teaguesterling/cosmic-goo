# shellcheck shell=bash
# lib/dialog.sh — picker abstraction for the compose flow.
#
# Source this file; do not exec it.
#
# Provides a dmenu-protocol picker (candidates on stdin, selection on stdout)
# that works across fuzzel / rofi / wofi / fzf, with zenity as a GTK fallback
# for systems without a wlroots picker. The picker is chosen via GOO_PICKER,
# else auto-detected. A test/scripted mode reads pre-seeded answers from a file
# so the compose flow can be driven non-interactively.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/dialog.sh: source this file, do not exec it" >&2
    exit 1
fi

# Echo the picker backend name: GOO_PICKER if set, else first available.
dialog_picker() {
    if [ -n "${GOO_PICKER:-}" ]; then
        printf '%s' "$GOO_PICKER"
        return 0
    fi
    local c
    for c in fuzzel rofi wofi fzf zenity; do
        if command -v "$c" >/dev/null 2>&1; then
            printf '%s' "$c"
            return 0
        fi
    done
    return 1
}

# _pick PROMPT  — read newline-separated candidates on stdin, echo the choice.
#
# Test/scripted mode: if GOO_COMPOSE_ANSWERS names a file, each call consumes
# (and removes) its first line and echoes that as the selection — letting tests
# drive the whole flow deterministically without a real picker.
dialog_pick() {
    local prompt=$1

    if [ -n "${GOO_COMPOSE_ANSWERS:-}" ] && [ -f "$GOO_COMPOSE_ANSWERS" ]; then
        # Consume candidates from stdin (ignored) so producers don't block.
        cat >/dev/null
        local ans
        ans=$(head -n 1 "$GOO_COMPOSE_ANSWERS")
        # Pop the consumed line.
        tail -n +2 "$GOO_COMPOSE_ANSWERS" > "$GOO_COMPOSE_ANSWERS.tmp" \
            && mv "$GOO_COMPOSE_ANSWERS.tmp" "$GOO_COMPOSE_ANSWERS"
        [ -z "$ans" ] && return 1   # empty answer = cancel
        printf '%s\n' "$ans"
        return 0
    fi

    local backend
    backend=$(dialog_picker) || {
        echo "goo compose: no picker found (install fuzzel/rofi/wofi/fzf/zenity or set GOO_PICKER)" >&2
        return 1
    }
    case "$backend" in
        fuzzel) fuzzel --dmenu --prompt "$prompt ❯ " ;;
        rofi)   rofi -dmenu -i -p "$prompt" ;;
        wofi)   wofi --dmenu --insensitive --prompt "$prompt" ;;
        fzf)    fzf --prompt "$prompt ❯ " --height=40% --reverse ;;
        zenity)
            # zenity --list takes rows as argv, not stdin, so slurp first. One
            # column; the whole "addr<TAB>label" line is the row and is returned
            # verbatim (callers strip at the tab). Non-zero exit = cancel.
            local rows
            mapfile -t rows
            [ "${#rows[@]}" -eq 0 ] && return 1
            zenity --list --title="goo" --text="$prompt" \
                   --column="$prompt" --hide-header "${rows[@]}" 2>/dev/null
            ;;
        *)
            echo "goo compose: unknown picker '$backend'" >&2
            return 1
            ;;
    esac
}

# dialog_confirm PROMPT — yes/no via the picker. Returns 0 for yes.
dialog_confirm() {
    local prompt=$1 choice
    choice=$(printf 'yes\nno\n' | dialog_pick "$prompt") || return 1
    [ "$choice" = "yes" ]
}
