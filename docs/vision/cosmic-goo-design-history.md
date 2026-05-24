# ARCHIVED — Design History

> This document captures the design evolution that led to cosmic-goo.
> It is preserved as historical context; the current spec is **cosmic-goo-spec.md**.
> Names and architecture in this document predate the rename and the pop-launcher pivot.

---

# Cosmic Scenes

Design notes for a keyboard-driven workspace and context layer on top of Cosmic (Pop!_OS), driven by the Microsoft Natural Ergonomic 4000 keyboard's underused special keys.

## Vision

Two organizing ideas:

1. **Scenes are crystallized work contexts** — explicitly-defined or progressively-captured combinations of workspace + apps + tmux session + working directory. The lifecycle (capture → favorite → config) is the *configuration ratchet* from the Ma framework, applied to desktop management.

2. **Three orthogonal key axes**:
   - **Dedicated top-row keys** = *where am I* (workspace navigation)
   - **Dedicated nav keys** (Back/Forward, Zoom) = *how did I get here* (history & view state)
   - **F-row keys** (F-Lock OFF) = *what do I do with this* (content actions on the current selection)
   - **Modifiers** = depth, variant, assignment (cross-cutting)

The whole layout dogfoods existing investments: `tmux-use` handles folder→session resolution, `ffs` provides filesystem features, the DuckDB toolchain (`sitting_duck`) can power content classification, and either `fabric` or a fabric-shaped wrapper handles LLM prompt templates.

---

## Feature Set

### Workspace Anchors (Singleton Workspaces)
Hard-to-find workspaces in a sea of project workspaces get dedicated keys. **Focus-or-spawn semantics**: switch to the existing instance if present; spawn at end if missing.

- Browser workspace
- Mail/calendar workspace
- Claude Desktop workspace

### Numbered Favorite Scenes (1–5)
Quick-jump bookmarks for project contexts. Defined via three paths:
1. **Config file** (`~/.dotfiles/cosmic-scenes/scenes/*.toml` — persistent, versionable)
2. **Capture mode** (point at a running workspace: "this is now scene N", with depth options)
3. **XF86New** (interactive: new alacritty + cwd + tmux session stack, optional favorite-slot assignment)

### Scene Catalog (XF86Open)
Browse all defined scenes (active or dormant). Includes context-aware mode: **selection + Shift+Open** opens the scene matching the selected path/repo.

### Content Actions on Selection (F-row)
Selection-aware AI/automation actions. Each key applies a templated operation to the current selection or clipboard, with handling for files, paths, images, URLs, text, code, and data tables.

### Workspace/Focus History (Back/Forward)
History-aware navigation through recently-visited workspaces and windows. Separate from numeric workspace switching (Meta+1..9), which addresses by slot.

### Session Resurrection (TBD slot)
Re-opening last closed tmux session/window/pane is an attractive candidate for *some* key (the missing Ctrl+Shift+T for terminal workflows). Not currently bound to Undo/Redo — those keys are intentionally left undefined while we figure out the best fit.

---

## Keymap

### Dedicated top-row keys (always-on, F-Lock irrelevant)

| Key | Action | Modifier behavior |
|-----|--------|-------------------|
| XF86HomePage (Web) | focus-or-spawn browser anchor | — |
| XF86Search | focus-or-spawn Claude Desktop anchor | Shift = alpaca |
| XF86Mail | focus-or-spawn mail/calendar anchor | — |
| XF86Calculator | launch qalculate (transient, not anchored) | — |
| XF86Favorites (Star) | live workspace overview | — |
| Favorites 1–5 | focus-or-spawn favorite scene N | Shift = assign current to slot N |
| XF86Back (dedicated nav) | step back in workspace/focus history | — |
| XF86Forward (dedicated nav) | step forward in history | — |
| Zoom rocker | scroll up/down (was zoom in/out) | TBD: Cosmic compat |

### F-row (F-Lock OFF)

| Key | Keysym | Action | Shift | Alt | Ctrl |
|-----|--------|--------|-------|-----|------|
| F1 | XF86Help | think on selection | really think | — | ultrathink |
| F2 | XF86Undo | *undefined — TBD* | — | — | — |
| F3 | XF86Redo | *undefined — TBD* | — | — | — |
| F4 | XF86New | new scene wizard (alacritty + cwd + tmux) | assign to favorite slot | — | — |
| F5 | XF86Open | scene catalog browser | open scene for selection (dir/repo) | — | — |
| F6 | XF86Close | summarize / wrap up selection | — | — | — |
| F7 | XF86Reply | draft response to selection | — | route to Claude Code | clipboard |
| F8 | XF86MailForward | new chat with selection attached | — | Claude Code | clipboard |
| F9 | XF86Send | send selection to existing chat (chooser) | — | Claude Code | clipboard |
| F10 | XF86Spell | critique / review | — | — | — |
| F11 | XF86Save | persist to memory/notes | — | — | — |
| F12 | XF86Print | visualize as infographic | — | — | — |

**Modifier convention** (revised after discovering the official `claude://` URL scheme):

| Modifier | Endpoint | Mechanism | Where result lands |
|----------|----------|-----------|---|
| Plain | Fabric → Anthropic API | `wl-paste \| fabric -p X` | terminal / notification |
| Alt | Claude Code (new session) | `claude://code/new?q=...&folder=...` | new Code session in Desktop |
| Alt+Shift | Claude Code (existing tmux) | `tmux send-keys` to active session | existing terminal |
| Shift | Claude Desktop (new chat) | `claude://claude.ai/new?q=...` | new chat in Desktop |
| Shift+Alt | Claude Desktop (specific chat) | `claude://claude.ai/chat/{uuid}` from chooser | resumes existing chat |
| Ctrl | clipboard only | assemble + copy, don't send | clipboard |

**Reply/Forward semantics:** these probably *invert* the default to Shift (i.e., land in Claude Desktop UI) because they want conversation continuity. Other F-row actions stay on the Fabric default.

**Constraint:** the `q` parameter is truncated to ~14,000 characters. For selections beyond that, fall back to: (a) Fabric path (API has higher limits), (b) inline a fragment with a "see clipboard for full content" note, or (c) save to tmpfile and reference path (but `file` parameter is "accepted but not currently supported" — so the user reads from tmpfile manually).

---

## Architecture

### Project structure (initial)

`cosmic-scenes` is a collection of repos under `~/.dotfiles` (submodules) or as siblings:

```
~/.dotfiles/
├── cosmic-scenes/             # The new project (this design)
│   ├── bin/                   # Scene scripts (one per binding)
│   ├── lib/                   # Shared utilities (sourced)
│   ├── scenes/                # User scene definitions (TOML)
│   ├── keymap/                # Cosmic-comp config generator
│   └── doc/                   # Cheatsheet, this design doc
├── tmux-use/                  # Existing — folder→session
├── ffs/                       # Existing — filesystem features
└── content-dispatch/          # New (TBD) — clipboard/selection classifier
                               # Possibly part of cosmic-scenes/lib/ initially
External:
└── fabric/                    # Forked or used as dependency
```

`content-dispatch` starts inside `cosmic-scenes/lib/` and graduates if `sitting_duck` integration grows it into its own thing.

### Layered model

```
┌──────────────────────────────────────────────┐
│  Keymap (cosmic-comp config, generated)      │
└─────────────────────┬────────────────────────┘
                      │ keybinding fires
┌─────────────────────▼────────────────────────┐
│  Scene scripts (bin/scene-*.sh)              │
│  Thin shims: parse args → call lib functions │
└─────────────────────┬────────────────────────┘
                      │ sources
┌─────────────────────▼────────────────────────┐
│  Scenes lib (lib/*.sh)                       │
│   ├── window   : find / focus / spawn        │
│   ├── workspace: list / switch / insert      │
│   ├── selection: PRIMARY + CLIPBOARD         │
│   ├── dispatch : classify clipboard content  │
│   └── tmux     : delegate to tmux-use        │
└──┬───────────────────┬───────────────────┬───┘
   │                   │                   │
┌──▼──────────┐   ┌────▼──────┐   ┌────────▼──┐
│ Cosmic IPC  │   │ tmux-use  │   │ fabric    │
│ (D-Bus?)    │   │           │   │ (or eqv)  │
└─────────────┘   └───────────┘   └───────────┘
```

### Claude Desktop URL handler (key architectural simplification)

Claude Desktop registers a `claude://` URL scheme handler ([official docs](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link)). This eliminates the need for UI automation. Supported routes:

| URL | Effect |
|-----|--------|
| `claude://claude.ai/new?q=<prompt>` | Open new chat with prompt prefilled (not auto-sent) |
| `claude://claude.ai/chat/{uuid}` | Open specific existing chat |
| `claude://claude.ai/project/{uuid}` | Open specific project |
| `claude://code/new?q=<prompt>&folder=<cwd>` | New Claude Code session with prompt and working dir |
| `claude://cowork/new?q=<prompt>&folder=<cwd>&file=<file>` | New Cowork session |

Trigger via `xdg-open "claude://..."`. Constraints:
- `q` truncated at ~14,000 characters
- `file` attach is "accepted but not currently supported"
- Folder paths shown in a confirmation dialog before adopted (Code/Cowork)

This is documented for macOS/Windows; Linux (aaddrick build) is the same Electron app and should honor the handler if the `.desktop` file with `x-scheme-handler/claude=...` is installed. Verify before designing around it.

### Key decisions to lock in early

- **Scene definition format**: TOML at `~/.dotfiles/cosmic-scenes/scenes/*.toml` (templates, versioned). Runtime instance state at `~/.local/state/cosmic-scenes/` (machine-local). Dotfiles = catalog; local = live.
- **Capture depth**: ship **shallow** (workspace # + app classes + cwd) and **manifest** (declarative TOML) only. Medium (tmux layout, browser tabs) and deep (open files, cursor) deferred — too brittle for payoff.
- **Singleton semantics**: each scene declares `singleton: true|false`. Anchors are singletons. Project scenes default to singleton. Capture mode default = singleton.
- **Anchor addressing**: by **app_id**, not workspace slot. Cosmic workspaces are dynamic by default — empty workspaces auto-disappear, new ones auto-appear. Slot numbers are unstable. `cos-cli info --json` is the source of truth for "where does Firefox live right now."
- **Anchor ordering sugar deferred indefinitely.** With dynamic workspaces, position-based ordering is fighting the OS. If we want canonical positions, the right primitive is the upcoming [cosmic-comp WindowRules workspace assignment](https://github.com/pop-os/cosmic-comp/pull/2214), not our own move logic.

---

## Utilities

### Primitives (`lib/`)

| Function | Purpose | Backed by |
|----------|---------|-----------|
| `find_window(criteria)` | Locate window by app_id (partial, case-insensitive) | `cos-cli info --json` + jq |
| `focus_app(app_id\|index)` | Focus an app | `cos-cli activate` |
| `focus_or_spawn(app_id, cmd, ws_hint)` | Anchor primitive: if exists focus, else launch + move | composed (cos-cli + xdg-open + cos-cli move --wait) |
| `enumerate_apps()` | List apps with workspace metadata | `cos-cli info --json` |
| `enumerate_workspaces()` | List workspace groups + workspaces | `cos-cli info --json` |
| `current_selection()` | Get PRIMARY selection text + MIME | `wl-paste --primary` (verify Cosmic reliability) |
| `current_clipboard()` | Get clipboard with MIME inventory | `wl-paste -l` + `wl-paste -t` |
| `dispatch_content(blob)` | Classify selection type → handler key | sitting_duck + heuristics |
| `tmux_session_for(dir)` | Resolve canonical session name | tmux-use |
| `scene_create(name, def)` | Add new scene definition | shell + file writes |
| `scene_capture(ws, depth)` | Snapshot workspace as scene | cos-cli info + introspection |
| `scene_open(name)` | Instantiate scene | composed |
| `claude_send_new(prompt)` | New Claude Desktop chat with prompt | `xdg-open "claude://claude.ai/new?q=..."` |
| `claude_send_code(prompt, dir)` | New Claude Code session | `xdg-open "claude://code/new?q=...&folder=..."` |
| `fabric_send(pattern, content)` | API call via Fabric | `echo content \| fabric -p pattern` |

### External integrations to evaluate or adopt

- **[cos-cli](https://github.com/estin/cos-cli)** — third-party Rust CLI for Cosmic window/workspace ops. Solves `find_window`/`activate`/`move`. Strong candidate to vendor as a runtime dependency.
- **fabric** — prompt template library (v1.4.447+, Apr 2026). 250+ patterns, Anthropic SDK, Unix pipes, multi-provider. Strong candidate for direct use.
- **Clipboard stack**: `wl-clipboard` (base) + `cliphist` (history, text+image) + `wl-clip-persist` (fixes Wayland's "clipboard disappears when source closes"). Requires `COSMIC_DATA_CONTROL_ENABLED=1` in env for wlr-data-control to work.
- **rofi-wayland / wofi / fuzzel** — launcher UIs for scene catalog and chat chooser.
- **grim + slurp + zbarimg** — QR-from-screen as `grim -g "$(slurp)" - | zbarimg -`.
- **wev** — keysym verification (run first to confirm what each key actually sends).

### Reconnaissance to do before more design

1. **`wev` capture** every special key, F-Lock on and off → confirm keysyms and detect collisions (especially XF86Forward dedicated vs F8 XF86MailForward)
2. **`busctl --user list | grep -i cosmic`** → enumerate Cosmic D-Bus interfaces
3. **Check `cosmic-comp` docs** for workspace ordering primitives
4. **Test `wl-paste --primary`** reliability under Cosmic
5. **Verify `claude://` URL handler** is registered: `xdg-mime query default x-scheme-handler/claude` — should return something like `claude-desktop.desktop`. If not, the URL-handoff architecture won't work and we revert to UI automation.
6. **Smoke-test `xdg-open "claude://claude.ai/new?q=Hello"`** — does it actually open Claude Desktop with the prefilled prompt?
7. **Decide clipboard manager**: cliphist vs gpaste vs clipman

---

## Open Questions

- **Workspace addressing**: settled — by app_id via cos-cli, not slot numbers. Cosmic's dynamic workspaces make slot-based addressing unstable.
- **Anchor ordering sugar**: deferred to upcoming WindowRules feature; not implementing ourselves.
- **Content dispatcher**: how much `sitting_duck` integration day one vs. starting with simple heuristics (path regex, URL regex, MIME from clipboard)? Probably heuristics first; sitting_duck for the code-aware path later.
- **Fabric vs. roll-your-own**: leaning use-directly given the research; final decision after smoke test.
- **Scene instance multiplicity**: when a non-singleton scene is opened and an instance exists — open a second, or focus the first? Likely: focus by default; Shift forces new.
- **Wayland selection reliability**: PRIMARY selection on Wayland is patchy. May need a small daemon that watches selection and caches it for scripts to read. Recon item.
- **Send-to-existing-chat**: deferred. Default Shift+key → new chat. Revisit if friction proves real.

---

## Next: Fabric Evaluation

Specific questions to answer when we look at fabric:

1. **Pattern coverage** — does its built-in library include analogues of Reply / Summarize / Critique / Visualize / Memorize?
2. **Input pipeline** — clipboard support? File paths as input? Multi-modal (images, PDFs)?
3. **Routing** — can it route different content types to different patterns, or is that our responsibility?
4. **Backends** — does it support Claude Code, Anthropic API directly, and Ollama (alpaca path)? Note: Claude Desktop no longer requires API integration — we route via `claude://` URL handler, which sidesteps fabric entirely for that endpoint.
5. **Composability** — can we wrap it so F-row scripts are 3-line shims, or are we fighting its model?
6. **Customization** — easy to add new patterns? Versionable in dotfiles?

**Decision matrix:**
- **Use directly** if patterns + Anthropic SDK + Ollama all work and CLI composes cleanly
- **Fork and extend** if patterns are good but routing or backends miss
- **Inspired but new** if the model is wrong

**Updated assessment after research:** Fabric is v1.4.447+ as of April 2026, supports Claude Opus 4.7, Anthropic SDK, Ollama, Unix piping, REST API mode, 250+ patterns. Strong candidate for direct use. The earlier "Claude Desktop has no API" sticking point dissolved — we just don't route Fabric to Claude Desktop. Fabric handles the Plain modifier path (API directly), and `claude://` URLs handle the Shift/Alt paths.

**Adjacent tools worth knowing about:**
- `claudric` — bash wrapper piping content through Claude Code with Fabric `system.md` prompts (overlaps significantly with what we're building)
- `fabric-decomp` — converted all 251 Fabric patterns to native Claude Code skills (if we lean heavily on Claude Code, this is the pattern library without the Fabric binary)
- `fabric-mcp` — Claude Desktop side: exposes Fabric patterns as MCP tools so Claude can call them mid-conversation
