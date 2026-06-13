# Example bindings: Microsoft Natural Ergonomic 4000

The MS Natural 4000 has a lot of underused special keys. This doc maps **shipped** goo verbs onto them as a starting point — a hotkey is just a saved goo *sentence*. None of this is generated; copy-paste into COSMIC's shortcut config (`~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`) and adjust.

The point of binding goo (rather than one-off scripts) is the **object + verb** model: a key names a thing on your desktop, or borrows the selection/clipboard, and applies a verb. Text verbs (`summarize`, `critique`) are *one family* — the same keys can act on the clipboard (`do ^`), a window, a file path, or media transport.

## Modifier convention

Modifiers select an **adverb value** — they never change the verb. For the LLM text verbs that's the `via` route adverb (the four shipped values), and a separate modifier picks `--model`:

| Modifier | Adverb pick | Where the result lands |
|---|---|---|
| Plain | `--via=woollama` (default) | terminal / notification (the local model replies) |
| Ctrl | `--via=clipboard` | clipboard (paste the prompt anywhere) |
| Shift | `--via=claude-desktop` | Claude Desktop (new chat) |
| Alt | `--via=claude-code` | Claude Code session |
| Super | `--model=big` (combine with the above) | as the chosen `via` |

So `F10` plain → `goo critique --via=woollama`; `F10` Ctrl → `goo critique --via=clipboard`; `F10` Alt+Super → `goo critique --via=claude-code --model=big`. The selection is captured from the PRIMARY selection / clipboard, so the verb has a subject without you naming one.

## F-row (F-Lock OFF)

The F-row carries semantic keysyms (Help, Open, Close, Reply…). These map naturally onto text/LLM verbs that borrow the selection, plus a couple of desktop actions:

| Key | Keysym | Sentence | Modifier behaviour |
|---|---|---|---|
| F1 | XF86Help | `goo think` | Shift → `--depth=really`, Ctrl → `--depth=ultra` (then `--via` as above) |
| F2 | XF86Undo | `goo do ^` | noun-first on the clipboard — list its verbs, pick one |
| F3 | XF86Redo | `goo again` | re-run the last verb+adverbs on the current selection |
| F4 | XF86New | `goo notify` | toast the selection; Ctrl → `--urgency=critical` |
| F5 | XF86Open | `goo open ^` | open whatever the clipboard addresses (path / URL) in its default app |
| F6 | XF86Close | `goo summarize` | `--via` as above |
| F7 | XF86Reply | `goo draft-response` | `--via` as above (Shift → Claude Desktop, Alt → Claude Code) |
| F8 | XF86MailForward | `goo copy-path .` | copy the cwd path to the clipboard |
| F9 | XF86Send | `goo search` | web-search the selection; Shift → `--engine=github`, Ctrl → `--engine=mdn` |
| F10 | XF86Spell | `goo critique` | `--via` as above |
| F11 | XF86Save | `goo screenshot` | whole screen → clipboard |
| F12 | XF86Print | `goo capture-region` | select a region → clipboard (Ctrl → `goo ocr-region` to OCR it instead) |

`think`, `summarize`, `critique`, and `draft-response` are selection-aware text verbs (`uses_adverbs: via, …`); `notify`, `search`, `screenshot`, `capture-region`, `ocr-region`, `copy-path` are all shipped verbs. `do ^` and `again` are core dispatch moves.

## Dedicated top-row keys (F-Lock irrelevant)

The always-on keys above the F-row suit media transport and one-shot launches:

| Key | Action |
|---|---|
| XF86AudioPlay | `goo play-pause` (media transport — no subject needed) |
| XF86AudioNext / XF86AudioPrev | `goo next` / `goo prev` |
| XF86AudioMute | `goo mute-toggle` (default audio sink) |
| XF86AudioRaiseVolume / XF86AudioLowerVolume | `goo volume-up` / `goo volume-down` |
| XF86HomePage (Web) | `goo activate :app:firefox` (focus the browser; fuzzy-match) |
| XF86Search | `goo search` on the selection (web search) |
| XF86Mail | `goo now-playing` (or bind any app: `goo activate :app:thunderbird`) |
| XF86Calculator | launch qalculate, or `goo calc` on the selection (`goo calc` evaluates clipboard text) |
| XF86Favorites | `goo do :win:` — pick a window and act on it (activate / close / move-to) |
| XF86Back / XF86Forward | `goo switch :ws:` — workspace switching (see the recon note on the F8 collision) |

The window/app/workspace bindings showcase the *non-text* half of the model: `do :win:` lists the verbs for the picked window (activate, close, move-to, maximize…); `activate :app:firefox` fuzzy-matches a running app. These take a subject by sigil rather than borrowing the selection.

## A working binding right now

The shipped CLI is what you bind. Bind F10 to `goo critique --via=clipboard` and you get a "review the selected text" hotkey that drops the rendered prompt onto the clipboard, ready to paste into whatever LLM surface you prefer. Bind F11 to `goo screenshot` and you have a one-key "screen → clipboard" grab.

COSMIC Settings → Keyboard → Custom Shortcuts:

| Shortcut | Command |
|---|---|
| `F10` | `/home/you/.local/bin/goo critique --via=clipboard` |
| `F10 + Ctrl` | `/home/you/.local/bin/goo critique --via=clipboard` |
| `F1` | `/home/you/.local/bin/goo think --via=clipboard` |
| `F1 + Shift` | `/home/you/.local/bin/goo think --depth=really --via=clipboard` |
| `F1 + Ctrl` | `/home/you/.local/bin/goo think --depth=ultra --via=clipboard` |
| `F11` | `/home/you/.local/bin/goo screenshot` |
| `F12` | `/home/you/.local/bin/goo capture-region` |
| `XF86AudioPlay` | `/home/you/.local/bin/goo play-pause` |
| `XF86HomePage` | `/home/you/.local/bin/goo activate :app:firefox` |

If you've installed cosmic-goo system-wide, point at `/usr/bin/goo` instead. If you symlinked into `~/.local/bin`, make sure that's on the COSMIC shortcut PATH (it usually is by default).

The selection is captured from `wl-paste --primary`, so for the text verbs highlight something in any app, hit the key, and the verb acts on it. The audio/screenshot/window verbs need no selection at all.

## Reconnaissance

Before binding keys, verify the keysyms your keyboard actually sends. `recon/keys.sh` runs `wev` interactively and walks you through pressing each key with F-Lock on and off. The output goes to `keysyms.log` in your CWD.

```bash
bash recon/keys.sh
```

Different MS Natural revisions and `xkb` configurations sometimes report unexpected keysyms (especially `XF86Forward` dedicated vs `F8`/`XF86MailForward` — there's a known collision on some setups). The recon output tells you which.
