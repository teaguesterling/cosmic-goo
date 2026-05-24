# cosmic-goo

> **G**rammar **O**f **O**perations
>
> A pop-launcher plugin suite and shell library that adds compositional grammar — subject → verb → object — over the COSMIC launcher. Inspired by gnome-do / Quicksilver. Designed to be invoked from anywhere: hotkeys, scripts, the launcher inline, or a dedicated compose dialog.

---

## TL;DR

cosmic-goo extends the COSMIC launcher with **compositional sentences**: pick a noun (an app, a tmux session, a chunk of text), pick a verb (move, critique, summarize), optionally pick an object (a workspace, a chat), execute. Modifiers (`--via=...`, `--depth=...`) live as named **adverbs** on verbs.

The same machinery works from the CLI (`goo critique --via=fabric`), making it scriptable and bindable to any key. cosmic-goo doesn't ship its own key layout; users bind keys via COSMIC settings (or any other mechanism).

A compose dialog is reachable on demand from the launcher, for sentences too complex for inline composition. It's not the default UI — most invocations go through the launcher inline or the CLI directly.

---

## Vision

The COSMIC launcher (`pop-launcher` daemon + `cosmic-launcher` frontend) is 80% of what gnome-do/Quicksilver/Albert was — minus the **compositional grammar**. cosmic-goo fills that gap:

- As a **pop-launcher meta-plugin** that handles noun→verb→object composition with type-aware autocomplete
- As a **CLI** (`goo`) for scripting and hotkey bindings
- As an **on-demand compose dialog** for richer multi-step interactions

The whole system is **plugin-driven**. A plugin is a TOML file declaring any combination of types, sources, verbs, adverbs, and shell helpers. Adding tmux integration is one TOML plus an updated `tmux-use --json` flag. Adding a new endpoint (say, a hypothetical `gemini-cli`) is one TOML declaring a new adverb value for `via`.

---

## Three invocation modes

| Mode | Trigger | Subject source | Use case |
|------|---------|----------------|----------|
| **CLI** | `goo <verb> [...]` from anywhere | explicit args, or implicit from selection/clipboard | scripts, hotkey bindings, terminal use |
| **Launcher inline** | Super → type | typed and resolved live | "what was that tmux session?" |
| **Compose dialog** | Promote from launcher (sigil) or `goo compose` from CLI | seeded from inline state, or fresh | full noun-verb-object-adverbs flow with previews |

All three modes share the same plugin/type/verb/adverb backend. The CLI is the universal contract; the launcher and dialog are frontends.

---

## Architecture

```
       Any hotkey, script, or terminal              Cosmic Launcher (Super)
                       │                                    │
                       ▼                                    ▼
              ┌──────────────────┐               ┌──────────────────────┐
              │  goo CLI         │               │  pop-launcher daemon │
              │  (bin/goo)       │               │  + meta-plugin       │
              └────────┬─────────┘               └──────────┬───────────┘
                       │                                    │
                       │                  ╔═════════════════╪═════════════════╗
                       │                  ║   (on promote)  │ Fill("🌌...")   ║
                       │                  ║                 ▼                 ║
                       │                  ║          ┌─────────────────┐      ║
                       │                  ║          │ goo-compose     │      ║
                       │                  ║          │ (libcosmic GUI) │      ║
                       │                  ╚═══════════════════════════════════╝
                       │                                    │
                       └──────────────────┬─────────────────┘
                                          ▼
                          ┌─────────────────────────────────┐
                          │   cosmic-goo lib                │
                          │   plugin loader, type resolver, │
                          │   verb dispatch, adverb apply   │
                          └─────────────────┬───────────────┘
                                            │
                                            ▼
        cos-cli │ tmux-use │ ffs │ cliphist │ fabric │ claude:// URLs │ wl-paste
```

The CLI is the canonical entry point. Both the launcher meta-plugin and the compose dialog ultimately invoke the same underlying lib.

---

## Type system

**Everything is a MIME type.** No separate kingdom for handles.

| Kind | Examples | Where defined |
|------|----------|---------------|
| Standard content | `text/plain`, `text/markdown`, `text/x-python`, `image/png`, `inode/directory` | IANA / xdg-mime / libmagic |
| Vendor handles | `application/vnd.tmux-use.session`, `application/vnd.cos-cli.app`, `application/vnd.cos-cli.workspace`, `application/vnd.cosmic-goo.scene` | Plugins declare in their own vendor namespace |

**Plugins own their vendor namespaces.** `tmux-use`'s plugin defines `application/vnd.tmux-use.session`; `cos-cli`'s plugin defines `application/vnd.cos-cli.app`. cosmic-goo itself owns `application/vnd.cosmic-goo.*` for things it produces internally (scenes, favorite-slots).

**Type matching uses MIME globs.** Verbs declare `accepts = ["text/*"]` or `accepts = ["application/vnd.cos-cli.app", "application/vnd.tmux-use.session"]`. cosmic-goo implements the matcher (it's small).

**Type inference for raw content:**

| Source | Detection |
|--------|-----------|
| Wayland clipboard | `wl-paste --list-types` returns MIME natively |
| File paths | `xdg-mime query filetype` or `file --mime-type` |
| Selection text | libmagic + heuristics (URL regex → `text/x-uri`, path detection → `inode/file` after stat) |
| Sources emitting handles | declared in the source's TOML |

---

## Plugins

A plugin is a TOML file at `plugins/<name>.toml` (simple case) or a directory `plugins/<name>/` containing `plugin.toml` plus binaries/scripts (rich case). The loader scans both forms.

A plugin contributes any combination of: **types**, **sources**, **verbs**, **adverbs**. Most plugins contribute one of these; some bundle several.

### Example: a complete plugin in one file

```toml
# plugins/tmux.toml
name = "tmux"
description = "tmux session source and verbs"

# Declare a vendor MIME owned by tmux-use
[[types]]
name = "application/vnd.tmux-use.session"
display = "tmux session"
kind = "handle"

# A source emitting that type
[[sources]]
name = "tmux-sessions"
prefix = "tmux"           # for :tmux scoping in launcher
icon = "utilities-terminal"
emits = "application/vnd.tmux-use.session"
list_cmd = "tmux-use --json"
preview_cmd = "tmux list-windows -t {subject.id} -F '#W'"

# Verbs that accept this type
[[verbs]]
name = "switch"
accepts = ["application/vnd.tmux-use.session"]
default_for = "application/vnd.tmux-use.session"
cmd = "tmux-use switch {subject.id}"

[[verbs]]
name = "kill"
accepts = ["application/vnd.tmux-use.session"]
cmd = "tmux kill-session -t {subject.id}"
confirm = true

[[verbs]]
name = "rename"
accepts = ["application/vnd.tmux-use.session"]
takes_string = true       # prompts for a string object
cmd = "tmux rename-session -t {subject.id} {object}"
```

### Example: an adverb-only plugin

```toml
# plugins/claude-routing.toml
name = "claude-routing"
description = "Routes text verbs to Claude Desktop, Claude Code, or clipboard"

[[adverbs]]
name = "via"
kind = "selector"
applies_to = ["text/*"]
default = "fabric"

[adverbs.via.values.fabric]
description = "Send to Anthropic API via Fabric"
template = "echo {verb.prompt} | fabric -p {verb.fabric_pattern}"

[adverbs.via.values.claude-desktop]
description = "Open new chat in Claude Desktop"
template = "xdg-open 'claude://claude.ai/new?q={url-encode {verb.prompt}}'"

[adverbs.via.values.claude-code]
description = "Open new Claude Code session"
template = "xdg-open 'claude://code/new?q={url-encode {verb.prompt}}&folder={cwd}'"

[adverbs.via.values.clipboard]
description = "Copy assembled prompt to clipboard"
template = "echo {verb.prompt} | wl-copy"
```

### Example: a text-verbs plugin using the adverbs above

```toml
# plugins/text-verbs.toml
name = "text-verbs"

[[verbs]]
name = "critique"
accepts = ["text/*"]
uses_adverbs = ["via", "style"]   # optional; lists which adverbs are valid here
fabric_pattern = "analyze_claims"
prompt = """
You are providing expert review of the following passage.
Deduce the desired intent and tone, then critique accordingly.

---
{subject.text}
"""

[[verbs]]
name = "summarize"
accepts = ["text/*"]
uses_adverbs = ["via", "length"]
fabric_pattern = "summarize"
prompt = "Summarize the following:\n\n{subject.text}"

[[verbs]]
name = "think"
accepts = ["text/*"]
uses_adverbs = ["via", "depth"]
fabric_pattern = "extract_wisdom"
prompt = "{depth_prefix} the following:\n\n{subject.text}"

# A depth adverb declared inline for `think` (could live in its own plugin)
[[adverbs]]
name = "depth"
kind = "selector"
applies_to_verbs = ["think"]
default = "normal"

[adverbs.depth.values.normal]
template_var = { depth_prefix = "Think carefully about" }

[adverbs.depth.values.really]
template_var = { depth_prefix = "Really think — deeply consider —" }

[adverbs.depth.values.ultra]
template_var = { depth_prefix = "Ultrathink: exhaustively analyze every angle of" }
```

### Plugin discovery

- Built-in: `/usr/share/cosmic-goo/plugins/`
- System-wide: `/etc/cosmic-goo/plugins/`
- User: `~/.config/cosmic-goo/plugins/`
- Project-local: `$PWD/.cosmic-goo/plugins/` (for scene-scoped overrides)

Later wins; user can override built-in plugins. The loader resolves types, sources, verbs, and adverbs into a single registry at startup.

---

## Verbs and adverbs

### Verbs

A verb has:
- A name (`critique`, `move-to-workspace`)
- A set of accepted input types (MIME globs)
- Optionally, a single object type (for two-step verbs)
- A template, OR a set of adverb-selected templates
- Optional `confirm = true` for destructive operations

Verbs are looked up by type compatibility. Given a subject of type `application/vnd.cos-cli.app`, the registry returns all verbs whose `accepts` matches — across all loaded plugins.

### Adverbs

Adverbs modify *how* a verb is performed. Two flavors:

| Kind | Behavior | Example |
|------|----------|---------|
| **selector** | Picks among a known set of alternatives, each with its own template or template fragment | `--via=fabric\|claude-desktop\|...`, `--depth=normal\|really\|ultra` |
| **fill** | Takes a free-form value, inserted into the verb's template | `--name=<string>`, `--to-chat=<uuid>` |

Adverbs declare `applies_to` (a list of types) or `applies_to_verbs` (specific verb names) to scope where they're valid.

### Modifier keys → adverb values

Modifier keys in a key binding select adverb values; they don't change the verb. From the CLI side:

```
goo critique                              # default adverbs
goo critique --via=claude-desktop         # explicit
goo think --depth=ultra                   # selector adverb
goo think --depth=ultra --via=clipboard   # multiple adverbs
goo send --to-chat=$UUID                  # fill adverb (with chooser fallback)
```

A keybinding for `F10 Shift` invokes `goo critique --via=claude-desktop` from the user's chosen shortcut tool (COSMIC settings, sxhkd, etc.).

---

## Launcher integration

### Inline composition grammar

The meta-plugin parses the launcher input as a left-to-right sequence of tokens. Each token's role is determined by its position and sigil:

| Token form | Meaning | Example |
|-----------|---------|---------|
| `/.../` | Content subject (text from selection / clipboard / file) | `/selected text/` |
| bare name | Handle subject (resolved against sources) | `firefox` |
| `:prefix name` | Subject restricted to a source | `:tmux dotfiles` |
| `*name*` | Verb | `*critique*` |
| `--adverb=value` | Selector adverb (named value) | `--via=fabric` |
| `--adverb value` | Fill adverb (free value) | `--name "My Scene"` |
| bare name after verb | Object | `ws-2` |

The displayed text uses sigils for visual structure. The user can also type without sigils — the meta-plugin parses syntax and re-formats with sigils via `Fill`.

### Autocomplete mechanics

Tab confirms and advances. Down arrow cycles values for the current token. Space and `=` enter the next position. The autocomplete pool depends on which slot the cursor is in:

| Cursor position | Completion pool |
|-----------------|-----------------|
| Empty / start | all source items (matching typed prefix) + the standard shortcuts (`sel`, `clip`, `app:...`, `file:...`) |
| After subject | verbs whose `accepts` matches the subject's type |
| After verb taking an object | sources/items emitting the object type |
| After `--` | adverbs whose `applies_to` matches the verb/type |
| After `=` (selector adverb) | known values for that adverb |

**Worked example** (your keystroke sequence):

```
Keystrokes:  sel<TAB>cr<TAB>s<DOWN><TAB>v<TAB>f<TAB>
Renders to:  /selected text/ *critique* --style=harshly --via=fabric
```

- `sel<TAB>` — `sel` filters subjects; only `selection` matches; Tab confirms; meta-plugin emits `Fill("/selected text/ ")` substituting the current selection's display form.
- `cr<TAB>` — `cr` filters verbs accepting `text/*`; `critique` matches; Tab confirms; Fill becomes `Fill("/selected text/ *critique* ")`.
- `s<DOWN><TAB>` — `s` filters adverbs applicable to `critique`; `--style` matches; Down cycles its values (`gently`, `firmly`, `harshly`, ...); Tab confirms current value.
- `v<TAB>f<TAB>` — `v` filters remaining adverbs; `--via` matches; Tab opens value selection; `f` filters values; `fabric` matches; Tab confirms.
- Enter executes; Tab one more time prompts whether to promote to compose dialog.

### Promotion to compose dialog

A specific synthetic launcher result, displayed with a `🌌` sigil and labeled like `🌌 Open in compose dialog`, is always present when the input contains a partial sentence. Activating it:

1. Meta-plugin writes the parsed state to `/run/user/$UID/cosmic-goo/compose-state.json`
2. Meta-plugin spawns `goo-compose` (or signals an existing daemon)
3. Launcher closes; compose dialog opens, pre-filled with what was assembled

Alternative invocation: typing `>>` or `compose:` as the leading prefix in the launcher immediately promotes (no need to assemble first).

---

## Compose dialog (goo-compose)

A small libcosmic/iced application. Three panels (subject / verb / object), with an adverb tray at the bottom.

```
┌─ goo-compose ──────────────────────────────────────────────────┐
│                                                                │
│  ┌─Subject──────┬─Verb─────────┬─Object─────────────┐          │
│  │ /selected/   │ ▸ critique   │ (none required)    │          │
│  │ firefox      │   summarize  │                    │          │
│  │ dotfiles     │   think      │                    │          │
│  │ ...          │   reply      │                    │          │
│  └──────────────┴──────────────┴────────────────────┘          │
│       [text/*]   →  [accepts text/*]  →   (terminal)           │
│                                                                │
│  Adverbs: [--via fabric ▾]  [--style normal ▾]                 │
│                                                                │
│  Preview: «echo "..." | fabric -p analyze_claims»              │
│                                                                │
│  [Cancel]                                          [Execute ↵] │
└────────────────────────────────────────────────────────────────┘
```

- Each panel is fuzzy-filterable; type to narrow, arrows to navigate.
- Tab moves focus between panels and to the adverb tray.
- Type contract shown below the panels — visual breadcrumb of what the verbs accept.
- Adverbs auto-populate from the selected verb. Selector adverbs are dropdowns; fill adverbs are text inputs.
- Preview shows the actual command that will be executed.
- Execute runs it; Cancel closes the dialog.

**Snappiness target:** sub-100ms wake time. Two implementation paths:

1. **Cold start** — `goo-compose` is a fresh process each invocation. Acceptable if libcosmic+iced startup stays under ~150ms.
2. **Daemonized** — `goo-composed` runs in the background, listens on a socket, shows the window on signal. Sub-30ms wake. Probably the eventual target.

State handoff via `/run/user/$UID/cosmic-goo/compose-state.json` (temp file). Daemonized mode can use a socket message instead.

---

## CLI surface

```
goo                                              # opens compose dialog (empty)
goo <verb> [subject] [object] [--adverb=value]   # execute a sentence
goo list <source>                                # raw JSON of source items
goo describe <verb>                              # show verb's accepted types, adverbs, template
goo compose [partial-sentence]                   # explicitly launch compose dialog
goo plugins                                      # list loaded plugins
goo validate                                     # check all plugins for errors
```

If the subject is omitted and the verb accepts an implicit subject, cosmic-goo falls back in order: PRIMARY selection → clipboard → focused app.

---

## Repository layout

```
cosmic-goo/                    # standalone repo
├── plugins/                   # built-in plugins (one TOML each, plus optional dirs)
│   ├── core.toml              # base types, base verbs
│   ├── selection.toml         # selection source + clipboard source
│   ├── apps.toml              # cos-cli wrapper, app source, app verbs
│   ├── workspaces.toml        # workspace source + verbs
│   ├── tmux.toml              # tmux-use wrapper
│   ├── files.toml             # ffs wrapper
│   ├── scenes/                # rich plugin: scenes source + verbs + scene-specific scripts
│   │   ├── plugin.toml
│   │   └── bin/
│   ├── claude-routing.toml    # the `via` adverb (fabric / desktop / code / clipboard)
│   ├── text-verbs.toml        # text/* verbs (critique, summarize, think, reply, ...)
│   └── fabric.toml            # exposes fabric patterns as verbs and as a source
├── bin/
│   ├── goo                    # main CLI
│   └── goo-compose            # compose dialog binary (built from src/)
├── src/                       # Rust source for goo-compose
├── lib/                       # shared shell utilities (used by goo CLI and plugin scripts)
│   ├── plugin-loader.sh
│   ├── types.sh               # MIME detection, glob matching
│   ├── verbs.sh
│   ├── adverbs.sh
│   ├── selection.sh
│   └── url-encode.sh
├── pop-launcher/
│   └── goo-meta/              # the meta-plugin for pop-launcher
│       ├── plugin.ron
│       └── goo-meta           # executable (shell or Rust)
├── scenes/                    # user scene definitions live here (or override in ~/.config)
├── doc/
│   ├── architecture.md        # this spec
│   ├── plugin-authoring.md
│   ├── cli-reference.md
│   └── examples/
│       ├── ms-natural-4000-bindings.md   # keyboard layout example
│       ├── stream-deck-bindings.md       # other example
│       └── sxhkd-bindings.md             # another
├── recon/
│   ├── env.sh
│   └── keys.sh
└── README.md
```

cosmic-goo does **not** ship a keymap or generate COSMIC shortcut config. Examples live in `doc/examples/` as reference layouts users can adapt.

---

## Implementation phases

### Phase 0 — recon (in progress)
Run `recon/env.sh` and `recon/keys.sh`. Confirm: `claude://` handler resolves, pop-launcher is scriptable, `wl-paste --primary` works, `cos-cli` builds.

### Phase 1 — CLI + first plugins (~1 week)
- `bin/goo` as a shell entry point
- Plugin loader (`lib/plugin-loader.sh`)
- Type system + MIME glob matcher (`lib/types.sh`)
- Verb dispatch (`lib/verbs.sh`)
- Three plugins minimum: `selection.toml`, `apps.toml`, `claude-routing.toml`
- Three verbs minimum: `critique`, `activate`, `draft-response`
- Validates: CLI works, types resolve, all three routes (fabric, claude-desktop, claude-code) fire correctly

### Phase 2 — pop-launcher integration
- `pop-launcher/goo-meta/` meta-plugin
- Inline composition with autocomplete (subjects + verbs + simple adverbs)
- No promote-to-dialog yet (button shows placeholder)

### Phase 3 — scenes plugin
- The first "rich" plugin (dir with `plugin.toml` + helper scripts)
- Implements anchor scenes (browser, mail, claude-desktop) + favorite scenes (1–5) + scene definitions
- Adds the `scene` MIME type and scene-specific verbs

### Phase 4 — compose dialog
- `goo-compose` as a libcosmic/iced binary
- State handoff via temp file (daemonization deferred)
- Promote button in launcher meta-plugin starts working

### Phase 5 — broadening
- tmux, files, workspaces, clipboard-history plugins
- `goo-composed` daemon for sub-30ms wake
- content-dispatch graduates from heuristics to sitting_duck integration

### Phase 6 — open-source polish
- README, contributing guide, plugin authoring doc
- Example bindings for several keyboards
- Submit `tmux-use --json` and `ffs --json` flags upstream

---

## Open questions

- **Meta-plugin language**: bash for v1, Rust likely for v2 once protocol stabilizes. Avoid premature Rust.
- **`wl-paste --primary` reliability under Cosmic**: needs recon. If flaky, build a selection-caching daemon (separate from the compose daemon, or merged).
- **Adverbs that come from prompts during verb execution**: e.g., `send-to-chat` needs a chat picker. Currently modeled as a fill adverb with a chooser fallback. Verify the UX works in practice; may want a dedicated "chooser" adverb kind.
- **Plugin signing / sandboxing**: long-deferred. Plugins run shell commands and have full user privilege. Acceptable for v1; revisit when sharing externally.
- **Compose dialog tab navigation**: does Tab move between panels, or within a panel? My guess: between. Within-panel is arrow keys + filter typing.
- **Type inference cost**: running `file --mime-type` on every selection might add latency. May need a small cache or only-on-demand detection.

---

## Reconnaissance status

| Item | Status |
|------|--------|
| Cosmic D-Bus interfaces | known; less complete than Hyprland |
| Workspace/window API | covered by `cos-cli` (third-party, install via cargo) |
| pop-launcher protocol | documented, plugin model understood |
| `claude://` URL scheme | documented for macOS/Windows; Linux verification pending |
| `wl-paste --primary` | Cosmic-specific reliability check pending |
| MS NE 4000 keysyms | `recon/keys.sh` ready to capture |
| Cosmic dynamic workspaces | confirmed; address by app_id not slot |
| `COSMIC_DATA_CONTROL_ENABLED` | required for clipboard managers; trade-off documented |

---

## Glossary

- **GOO** — Grammar Of Operations.
- **Plugin** — a TOML file (or directory) declaring any combination of types, sources, verbs, adverbs, and helper scripts. Unit of distribution and override.
- **Type** — a MIME type, either standard (`text/markdown`, `inode/directory`) or vendor (`application/vnd.tmux-use.session`). Verbs target types; sources emit types. One type kingdom, no separate handle space.
- **Source** — a place that emits typed items (apps, scenes, files, clipboard, selection). Declared in a plugin with a `list_cmd`.
- **Verb** — a named action with an `accepts` type list and a template (or adverb-selected templates). Verbs are not source-scoped; they apply to any source emitting compatible types.
- **Adverb** — a modifier on a verb. Selector adverbs (`--via=fabric|claude-desktop|...`) pick from a known set; fill adverbs (`--name=<string>`) take free values. Replaces "routes" and "modifier conventions" from earlier designs.
- **Subject** — the first argument to a verb. In the launcher, the noun the user picks. In hotkey/CLI mode, the implicit selection/clipboard.
- **Object** — the second argument to a two-step verb (workspace for `move-to-workspace`, chat for `send-to-chat`).
- **Scene** — a named work context (workspace + apps + tmux session + cwd). Scenes are typed `application/vnd.cosmic-goo.scene`. One source/verb set among many.
- **Anchor** — a singleton scene expected to persist throughout a session (browser, mail, claude-desktop). Focus-or-spawn semantics.
- **Compose dialog** — the optional three-panel GUI, invoked on demand via the launcher's `🌌` promotion or `goo compose` CLI.
- **Promote** — the act of transferring a partial sentence from the launcher's inline composition into the compose dialog.
