# cosmic-goo

> **G**rammar **O**f **O**perations
>
> A pop-launcher plugin suite and shell library that adds noun–verb–object composition over the COSMIC launcher, with hotkey shortcuts for selection-aware actions.

---

## TL;DR

`cosmic-goo` does two things, sharing the same underlying primitives:

1. **Launcher mode** — extends `pop-launcher` (used by `cosmic-launcher`) with domain plugins and a grammar meta-plugin so the system-wide Super-key launcher can find, compose, and execute actions across tmux sessions, scenes, files, apps, workspaces, clipboard items, and fabric patterns.

2. **Hotkey mode** — binds the underused special keys on the Microsoft Natural Ergonomic 4000 (and any keyboard with comparable keys) to selection-aware actions that bypass the launcher and apply verbs directly to the current selection/clipboard.

The design is gnome-do reincarnated for COSMIC, with two-step composition (noun→verb→object) as the central UX abstraction.

---

## Vision

The COSMIC launcher (`pop-launcher` daemon + `cosmic-launcher` frontend) is already 80% of what gnome-do/Quicksilver/Albert was. What it lacks is **compositional grammar**: the ability to express *verb on object with target*, not just *find one thing and activate the default action*.

`cosmic-goo` fills that gap as a meta-plugin while also providing per-domain plugins for the data sources we care about. The same primitive layer powers a parallel hotkey path for actions that operate on the current selection — bypassing the launcher entirely because the noun is already supplied.

This is the **configuration ratchet** from the Ma framework applied to desktop actions: progressively crystallize habitual desktop operations into named verbs, addressable from either entry point.

---

## Two entry points

| Aspect | Launcher mode (Super-key) | Hotkey mode (F-row, etc.) |
|--------|--------------------------|---------------------------|
| Trigger | Super (or other launcher binding) | Single special key |
| Noun source | User typing + fuzzy search | Current selection / clipboard / window context |
| Verb source | User typing (after noun) or default | Determined by key (key → verb mapping) |
| Object source | User typing (third step, if applicable) | Implicit or via a small chooser |
| Latency | Interactive, multi-second | Sub-second |
| Use case | "what was that tmux session called?" | "summarize this selection" |

---

## Architecture

```
                    F-row hotkey                Cosmic Launcher (Super)
                         │                              │
                         ▼                              ▼
              ┌─────────────────────┐         ┌─────────────────────┐
              │  bin/goo-* scripts  │         │  pop-launcher       │
              │  (hotkey actions)   │         │  (system service)   │
              │                     │         │                     │
              └──────────┬──────────┘         └──────────┬──────────┘
                         │                              │ JSON IPC
                         │                              ▼
                         │              ┌──────────────────────────────┐
                         │              │  cosmic-goo plugins          │
                         │              │  - domain plugins (per noun) │
                         │              │  - meta-plugin (grammar)     │
                         │              └──────────────┬───────────────┘
                         │                             │
                         └──────────────┬──────────────┘
                                        ▼
                          ┌─────────────────────────────┐
                          │  cosmic-goo lib (shell + jq)│
                          │  - nouns: list/find         │
                          │  - verbs: execute           │
                          │  - selection / clipboard    │
                          │  - content dispatch         │
                          └─────────────┬───────────────┘
                                        │
                                        ▼
        cos-cli │ tmux-use │ ffs │ cliphist │ fabric │ claude:// URL handler
```

Two entry points (hotkey + launcher), same plugin/lib layer, same external tools.

---

## Grammar / composition model

### Three-layer separation

A user-visible *operation* is the composition of three things:

| Layer | What it is | Examples |
|-------|-----------|----------|
| **Key / Trigger** | The user input that initiates an action | F10, Super+`firefox sticky`, click in launcher |
| **Verb** | The named action in the grammar | `critique`, `move-to-workspace`, `activate`, `switch` |
| **Route** | How the verb executes | `fabric-api`, `claude-desktop-new`, `cos-cli`, `tmux-send-keys` |

Keys map to (verb, default route) pairs. Modifiers change the route, not the verb. The same `critique` verb executes through three different routes depending on whether plain / Shift / Alt is held.

### Sentence shape

```
[source:]subject [verb [object]]
```

| Pattern | Example | Behavior |
|---------|---------|----------|
| `subject` | `dotfiles` | Find candidate nouns across sources; apply default verb on activate |
| `subject verb` | `firefox sticky` | Find noun, apply named verb |
| `subject verb object` | `firefox move ws-2` | Full three-step |
| `source:subject` | `:tmux dotfiles` | Restrict noun search to one source |
| `verb subject` (alt order) | `move firefox` | Verb-first; second noun completes |

Subject-first is the default order (matches fuzzy-finding muscle memory). Verb-first is supported when the user types a known verb name.

### Type-mediated verb selection

Verbs are not bound to sources. They are bound to **object types**. A subject's type determines which verbs are offered for it. The same verb (`critique`, `summarize`) works on text from selection, clipboard, or file contents — they're all `text`-typed.

This means:
- Adding a new source automatically gains all verbs compatible with the types it emits.
- A verb like `move-to-workspace` works on any `app`-typed or `tmux-session`-typed noun, regardless of which source surfaced it.
- Sources still matter for scoping searches (`:tmux` narrows the noun pool), but they don't constrain verbs.

### How it maps to pop-launcher protocol

`pop-launcher` is single-step (`Search` → `Activate`). cosmic-goo's meta-plugin holds the multi-step state internally:

1. User types `firefox`. Meta-plugin queries sources in parallel, returns matching nouns with their types.
2. User picks Firefox (type: `app`). Meta-plugin emits `Fill("firefox ")` and lists verbs compatible with type `app`.
3. User types/picks `move`. The `move-to-workspace` verb requires a `workspace`-typed object; meta-plugin emits `Fill("firefox move ")` and lists workspaces.
4. User picks workspace. Meta-plugin activates: `cos-cli move --app-id firefox --workspace <id>`.

Per-item alternate verbs surface as `Context` options — single-key context menu pops the verb list for the focused item, filtered by its type.

### Verbs with implicit subjects (hotkey mode)

F-row keys carry implicit subjects supplied by context:
- `wl-paste --primary` for selection (type: `text`, sometimes detectable as `path`/`url`)
- `wl-paste` for clipboard (type: from MIME)
- `cos-cli info --json` filtered to focused window (type: `app`)

`F10` invokes `critique` on the selection-subject; same verb-execution path as the launcher's `<selection> critique` sentence.

---

## Type system

Object types are MIME types where applicable, with a small `goo/*` namespace for runtime references that aren't files.

### Type tags

| Type | Scope | Examples / values | Used for |
|------|-------|-------------------|----------|
| MIME (real) | content | `text/plain`, `text/x-python`, `application/json`, `image/png`, `text/uri-list`, `application/pdf` | text content, clipboard items, file contents |
| `goo/app` | runtime ref | (single value) | running window from cos-cli |
| `goo/workspace` | runtime ref | (single value) | Cosmic workspace |
| `goo/tmux-session` | runtime ref | (single value) | tmux session |
| `goo/scene` | runtime ref | (single value) | cosmic-goo scene |
| `goo/favorite-slot` | runtime ref | (single value) | 1–5 numeric slot |
| `goo/chat` | runtime ref | (single value) | Claude conversation (deferred) |

Type detection from clipboard uses `wl-paste --list-types`; from files uses `xdg-mime query filetype` or `file --mime-type`; from selection uses heuristics (URL regex, path test) layered on `text/plain`.

### Type aliases

For TOML authors, common type families resolve to MIME patterns:

| Alias | Expands to |
|-------|-----------|
| `text` | `text/*` |
| `code` | `text/x-python`, `text/x-c`, `text/x-rust`, `text/x-shellscript`, ... |
| `image` | `image/*` |
| `pdf` | `application/pdf` |
| `markup` | `text/html`, `text/markdown`, `application/xml` |
| `data` | `application/json`, `text/csv`, `application/x-yaml` |
| `path` | `text/uri-list`, `inode/directory`, `inode/symlink` |

Aliases are defined in `core/aliases.toml`; users and plugins can add their own.

### Type coercion

Some verbs emit secondary types (a `read` verb on `path` emits `text/plain`). Verb chains use these:

```
selection (text/plain) → critique → (no chain)
firefox-tab-url (text/uri-list) → fetch → (text/html) → summarize → (no chain)
```

Coercion is opt-in via the verb declaring `emits = "..."`. Most verbs are terminal (no emit).

---

## Definition formats

### Source definition (was: domain definition)

Each source is a TOML file describing how to list items and what type they have.

```toml
# sources/tmux-sessions.toml
name = "tmux-sessions"
prefix = "tmux"             # for :tmux scoping in launcher
icon = "utilities-terminal"
emits_type = "tmux-session"

[list]
cmd = "tmux-use --json"
# expected JSON schema: [{"id": str, "title": str, "subtitle": str?, "metadata": {}}, ...]

[preview]
cmd = "tmux list-windows -t {id} -F '#W'"
optional = true
```

Sources don't declare verbs. Verbs are declared independently with type signatures and apply to any source emitting compatible types.

### Verb definition (new)

```toml
# verbs/critique.toml
name = "critique"
accepts_type = "text"
# emits_type = "..."  # optional, for chained verbs (e.g. `read` emits `text`)

[routes.fabric-api]      # default route
template = "wl-paste | fabric -p analyze_claims"
default = true

[routes.claude-desktop-new]
template = "xdg-open 'claude://claude.ai/new?q={prompt}'"
prompt_template = """
The user is providing you a passage they want you to provide an expert review of. \
Deduce the desired intent and/or tone and critique accordingly.

---
{subject}
"""

[routes.clipboard-only]
template = "wl-copy"
prompt_template = "(same as claude-desktop-new)"
```

```toml
# verbs/move-to-workspace.toml
name = "move-to-workspace"
accepts_type = "app"        # subject must be app-typed
takes_object = true
object_type = "workspace"   # object must be workspace-typed

[routes.cos-cli]
template = "cos-cli move --app-id {subject.id} --workspace {object.index}"
default = true
```

The shell helpers `goo list <source>`, `goo verb <verb> <subject> [object]`, and `goo route <verb> <subject> --via <route>` are the only entry points scripts need to know about. The meta-plugin reads the TOML files at startup.

---

## Keymap (unchanged from prior design)

### Dedicated top-row keys (always-on, F-Lock irrelevant)

| Key | Action |
|-----|--------|
| XF86HomePage (Web) | focus-or-spawn browser anchor |
| XF86Search | focus-or-spawn Claude Desktop anchor (Shift = alpaca) |
| XF86Mail | focus-or-spawn mail/calendar anchor |
| XF86Calculator | launch qalculate |
| XF86Favorites (Star) | live workspace overview |
| Favorites 1–5 | focus-or-spawn favorite scene N (Shift = assign current to slot N) |
| XF86Back / XF86Forward | workspace/focus history navigation |
| Zoom rocker | scroll up/down (pending Cosmic verification) |

### F-row (F-Lock OFF) — keys bind to (verb, default route, modifier behavior)

| Key | Keysym | Verb | Default route | Implicit subject | Modifier behavior |
|-----|--------|------|---------------|------------------|-------------------|
| F1 | XF86Help | `think` | fabric-api | selection | Shift/Ctrl vary depth (template) |
| F2 | XF86Undo | *unassigned* | — | — | — |
| F3 | XF86Redo | *unassigned* | — | — | — |
| F4 | XF86New | `create-scene` | shell | (interactive) | Shift = assign to favorite slot |
| F5 | XF86Open | `browse-scenes` | launcher | — | Shift+selection: scene-for-path |
| F6 | XF86Close | `summarize` | fabric-api | selection | — |
| F7 | XF86Reply | `draft-response` | claude-desktop-new | selection | Alt=Code, Ctrl=clipboard |
| F8 | XF86MailForward | `new-chat-with` | claude-desktop-new | selection | Alt=Code, Ctrl=clipboard |
| F9 | XF86Send | `send-to-chat` | claude-desktop-existing | selection, chat (chooser) | Alt=Code, Ctrl=clipboard |
| F10 | XF86Spell | `critique` | fabric-api | selection | — |
| F11 | XF86Save | `save-to-notes` | clipboard-or-memory | selection | — |
| F12 | XF86Print | `visualize` | fabric-api | selection | — |

The verb name is what shows up in the launcher when you address the same action through grammar mode (`<selection> critique`). The keysym is just the physical input.

---

## Repository layout

```
cosmic-goo/                    # standalone repo
├── plugins/                   # pop-launcher plugins
│   ├── goo-meta/              # the grammar meta-plugin (multi-step composition)
│   │   ├── plugin.ron
│   │   └── goo-meta (executable, language TBD)
│   └── goo-flat/              # optional: per-source flat plugins for early phases
├── bin/                       # hotkey-mode action scripts + entry helpers
│   ├── goo                    # main entry — `goo list <source>`, `goo verb …`, `goo route …`
│   ├── goo-anchor-browser
│   ├── goo-anchor-claude
│   ├── goo-anchor-mail
│   ├── goo-key-spell          # F10 handler — invokes verb=critique on selection
│   ├── goo-key-reply          # F7 handler — invokes verb=draft-response
│   └── goo-...
├── lib/                       # shared shell utilities
│   ├── nouns.sh               # list/find from sources, type tagging
│   ├── verbs.sh               # verb lookup, route selection, execution
│   ├── selection.sh           # PRIMARY + CLIPBOARD with MIME detection
│   ├── dispatch.sh            # content-type classifier (text/path/url/code/...)
│   └── claude.sh              # claude:// URL builders
├── sources/                   # source definitions (TOML — where typed objects come from)
│   ├── selection.toml
│   ├── clipboard.toml
│   ├── apps.toml
│   ├── workspaces.toml
│   ├── tmux-sessions.toml
│   ├── files.toml
│   ├── scenes.toml
│   ├── favorites.toml
│   └── fabric-patterns.toml
├── verbs/                     # verb definitions (TOML — what can be done)
│   ├── think.toml
│   ├── critique.toml
│   ├── summarize.toml
│   ├── draft-response.toml
│   ├── send-to-chat.toml
│   ├── save-to-notes.toml
│   ├── visualize.toml
│   ├── activate.toml
│   ├── move-to-workspace.toml
│   ├── switch.toml
│   ├── kill.toml
│   └── ...
├── routes/                    # route definitions (TOML — how verbs execute)
│   ├── fabric-api.toml
│   ├── claude-desktop-new.toml
│   ├── claude-desktop-existing.toml
│   ├── claude-code-new.toml
│   ├── claude-code-existing.toml
│   ├── cos-cli.toml
│   └── clipboard-only.toml
├── scenes/                    # scene catalog (TOML — instances of the scene type)
├── keymap/                    # cosmic-comp shortcut config generator
│   └── generate.sh
├── recon/                     # one-time env capture scripts
│   ├── env.sh
│   └── keys.sh
├── doc/                       # cheatsheet + this spec
└── README.md
```

Three TOML directories carry the heart of the system:
- `sources/` — *where* objects come from (and what type they have)
- `verbs/` — *what* can be done (with type signatures)
- `routes/` — *how* verbs execute (with command templates)

Adding a new domain = one TOML in `sources/`. Adding a new action = one TOML in `verbs/`. Adding a new endpoint = one TOML in `routes/`. None of these requires editing the meta-plugin.

---

## Implementation phases

### Phase 0 — recon (in progress)
- Confirm `claude://` URL handler works on aaddrick Linux build
- Verify pop-launcher is installed and scriptable
- Capture all keysyms with `wev`
- Smoke-test `wl-paste --primary` reliability under Cosmic
- Install/build cos-cli

### Phase 1 — minimum viable end-to-end (~1 week)
Three things, proving all three endpoint types and both entry points:

1. **`goo-anchor-browser`** (hotkey script) — focus-or-spawn browser. Validates cos-cli integration and the lib/window primitives.
2. **`goo-verb-spell`** (hotkey script) — selection → fabric `analyze_claims` → notification. Validates the fabric path and selection capture.
3. **`goo-verb-reply`** (hotkey script) — selection → `claude://claude.ai/new?q=...` with reply template. Validates the URL-handler path.

No pop-launcher plugins yet. No grammar yet. Just three hotkey scripts and the lib that supports them.

### Phase 2 — first pop-launcher plugin
One simple flat-list plugin (probably **scenes**), as a script-style plugin in `~/.local/share/pop-launcher/scripts/`. Proves the launcher integration without committing to a real plugin process yet.

### Phase 3 — real domain plugin
**tmux-sessions** as a full plugin with per-item context menus (switch, kill, rename). Validates the JSON IPC protocol and context-menu pattern. Language TBD — Rust if we're going there eventually, but Go or Python would ship faster for v1.

### Phase 4 — grammar meta-plugin
The `goo-meta` plugin. Owns multi-step composition. Other domain plugins continue working in parallel.

### Phase 5 — broadening
Files, apps, workspaces, clipboard, fabric-patterns plugins. content-dispatch graduates from a stub to real sitting_duck integration. Favorite-chats domain if we still want it.

---

## Open questions

- **Meta-plugin implementation language.** Bash for v1 (cheap, iterates fast); Rust if/when it stabilizes. Avoid premature Rust.
- **Selection daemon.** If `wl-paste --primary` proves flaky in Cosmic, we need a small daemon that watches selection and caches it. Postpone until recon results.
- **F2/F3 (Undo/Redo).** Still undefined. Strong candidates: re-open last closed tmux session/window/pane; clipboard history step. Leave alone until something earns the slot.
- **content-dispatch as separable repo.** Stays in `lib/` until it grows enough to justify its own life. Not before sitting_duck integration.
- **WindowRules for anchor pinning.** Track [cosmic-comp #2214](https://github.com/pop-os/cosmic-comp/pull/2214). If/when it lands, anchors can be declared declaratively and `goo-anchor-*` scripts simplify to just `cos-cli activate`.

---

## Reconnaissance status

| Item | Status |
|------|--------|
| Cosmic D-Bus interfaces | known: `com.system76.CosmicComp.*` exists; less complete than Hyprland IPC |
| Workspace API | covered by `cos-cli` (third-party, vendor as dep) |
| pop-launcher plugin protocol | known and documented |
| `claude://` URL scheme | documented for macOS/Windows; needs Linux verification (recon script item) |
| `wl-paste --primary` | needs Cosmic-specific reliability check |
| MS NE 4000 keysyms | needs `wev` capture (recon script item) |
| Cosmic dynamic workspaces | confirmed; addressing must be by app_id, not slot |
| `COSMIC_DATA_CONTROL_ENABLED` | known requirement; security trade-off documented |

See `cosmic-scenes-recon.sh` and `cosmic-scenes-recon-keys.sh` (to be renamed in this repo as `recon/env.sh` and `recon/keys.sh`) for the actual checks.

---

## Glossary

- **GOO** — Grammar Of Operations. The compositional layer cosmic-goo adds over pop-launcher's flat search-and-activate model.
- **Source** — a place that emits typed objects (apps via cos-cli, scenes from the catalog, text via wl-paste). Each source declares one primary emitted type. Was previously called "domain."
- **Type** — what an object IS (`text`, `path`, `app`, `workspace`, `scene`, `tmux-session`, etc.). Verbs target types; sources emit types. The same type can be emitted by multiple sources.
- **Verb** — a named action (`critique`, `move-to-workspace`, `switch`). Verbs declare which input types they accept and optionally an object type for two-step composition. Verbs are not domain-scoped; they apply across all sources emitting the matching type.
- **Route** — how a verb executes (`fabric-api`, `claude-desktop-new`, `cos-cli`, etc.). Each verb has a default route; modifiers in hotkey mode override the route, not the verb.
- **Key** — a keyboard binding (e.g., F10). Keys map to `(verb, default route, implicit-subject source)` tuples. Distinct from the verb itself.
- **Scene** — a named work context (workspace + apps + tmux session + cwd). Scenes are objects of the `scene` type, emitted by the scenes source.
- **Anchor** — a singleton scene that's expected to exist throughout a session (browser, mail, Claude Desktop). Focus-or-spawn semantics.
- **Hotkey mode** — verb-on-implicit-subject, triggered by a single keypress, bypasses the launcher. The subject's source is fixed by the key binding (usually `selection`).
- **Launcher mode** — interactive composition via pop-launcher, triggered by Super or another launcher binding. The user composes a full sentence (subject → verb → object).
