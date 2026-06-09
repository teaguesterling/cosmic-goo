# Example bindings: Microsoft Natural Ergonomic 4000

The MS Natural 4000 has a lot of underused special keys. This doc maps the cosmic-goo verb set onto them as a starting point. None of this is shipped or generated — copy-paste into COSMIC's shortcut config (`~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`) and adjust.

## Modifier convention

| Modifier | Endpoint | Where the result lands |
|---|---|---|
| Plain | woollama → LLM router | terminal / notification |
| Shift | Claude Desktop (new chat) | Claude Desktop |
| Alt | Claude Code (new session) | new Code session |
| Alt+Shift | Claude Code (existing tmux session) | existing terminal |
| Ctrl | clipboard only | clipboard |

Modifiers select adverb values; they never change the verb. `F10` plain → `goo critique --via=woollama`; `F10` Ctrl → `goo critique --via=clipboard`.

## F-row (F-Lock OFF)

| Key | Keysym | Verb | Modifier behaviour |
|---|---|---|---|
| F1 | XF86Help | `think` | Shift/Ctrl vary depth via `--depth=really\|ultra` |
| F2 | XF86Undo | *unassigned* | — |
| F3 | XF86Redo | *unassigned* | — |
| F4 | XF86New | `create-scene` (Phase 3) | — |
| F5 | XF86Open | `browse-scenes` (Phase 3) | — |
| F6 | XF86Close | `summarize` | — |
| F7 | XF86Reply | `draft-response` (default: Claude Desktop) | Alt = Code, Ctrl = clipboard |
| F8 | XF86MailForward | `new-chat-with` (Phase 2+) | Alt = Code, Ctrl = clipboard |
| F9 | XF86Send | `send-to-chat` (Phase 2+) | Alt = Code, Ctrl = clipboard |
| F10 | XF86Spell | `critique` | — |
| F11 | XF86Save | `save-to-notes` (deferred) | — |
| F12 | XF86Print | `visualize` (deferred) | — |

## Dedicated top-row keys (F-Lock irrelevant)

| Key | Action |
|---|---|
| XF86HomePage (Web) | focus-or-spawn browser anchor scene (Phase 3) |
| XF86Search | focus-or-spawn Claude Desktop anchor (Phase 3) |
| XF86Mail | focus-or-spawn mail/calendar anchor (Phase 3) |
| XF86Calculator | launch qalculate (one-liner shell binding, no cosmic-goo needed) |
| XF86Favorites | live workspace overview |
| Favorites 1–5 | favorite scene N (Phase 3); Shift = assign current to slot N |
| XF86Back / XF86Forward | workspace/focus history navigation |

## A working binding right now

What ships in Phase 1 is the CLI. Bind F10 to `goo critique --via=clipboard` and you get a "review the selected text" hotkey that drops the rendered prompt onto the clipboard, ready to paste into whatever LLM surface you prefer.

COSMIC Settings → Keyboard → Custom Shortcuts:

| Shortcut | Command |
|---|---|
| `F10` | `/home/you/.local/bin/goo critique --via=clipboard` |
| `F10 + Ctrl` | `/home/you/.local/bin/goo critique --via=clipboard` |
| `F1` | `/home/you/.local/bin/goo think --via=clipboard` |
| `F1 + Shift` | `/home/you/.local/bin/goo think --depth=really --via=clipboard` |
| `F1 + Ctrl` | `/home/you/.local/bin/goo think --depth=ultra --via=clipboard` |

If you've installed cosmic-goo system-wide, point at `/usr/bin/goo` instead. If you symlinked into `~/.local/bin`, make sure that's on the COSMIC shortcut PATH (it usually is by default).

The selection is captured from `wl-paste --primary`, so highlight something in any app, hit F10, and paste the assembled prompt wherever you want it.

## Reconnaissance

Before binding keys, verify the keysyms your keyboard actually sends. `recon/keys.sh` runs `wev` interactively and walks you through pressing each key with F-Lock on and off. The output goes to `keysyms.log` in your CWD.

```bash
bash recon/keys.sh
```

Different MS Natural revisions and `xkb` configurations sometimes report unexpected keysyms (especially `XF86Forward` dedicated vs `F8`/`XF86MailForward` — there's a known collision on some setups). The recon output tells you which.
