#!/usr/bin/env bash
# cosmic-goo-recon-keys.sh
# Guided wev capture for the MS Natural Ergonomic 4000 special keys.
# This script walks you through pressing each key and records the keysyms.
#
# Usage:
#   bash cosmic-goo-recon-keys.sh
#
# It launches wev in the background, prints prompts, and saves output to
# ./keysyms.log. Press Ctrl+C when done.

if ! command -v wev >/dev/null 2>&1; then
    echo "wev not installed. Install it first:"
    echo "  sudo apt install wev      # debian/ubuntu/pop"
    echo "  cargo install wev         # if your distro lacks it"
    exit 1
fi

OUTFILE="$PWD/keysyms.log"
: > "$OUTFILE"  # truncate

echo "================================================================"
echo "  Keysym capture for cosmic-goo"
echo "================================================================"
echo
echo "We need to identify what keysym each special key sends, both with"
echo "F-Lock ON and F-Lock OFF. wev will run in the background and log"
echo "everything to: $OUTFILE"
echo
echo "Instructions:"
echo "  1. A wev window will open in a moment. KEEP IT FOCUSED while you"
echo "     press keys. Click on it if focus wanders."
echo "  2. Press each key in the lists below ONCE."
echo "  3. The script will pause between phases — hit Enter to continue."
echo "  4. When done, press Ctrl+C to stop wev and close the window."
echo
read -p "Ready? Press Enter to launch wev..."

# Launch wev capturing only key events, tee'd to file
wev -f wl_keyboard:key 2>&1 | tee -a "$OUTFILE" &
WEV_PID=$!
sleep 1

trap 'echo; echo "Stopping wev..."; kill $WEV_PID 2>/dev/null; echo; echo "Saved to: $OUTFILE"; exit 0' INT

echo
echo "----------------------------------------------------------------"
echo "PHASE 1: F-Lock OFF"
echo "Make sure the F-Lock LED is OFF (toggle the F-Lock key once)."
echo "----------------------------------------------------------------"
echo
read -p "F-Lock is OFF — Enter to continue..."

cat <<'EOF' | tee -a "$OUTFILE"

>>> PHASE 1: F-Lock OFF <<<
Press these in order. Add a small pause between each press.

   F1  (Help)
   F2  (Undo)
   F3  (Redo)
   F4  (New)
   F5  (Open)
   F6  (Close)
   F7  (Reply)
   F8  (Forward)
   F9  (Send)
   F10 (Spell)
   F11 (Save)
   F12 (Print)

EOF
read -p "Done with phase 1 — Enter to continue..."

echo
echo "----------------------------------------------------------------"
echo "PHASE 2: F-Lock ON"
echo "Toggle F-Lock so the LED is now ON."
echo "----------------------------------------------------------------"
read -p "F-Lock is ON — Enter to continue..."

cat <<'EOF' | tee -a "$OUTFILE"

>>> PHASE 2: F-Lock ON <<<
Press these in order:

   F1
   F2
   F3
   F4
   F5
   F6
   F7
   F8
   F9
   F10
   F11
   F12

EOF
read -p "Done with phase 2 — Enter to continue..."

echo
echo "----------------------------------------------------------------"
echo "PHASE 3: Dedicated keys (F-Lock state doesn't matter)"
echo "----------------------------------------------------------------"

cat <<'EOF' | tee -a "$OUTFILE"

>>> PHASE 3: Dedicated keys <<<
Press these in order:

   My Favorites Star (workspace overview)
   Favorite 1
   Favorite 2
   Favorite 3
   Favorite 4
   Favorite 5
   Web/Home button
   Search button
   Mail button
   Calculator button
   Back (browser nav)
   Forward (browser nav)
   Zoom In  (rocker up)
   Zoom Out (rocker down)

EOF
read -p "Done with phase 3 — Enter to stop capture..."

echo
echo "Stopping wev..."
kill $WEV_PID 2>/dev/null
wait $WEV_PID 2>/dev/null

echo
echo "================================================================"
echo "Done. Output saved to: $OUTFILE"
echo
echo "What to look for in the log:"
echo "  - Lines containing 'sym:' show the keysym name (XF86Search etc.)"
echo "  - Compare F-Lock ON vs OFF for F1-F12 — they should differ"
echo "  - Check for collisions (e.g. XF86Forward dedicated vs F8 alternate)"
echo "  - Any keys that DON'T register at all need OS-level remapping"
echo "================================================================"
