# cosmic-goo Implementation Plan

A concrete, task-level breakdown of how to build cosmic-goo from empty repo to functional. Each task has a deliverable, file paths, acceptance criteria, and dependencies. Designed for incremental, demonstrable progress.

---

## Pre-flight: recon (~1 hour, blocking)

Before any code, validate the environment assumptions.

### R1: Run env recon

- Run `recon/env.sh` on longbottom (already drafted)
- Capture output to `recon-results.log`
- **Verify**: Cosmic D-Bus interfaces, COSMIC env vars, wl-paste behavior, claude:// handler registration

### R2: Run key recon

- Run `recon/keys.sh` interactively, capturing keysyms
- Save `keysyms.log`
- **Verify**: XF86Help, XF86Search, XF86Mail, etc. all register; no surprising collisions

### R3: Install cos-cli

- `cargo install --git https://github.com/estin/cos-cli`
- Run `cos-cli info --json | jq .` — confirm output shape

### R4: Smoke-test claude:// URL handler

- Run `xdg-open "claude://claude.ai/new?q=Hello%20from%20recon"` manually
- **Verify**: Claude Desktop opens with prompt prefilled

### R5: Smoke-test wl-paste --primary in Cosmic

- Select text in different apps (terminal, browser, editor)
- Run `wl-paste --primary` from another terminal
- **Verify**: Selection text returned reliably (or document where it fails)

**Gate to Phase 1**: All five recon items must pass or be documented as blockers requiring workarounds.

---

## Phase 1: CLI + first plugins (target: ~1 week of focused work)

Goal: Working `goo` CLI that can execute three verbs through three routes, validating the entire architecture end-to-end.

### T1.1: Project scaffolding

**Files created:**
```
cosmic-goo/
├── .gitignore
├── LICENSE
├── README.md (copy from drafts)
├── CONTRIBUTING.md (copy from drafts)
├── Makefile or justfile
├── bin/
├── lib/
├── plugins/
├── doc/
│   ├── architecture.md (copy from spec)
│   └── implementation-plan.md (this file)
├── recon/
│   ├── env.sh
│   └── keys.sh
└── tests/
```

**Dependencies**: none
**Acceptance**: `git init` clean, `make help` lists available commands

---

### T1.2: TOML parsing strategy

**Decision**: Use `yq` (mikefarah/yq, Go version) which natively reads TOML and outputs JSON. Document as a runtime dependency.

**Files created:**
- `lib/toml.sh` — small wrapper that calls `yq -p toml -o json` and pipes to `jq`

**Function provided:**
```bash
toml_get FILE QUERY  # echoes JSON result
toml_keys FILE QUERY # echoes top-level keys
```

**Dependencies**: T1.1
**Acceptance**: `bash -c '. lib/toml.sh; toml_get plugins/tmux.toml ".name"'` returns `"tmux"`

---

### T1.3: Plugin loader

**Files created:**
- `lib/plugin-loader.sh`

**Functions provided:**
```bash
plugin_discover                # finds all plugin.toml files in standard dirs
plugin_load FILE               # parses one plugin, registers contents
plugin_load_all                # loads all discovered plugins
plugin_registry_export         # dumps registered types/sources/verbs/adverbs as JSON
```

**Internal state**: a JSON file at `$XDG_RUNTIME_DIR/cosmic-goo/registry.json` updated on each load. Cached; only regenerated when plugin files change (mtime check).

**Dependencies**: T1.2
**Acceptance**:
- `goo plugins` lists at least the three Phase 1 plugins
- Loading is <100ms cold, <10ms cached
- No plugin file's contents are silently dropped

---

### T1.4: Type system + MIME glob matching

**Files created:**
- `lib/types.sh`

**Functions provided:**
```bash
mime_matches PATTERN MIME      # true if MIME matches glob pattern (e.g., "text/*" matches "text/plain")
mime_detect_content STRING     # returns MIME type for arbitrary text (libmagic + heuristics)
mime_detect_path PATH          # returns MIME type for a file path
```

**Algorithm for `mime_matches`**:
- Exact match: `text/plain` == `text/plain` ✓
- Suffix wildcard: `text/*` matches `text/anything` ✓
- Prefix wildcard: `*/json` matches `application/json` ✓
- Vendor: `application/vnd.tmux-use.*` matches `application/vnd.tmux-use.session` ✓

**Heuristics for `mime_detect_content`** (run in order, first match wins):
1. URL regex → `text/x-uri`
2. Looks like a file path → check with `file --mime-type`
3. libmagic via `file --mime-type -`
4. Default → `text/plain`

**Dependencies**: T1.1
**Acceptance**:
- Unit tests in `tests/types.bats` cover the matching cases
- `mime_detect_content "https://example.com"` returns `text/x-uri`
- `mime_matches "text/*" "text/markdown"` returns true

---

### T1.5: Selection capture

**Files created:**
- `lib/selection.sh`

**Functions provided:**
```bash
selection_primary          # text from PRIMARY selection
selection_clipboard        # text from CLIPBOARD
selection_clipboard_mimes  # list of MIME types currently on clipboard
selection_clipboard_as MIME # content of clipboard for a specific MIME
focused_app                # JSON of currently focused window (via cos-cli)
```

**Implementation notes:**
- Wraps `wl-paste`, `wl-paste --primary`, `wl-paste --list-types`
- Falls back gracefully if `wl-paste` unavailable
- `focused_app` shells to `cos-cli info --json | jq '.apps[] | select(.state[]? == "activated")'`

**Dependencies**: T1.1, R5 (recon — selection reliability)
**Acceptance**:
- `selection_primary` returns current selection text or empty
- `selection_clipboard_mimes` returns at least `text/plain` when text is copied

---

### T1.6: URL encoding

**Files created:**
- `lib/url-encode.sh`

**Functions provided:**
```bash
url_encode STRING          # echoes URL-encoded version
```

**Implementation**: use `jq -srR @uri` for safety; or a pure-bash implementation as fallback.

**Dependencies**: T1.1
**Acceptance**:
- `url_encode "Hello, world!"` returns `Hello%2C%20world%21`
- Unicode handled correctly

---

### T1.7: Verb dispatch

**Files created:**
- `lib/verbs.sh`

**Functions provided:**
```bash
verb_lookup NAME [TYPE]              # find verb by name; optionally filter by acceptance of TYPE
verb_default_for TYPE                # find the default verb for a given type
verb_for_subject SUBJECT_JSON        # list applicable verbs for a typed subject
verb_apply VERB SUBJECT [OBJECT] [ADVERBS...]   # build and execute the command
```

**`verb_apply` algorithm:**
1. Resolve the verb from the registry
2. Validate `accepts` matches subject type
3. If verb has `object_type`, validate object type
4. For each declared adverb, resolve its value (CLI override → default)
5. Apply adverb `template_var` injections to the verb template
6. If adverb selects a sub-template (selector adverb on the verb's outer template), use that
7. Substitute `{subject.*}`, `{object.*}`, `{verb.*}`, `{adverbs.*}`, `{cwd}` in the final template
8. If `confirm = true`, prompt yes/no
9. Execute via `eval` or `bash -c` (with appropriate quoting safety)

**Dependencies**: T1.3, T1.4, T1.5
**Acceptance**:
- `verb_lookup critique` returns the critique verb's definition
- `verb_apply critique "text content" --via=clipboard` puts the rendered prompt on the clipboard
- All template variables substitute correctly

---

### T1.8: Main CLI entry point

**Files created:**
- `bin/goo`

**Supports:**
```
goo                                # opens compose dialog (stub for now)
goo <verb> [subject] [object] [--adverb=value]
goo list <source>
goo describe <verb>
goo compose [partial-sentence]     # stub
goo plugins
goo validate
```

**Argument parser**: simple manual parsing in bash (getopts won't handle `--key=value` cleanly with our needs). Subcommand-style.

**Implicit subject resolution**: if no subject argument given, fall back in order:
1. PRIMARY selection (if non-empty)
2. Clipboard text (if non-empty)
3. Focused app (for verbs accepting app types)

**Dependencies**: T1.3 through T1.7
**Acceptance**:
- `goo plugins` lists loaded plugins
- `goo validate` returns 0 with no errors on built-in plugins
- `goo critique "test text"` executes the critique verb via the default route
- `goo describe critique` shows the verb's accepts, adverbs, and template

---

### T1.9: Phase 1 plugin set

**Files created:**

#### `plugins/selection.toml`
```toml
name = "selection"

[[sources]]
name = "selection"
prefix = "sel"
icon = "edit-select-all"
emits = "text/*"
list_cmd = "wl-paste --primary | head -c 200"  # for display in launcher
implicit = true  # this is the default subject when none given
```

#### `plugins/apps.toml`
```toml
name = "apps"

[[types]]
name = "application/vnd.cos-cli.app"
display = "running app"
kind = "handle"

[[sources]]
name = "apps"
prefix = "app"
icon = "preferences-system-windows"
emits = "application/vnd.cos-cli.app"
list_cmd = "cos-cli info --json | jq '[.apps[] | {id: .app_id, title: .title, subtitle: .app_id, metadata: .}]'"

[[verbs]]
name = "activate"
accepts = ["application/vnd.cos-cli.app"]
default_for = "application/vnd.cos-cli.app"
cmd = "cos-cli activate -i {subject.metadata.index}"
```

#### `plugins/claude-routing.toml`
Per spec section "Example: an adverb-only plugin"

#### `plugins/text-verbs.toml`
Per spec section "Example: a text-verbs plugin", including critique, summarize, think, and:

```toml
[[verbs]]
name = "draft-response"
accepts = ["text/*"]
uses_adverbs = ["via"]
default_route = "claude-desktop"  # this verb prefers the desktop UI
fabric_pattern = "create_summary"  # used if --via=fabric overridden
prompt = """
Consider the following selected text and begin drafting a response.
Identify key answers or information needed, and questions we should
ask in the response. Raise them to the user to get their options.

---
{subject.text}
"""
```

**Dependencies**: T1.3 (plugin loader must load these)
**Acceptance**:
- `goo critique "Sample text to analyze"` runs through Fabric
- `goo critique "Sample text" --via=claude-desktop` opens Claude Desktop URL
- `goo critique "Sample text" --via=claude-code` opens Claude Code URL
- `goo critique "Sample text" --via=clipboard` puts the assembled prompt on clipboard
- `goo activate firefox` focuses Firefox if running

---

### T1.10: Test suite

**Files created:**
- `tests/types.bats` — type matching and inference
- `tests/plugin-loader.bats` — loading, registry, validation
- `tests/verbs.bats` — verb lookup and template substitution
- `tests/integration/critique.bats` — full end-to-end with clipboard route (so no actual API call)

**Tool**: bats-core (https://github.com/bats-core/bats-core)

**Dependencies**: Each test file depends on the corresponding T1.X task
**Acceptance**: `make test` runs clean

---

### T1.11: Documentation pass

**Files created:**
- `doc/cli-reference.md`
- `doc/plugin-authoring.md` (extracted/expanded from CONTRIBUTING.md)
- Example bindings: `doc/examples/ms-natural-4000-bindings.md` (just descriptive — no generator)

**Dependencies**: Phase 1 substantially complete
**Acceptance**: A first-time user can read README → CONTRIBUTING → write a simple plugin without asking questions

---

### Phase 1 exit criteria

✓ `goo` CLI works on longbottom
✓ Three verbs (`critique`, `activate`, `draft-response`) execute correctly
✓ All four routes (`fabric`, `claude-desktop`, `claude-code`, `clipboard`) work
✓ Implicit subject (selection) resolves
✓ `goo validate` is clean
✓ Tests pass
✓ Can be bound to a key via COSMIC settings and behave correctly

**Open question to resolve in this phase**: where does `goo` install to? `/usr/local/bin` or `~/.local/bin`? Tooling: should ship a `make install` and `make install-user`.

---

## Phase 2: pop-launcher integration (~3-5 days)

### T2.1: meta-plugin scaffolding

**Files created:**
- `pop-launcher/goo-meta/plugin.ron` — pop-launcher manifest
- `pop-launcher/goo-meta/goo-meta` — shell script (later Rust)

**Manifest declares:**
- Plugin name: `goo-meta`
- Query: persistent (always invoked, even with empty query)
- Icon, description

**Dependencies**: Phase 1
**Acceptance**: pop-launcher loads it on restart, `cosmic-launcher` shows results from it

---

### T2.2: pop-launcher protocol implementation

**Implements (in `goo-meta`):**
- Read JSON requests from stdin: `Search`, `Activate`, `Context`, `ActivateContext`
- Write JSON responses to stdout: `Append`, `Context`, `Finished`, `Close`, `Fill`

**For each `Search` request:**
1. Parse the query string into tokens (subject / verb / object / adverbs)
2. Determine which stage the cursor is in
3. Query the appropriate completion pool
4. Emit `Append` for each candidate

**Dependencies**: T2.1
**Acceptance**:
- Typing `firefox` in cosmic-launcher shows Firefox app as a result
- Picking it autocompletes with `Fill("firefox ")` and shows verbs
- Selecting `activate` and pressing Enter focuses Firefox

---

### T2.3: Type-aware autocomplete

**Adds:**
- Per-stage completion pools (per the spec's autocomplete table)
- Sigil rendering (`/.../`, `:source`, `*verb*`, `--adverb=value`)
- Down-arrow value cycling for selector adverbs
- The worked example flow: `sel<TAB>cr<TAB>s<DOWN><TAB>v<TAB>f<TAB>`

**Dependencies**: T2.2
**Acceptance**:
- Each TAB press in the worked example produces the expected `Fill` update
- All verb completions filter correctly by type

---

### T2.4: Promote-to-compose stub

**Adds:**
- A synthetic launcher result labeled (placeholder sigil; we'll pick the real one in Phase 4) "Open in compose dialog" that's always present when input has structure
- On activation, writes state to temp file and shows a toast "compose dialog coming in Phase 4"

**Dependencies**: T2.3
**Acceptance**: The promote item appears; activating it writes the JSON state file; user sees the placeholder message

---

### Phase 2 exit criteria

✓ Meta-plugin registered with pop-launcher
✓ Inline composition works in cosmic-launcher
✓ Type contracts visible and respected
✓ Promote stub fires correctly (full implementation in Phase 4)

---

## Phase 3: scenes plugin (~3-5 days)

The first "rich" plugin — directory with `plugin.toml` plus helper scripts.

### T3.1: Scene definition format

**Files created:**
- `plugins/scenes/plugin.toml`
- `plugins/scenes/bin/scene-list.sh`
- `plugins/scenes/bin/scene-open.sh`
- `plugins/scenes/bin/scene-create.sh`
- Example scenes in `scenes/`

**Scene format** (`scenes/<name>.toml`):
```toml
name = "dotfiles"
display = "Dotfiles project"
singleton = true
cwd = "~/.dotfiles"

[[apps]]
type = "alacritty"
class = "Alacritty"

[tmux]
session = "dotfiles"   # resolved via tmux-use
```

**Dependencies**: Phase 1
**Acceptance**: `goo list scenes` lists all scene TOMLs

---

### T3.2: Anchor scenes implementation

Three built-in scenes:
- `browser` (Firefox or similar)
- `mail` (mail/calendar app)
- `claude-desktop` (Claude Desktop)

Each is a singleton scene with focus-or-spawn logic via cos-cli.

**Files created:**
- `scenes/browser.toml`, `scenes/mail.toml`, `scenes/claude-desktop.toml`
- `plugins/scenes/bin/scene-anchor.sh` — implements focus-or-spawn for any singleton scene

**Dependencies**: T3.1, cos-cli integration from Phase 1
**Acceptance**:
- `goo open browser` focuses the browser if running, spawns it otherwise
- Works after a fresh boot, after browser is moved between workspaces

---

### T3.3: Favorite scenes

Five user-assignable favorite slots, surfaced as `application/vnd.cosmic-goo.favorite-slot` typed objects with an `assign-favorite` verb.

**Files created:**
- `plugins/scenes/bin/scene-favorite.sh` — assign/show/clear favorite slots
- Storage at `~/.config/cosmic-goo/favorites.toml`

**Dependencies**: T3.1, T3.2
**Acceptance**:
- `goo assign-favorite dotfiles 1` sets slot 1
- `goo open favorite-1` opens the scene assigned to slot 1
- Default-empty favorites show as "(unassigned)"

---

### T3.4: Scene creation wizard

Interactive flow for capturing the current workspace state as a new scene.

**Files created:**
- `plugins/scenes/bin/scene-capture.sh`

**Logic:**
1. Query current workspace via cos-cli
2. Enumerate apps on that workspace
3. Detect active tmux session in any Alacritty window (via tmux-use or shell heuristics)
4. Prompt for scene name
5. Write `scenes/<name>.toml`

**Dependencies**: T3.3
**Acceptance**: Capturing a workspace produces a valid scene that can be reopened

---

### Phase 3 exit criteria

✓ Scenes plugin loads as a directory plugin
✓ Anchor scenes (browser/mail/claude-desktop) work end-to-end
✓ Favorite slots can be assigned and opened
✓ Capture wizard creates valid scene TOMLs
✓ At least one of these works via the launcher inline composition

---

## Phase 4: compose dialog (~1-2 weeks)

The first Rust component.

### T4.1: libcosmic/iced scaffolding

**Files created:**
- `src/main.rs`
- `Cargo.toml`
- `src/ui/` (modules)

**Dependencies**: Rust toolchain
**Acceptance**: `cargo build --release` produces `target/release/goo-compose`

---

### T4.2: State loading

**Reads:**
- `/run/user/$UID/cosmic-goo/compose-state.json` on startup
- Parses to determine which panels are pre-filled

**Schema for state JSON**:
```json
{
  "subject": {"id": "firefox", "type": "application/vnd.cos-cli.app", "title": "Firefox"},
  "verb": "move-to-workspace",
  "object": null,
  "adverbs": {}
}
```

**Dependencies**: T4.1
**Acceptance**: Launching `goo-compose` with a state file pre-fills panels

---

### T4.3: Three-panel UI

**Implements:**
- Subject panel: fuzzy-filterable list of sources/items
- Verb panel: filtered by subject type
- Object panel: filtered by verb's object_type (or hidden if N/A)
- Type contract breadcrumb below
- Adverb tray at bottom (dropdowns for selectors, text inputs for fills)
- Preview line showing the rendered command
- Cancel / Execute buttons

**Dependencies**: T4.2
**Acceptance**: Full keyboard navigation; mouse works; visual breadcrumb updates live

---

### T4.4: Execute via CLI

The dialog doesn't reimplement verb execution; it shells to `goo <verb> <subject> ...` with the assembled args.

**Dependencies**: T4.3, Phase 1 CLI
**Acceptance**: Executing from the dialog matches CLI invocation behavior exactly

---

### T4.5: Launcher integration

Update T2.4's stub to actually launch `goo-compose`.

**Dependencies**: T4.4
**Acceptance**: Promote button in launcher opens dialog with state intact

---

### Phase 4 exit criteria

✓ `goo-compose` binary builds and runs
✓ Sub-150ms cold-start launch time
✓ All compositions reachable from inline are also reachable from dialog, and vice versa
✓ Dialog can be invoked from launcher promote, from `goo compose`, or with no args (empty start)

---

## Phase 5: broadening (ongoing)

Once Phase 4 is shippable, the rest is breadth, not depth:

- T5.1: `tmux.toml` plugin — assumes `tmux-use --json` lands upstream (submit PR)
- T5.2: `files.toml` plugin — uses `ffs --json` (submit PR) or `fd --json` fallback
- T5.3: `workspaces.toml` plugin — workspace verbs (switch, rename)
- T5.4: `clipboard.toml` plugin — `cliphist` integration, text verbs on history items
- T5.5: `fabric.toml` plugin — exposes patterns as verbs in the registry
- T5.6: `goo-composed` daemon for sub-30ms compose dialog wake
- T5.7: Selection-caching daemon if Phase 1 R5 results show flakiness
- T5.8: content-dispatch graduates from heuristics to sitting_duck integration

These can be parallelized across contributors once Phase 4 is stable.

---

## Phase 6: open-source polish (when ready)

- T6.1: Pin a v0.1.0 tag
- T6.2: Polish README, add screenshots/GIFs
- T6.3: Pre-built packages (deb, rpm, AUR? Flatpak?)
- T6.4: Submit to pop-launcher's plugin directory if such a thing exists
- T6.5: Blog post / announcement
- T6.6: Bindings examples for several keyboards

---

## Effort estimate summary

| Phase | Calendar time | Focused engineering hours |
|-------|---------------|---------------------------|
| Recon | 1-2 hours | 1-2 hours |
| Phase 1 | 1 week | ~15-20 hours |
| Phase 2 | 3-5 days | ~10-15 hours |
| Phase 3 | 3-5 days | ~10-15 hours |
| Phase 4 | 1-2 weeks | ~25-40 hours |
| Phase 5 | Ongoing | varies |
| Phase 6 | 1 week | ~5-10 hours |

**To "shippable v0.1"**: roughly 60-100 focused hours, spread over 4-6 weeks of evenings.

Note these are honest estimates assuming concentration, not hero-mode. ADHD-adjacent scattered work will probably stretch the calendar; the engineering hours are still approximately right.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| `wl-paste --primary` flaky in Cosmic | Build selection-caching daemon early if R5 confirms |
| pop-launcher plugin protocol changes | Pin to a known version; track upstream |
| libcosmic/iced ergonomics painful | Start dialog in Phase 4, not Phase 1; can swap UI framework if needed |
| Spec churn during implementation | Decisions documented in spec; spec edits require an issue first once Phase 1 ships |
| `cos-cli` unmaintained | Vendor as submodule; consider rolling own in Phase 5 if needed |
| `claude://` URL handler limit (14k chars) | Document in CLI; gracefully fall back to clipboard-only route for long selections |

---

## What I'd build first if I had one evening

1. T1.1 — scaffolding (30 min)
2. T1.2 — toml.sh wrapper (15 min, just three functions)
3. T1.4 — types.sh with the MIME matcher and unit tests (60 min)
4. Write a single hand-crafted `plugins/text-verbs.toml` with just the `critique` verb hardcoded to clipboard route
5. Write `bin/goo` as a script that does exactly one thing: read the verb from argv, get text from selection or argv, format the prompt template, send to clipboard

That's about 2.5 hours and gives you something working you can bind to a key. The rest of Phase 1 is making it general and clean.
