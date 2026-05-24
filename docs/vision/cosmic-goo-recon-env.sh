#!/usr/bin/env bash
# cosmic-goo-recon.sh
# Environment reconnaissance for the cosmic-goo project.
# Run on the target machine (longbottom). Pipes a single text report to stdout
# (and optionally to ./cosmic-goo-recon.log if you redirect).
#
# Usage:
#   bash cosmic-goo-recon.sh | tee cosmic-goo-recon.log
#
# Non-destructive. Does NOT install anything. Does NOT change settings.

set -u  # no -e; we want to keep going on errors

sep() { printf '\n================ %s ================\n' "$1"; }
exists() { command -v "$1" >/dev/null 2>&1 && echo "yes ($(command -v "$1"))" || echo "no"; }
trywith() { if command -v "$1" >/dev/null 2>&1; then "$@"; else echo "(skipped: $1 not installed)"; fi; }

sep "Report metadata"
date
echo "host: $(hostname)"
echo "user: $USER"

sep "OS / kernel"
uname -a
[ -f /etc/os-release ] && cat /etc/os-release || echo "(no /etc/os-release)"

sep "Session type / desktop"
echo "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}"
echo "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unset}"
echo "XDG_SESSION_DESKTOP=${XDG_SESSION_DESKTOP:-unset}"
echo "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset}"
echo "DISPLAY=${DISPLAY:-unset}"
echo "DESKTOP_SESSION=${DESKTOP_SESSION:-unset}"

sep "Cosmic config tree"
if [ -d "$HOME/.config/cosmic" ]; then
    ls -la "$HOME/.config/cosmic/"
    echo
    echo "--- Cosmic keybindings (CosmicSettings.Shortcuts) ---"
    find "$HOME/.config/cosmic/com.system76.CosmicSettings.Shortcuts" -type f 2>/dev/null | head -20
    echo
    echo "--- CosmicComp config (if any) ---"
    find "$HOME/.config/cosmic/com.system76.CosmicComp" -type f 2>/dev/null | head -20
else
    echo "(no ~/.config/cosmic directory)"
fi

sep "COSMIC env vars of interest"
echo "COSMIC_DATA_CONTROL_ENABLED=${COSMIC_DATA_CONTROL_ENABLED:-unset}"
echo "  ^ needed for wlr-data-control (clipboard managers); set to 1 in your shell init if unset"

sep "Cosmic D-Bus interfaces"
trywith busctl --user list 2>/dev/null | grep -i cosmic || echo "(no cosmic services in user bus)"
echo "---"
echo "(system bus, filtered)"
trywith busctl --system list 2>/dev/null | grep -i cosmic || echo "(no cosmic services in system bus)"

sep "cos-cli availability"
echo "cos-cli installed: $(exists cos-cli)"
if command -v cos-cli >/dev/null 2>&1; then
    echo
    echo "--- cos-cli info ---"
    cos-cli info 2>&1 | head -50
    echo
    echo "--- cos-cli info --json (apps section, first 30 lines) ---"
    cos-cli info --json 2>&1 | head -30
fi

sep "Claude Desktop URL handler"
echo "xdg-mime query x-scheme-handler/claude: $(xdg-mime query default x-scheme-handler/claude 2>&1)"
echo
echo "Searching for claude .desktop files:"
find ~/.local/share/applications /usr/share/applications /var/lib/flatpak/exports/share/applications -name '*laude*' 2>/dev/null
echo
echo "Claude Desktop config (if any):"
ls -la "$HOME/.config/Claude/" 2>/dev/null | head -10 || echo "(no ~/.config/Claude)"

sep "Wayland clipboard / selection tooling"
echo "wl-paste:        $(exists wl-paste)"
echo "wl-copy:         $(exists wl-copy)"
echo "cliphist:        $(exists cliphist)"
echo "clipman:         $(exists clipman)"
echo "wl-clip-persist: $(exists wl-clip-persist)"
echo "gpaste-client:   $(exists gpaste-client)"
echo
echo "--- wl-paste --primary smoke test (first 200 bytes) ---"
trywith wl-paste --primary 2>&1 | head -c 200; echo
echo
echo "--- wl-paste (clipboard) smoke test (first 200 bytes) ---"
trywith wl-paste 2>&1 | head -c 200; echo
echo
echo "--- wl-paste --list-types (available MIMEs on clipboard) ---"
trywith wl-paste --list-types 2>&1 | head -10

sep "Input event tools"
echo "wev:    $(exists wev)"
echo "xev:    $(exists xev)"
echo "wtype:  $(exists wtype)"
echo "ydotool:$(exists ydotool)"

sep "Launcher / picker tools"
echo "rofi:   $(exists rofi)"
echo "wofi:   $(exists wofi)"
echo "fuzzel: $(exists fuzzel)"
echo "tofi:   $(exists tofi)"

sep "Screenshot / OCR / QR"
echo "grim:    $(exists grim)"
echo "slurp:   $(exists slurp)"
echo "zbarimg: $(exists zbarimg)"

sep "AI / LLM tooling"
echo "fabric:    $(exists fabric)"
echo "claude:    $(exists claude)"
echo "ollama:    $(exists ollama)"
echo "alpaca:    $(exists alpaca)"
if command -v fabric >/dev/null 2>&1; then
    echo
    echo "--- fabric version ---"
    fabric --version 2>&1 | head -5
    echo
    echo "--- fabric patterns count ---"
    fabric -l 2>/dev/null | wc -l
fi
if command -v claude >/dev/null 2>&1; then
    echo
    echo "--- claude version ---"
    claude --version 2>&1 | head -5
fi

sep "Existing related tooling (yours)"
echo "tmux:       $(exists tmux)"
echo "tmux-use:   $(exists tmux-use)"
echo "ffs:        $(exists ffs)"
echo "duckdb:     $(exists duckdb)"
echo
echo "Dotfiles repo (if present):"
[ -d "$HOME/.dotfiles" ] && (cd "$HOME/.dotfiles" && git remote -v 2>/dev/null | head -3) || echo "(no ~/.dotfiles directory)"

sep "Smoke test plan (do not run automatically)"
cat <<'EOF'
Run these by hand and note results:

1. claude:// URL handler (will open Claude Desktop with a prefilled prompt):
     xdg-open "claude://claude.ai/new?q=Hello%20from%20recon"

2. claude:// code variant:
     xdg-open "claude://code/new?q=Hello&folder=$HOME"

3. cos-cli activate (focus an app — replace 'firefox' with something you have running):
     cos-cli activate -i $(cos-cli info --json | jq '.apps[] | select(.app_id | test("firefox";"i")) | .index')

4. Keyboard capture: see cosmic-goo-recon-keys.sh for guided wev capture.
EOF

sep "Report complete"
echo "Save this output and send back. The interactive bits (URL handler smoke test, key capture) need separate runs."
