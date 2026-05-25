# Prior art & architecture direction

Design notes from the 2026-05-24 research pass. Not user docs — rationale for
where the project is headed. Two parts: what to steal from Kupfer/plumber, and
the core/integration + standalone + daemon architecture.

## Prior art: what to borrow

### Kupfer ([plugin API](https://kupferlauncher.github.io/Documentation/PluginAPI.html))

Maintained Python/GTK launcher; its object model is *our* model. Mapping:

| Kupfer | cosmic-goo |
|---|---|
| `Leaf` (object, `self.object`) | subject (JSON) |
| `Action` (verb), `activate(obj)` / `activate(obj, iobj)` | verb / two-step verb |
| `Source` (`get_items`, `provides`) | source (`list_cmd`, `emits`) |
| direct + **indirect object** | subject + object |

Ideas worth adopting (most relevant at Phase 2 / launcher):

1. **`valid_for_item(item)`** — a per-item predicate on top of `item_types()`.
   We only have `accepts` (type glob); we can't say "this verb applies to *some*
   items of this type." e.g. `extract` should offer only for `.zip`/`.tar`, not
   all files; `git-pull` only for repos with a remote. → add an optional
   `valid_when = "<shell test>"` to verbs (run against the resolved subject;
   verb hidden if it exits non-zero).
2. **`object_source(for_item)`** — a verb names the source for its indirect
   object, and that source can depend on the subject. We resolve objects against
   *any* source emitting `object_type`; an explicit per-verb `object_source`
   (and subject-dependent objects, e.g. "move window to a workspace *on the same
   output*") is a refinement.
3. **`rank_adjust` / `get_rank`** — ranking hints. We have none. The launcher
   (Phase 2) needs ranking; steal the "small integer nudge" approach.
4. **`has_content()` / `content_source()`** — a leaf can yield a child source
   (a folder → its files). This is our deferred `emits`/coercion idea
   (`url → fetch → html → summarize`). Kupfer shows the shape.
5. **`wants_context` / `is_async` / late results** — async actions that don't
   block the UI and post results/errors later. Directly relevant to the **daemon**
   (below) and to slow verbs (fabric API). Our verbs are synchronous bash today.

Terminology to adopt for consistency: **direct/indirect object** (vs our
"subject/object"), and a PowerShell-style **approved-verb vocabulary** so plugin
verbs don't drift (`git-status`/`service-status`/`now-playing` should share a
naming convention).

### Plan 9 plumber ([plumber(4)](https://9fans.github.io/plan9port/man/man4/plumber.html), [the paper](https://doc.cat-v.org/plan_9/4th_edition/papers/plumb))

A rule engine that classifies a datum by content/type and dispatches it. Rules
are `object verb argument` lines. Real examples from plan9port's `basic`:

```
# URL → web port
data matches '(https?|ftp|file|gopher|mailto)://[a-zA-Z0-9_@\-]+'
plumb to web
plumb start web $0

# file:line → editor, line as an attribute
data matches '([.a-zA-Z0-9_/\-@]*[a-zA-Z0-9_/\-])('$addr')?'
arg isfile      $1
data set        $file
attr add        addr=$3
plumb to edit
plumb client $editor

# man-page reference
data matches '([a-zA-Z0-9_\-./]+)\(([1-8])\)'
plumb start rc -c 'man '$2' '$1
```

Objects: `src dst type attr data wdir`. Verbs: `is isfile isdir matches set add
delete to start client`. Captures `$0/$1…`, attribute vars `$file/$addr`,
`${cmd}` expansion.

**This is a declarative version of our `mime_detect_content` + `address.sh`,
which we wrote imperatively in bash.** The deferred "content-dispatch" layer
(the spec's sitting_duck/heuristics) should be a **plumber-style rule table in
TOML** — classify raw text → type + extracted attrs → default verb:

```toml
[[dispatch]]                                   # a goo "plumbing rule"
matches = 'RFC:?\s*([0-9]+)'
type    = "text/x-uri"
set     = { url = "https://www.rfc-editor.org/rfc/rfc${1}.txt" }
verb    = "open-url"

[[dispatch]]
matches = '([\w./\-]+):([0-9]+)'   # file:line
isfile  = "${1}"
type    = "inode/file"
attr    = { line = "${2}" }
verb    = "open"
```

That's plumber's model in goo's idiom — the right shape for content-dispatch
when we build it. (We already have the verbs and addressing it would route to.)

## Architecture: `goo` core vs integrations

**Realization: the core is already COSMIC-agnostic.** `lib/{toml,types,url-encode,
plugin-loader,verbs,address,dialog}.sh` and `bin/goo` have zero COSMIC deps. The
only leaks: `lib/selection.sh`'s `focused_app` (cos-cli, already guarded) and the
cos-cli/pop-launcher *plugins*. So "make goo a library with cosmic as integration"
is largely a **naming + packaging** move, not a rewrite.

Plugins sort cleanly into dependency tiers:

| Tier | Depends on | Plugins |
|---|---|---|
| **core** | bash + jq + coreutils only | text-utilities, calculator, urls, sigils, claude-routing, text-verbs |
| **linux-desktop** | freedesktop / Wayland / PipeWire (any compositor) | selection, notifications, media (playerctl), audio (wpctl), screenshots (grim/slurp), clipboard-history (cliphist), network (nmcli), bluetooth, services (systemd) |
| **cosmic** | cos-cli / pop-launcher | apps, workspaces, (future) scenes, pop-launcher meta-plugin |

So:

- **`goo`** = the engine + core + linux-desktop tiers. Fully portable; the
  "standalone." Binary is `goo`.
- **`cosmic-goo`** = `goo` + the cosmic tier + COSMIC keybinding examples + the
  launcher meta-plugin. This repo, as the COSMIC-flavored distribution.
- **`goo-standalone` / `zenity-goo` are NOT separate codebases** — they're
  **install profiles / plugin tiers** of the same repo, plus a picker backend.

Concrete steps (cheap, mostly non-code):
- Document the tiers (above) and add install profiles: `make install-core`,
  `make install` (core+desktop), `make install-cosmic`.
- Move `focused_app` out of core `selection.sh` into the `apps` plugin (or behind
  a "window-manager capability" the core can call if present), so core has *zero*
  cos-cli reference.
- Add **`zenity` as a `dialog_pick` backend** (lib/dialog.sh already abstracts
  fuzzel/rofi/wofi/fzf) → the compose dialog works on any GTK system with no
  wlroots picker. That *is* "zenity-goo."
- Eventual: split into two repos (`goo` + `cosmic-goo`) only if/when there's an
  external consumer of core. Premature now; tiers-in-one-repo suffices.

## Execution model: one-shot (always) + optional daemon

**Hard requirement: one-shot must always work with no daemon** — scripts, SSH,
CI, and keybindings that spawn a fresh process all depend on it. That's the
current design and stays the baseline. The registry mtime cache already makes
warm one-shot loads ~10ms.

**The daemon is an optional optimization layer the client uses if present:**

- `goo` (client): try to connect to `$XDG_RUNTIME_DIR/cosmic-goo/goo.sock`. If
  up, send `{argv, cwd, env-subset, stdin}` and stream back `{stdout, stderr,
  exit}`. If not, fall through to the in-process one-shot path. Transparent.
- `good` (daemon): holds the parsed registry in memory, watches plugin dirs
  (inotify/mtime) to reload, serves requests, and can run **async** verbs
  (Kupfer's late-results model) so a fabric API call doesn't block. Verb
  execution still shells `bash -c` the rendered command — the daemon just keeps
  the registry + lib warm.

**Where the daemon actually pays off** (and thus when to build it):
1. **Launcher meta-plugin (Phase 2)** — per-keystroke completion latency; a cold
   one-shot per keystroke is unacceptable, a warm daemon is instant.
2. **Compose dialog wake** — the spec's sub-30ms `goo-composed` target.
3. **Stateful features** — selection cache (if `wl-paste --primary` proves
   flaky), clipboard/launch history, async verb results.

For pure CLI use the cache already suffices, so the daemon's marginal win there
is small (saves bash-source + a few jq spawns, tens of ms). **Build the daemon
alongside the interactive frontends (Phase 2/4), where it earns its keep.**

**Implementation language:** bash can't host a socket server cleanly. Options:
(a) `socat`/`systemd` socket-activation in front of a persistent bash worker via
FIFO — hacky but pure-shell; (b) reimplement the hot path (registry load +
resolve + dispatch) in a small **Rust** `good`, which dovetails with the spec's
Rust `goo-compose`/`goo-composed`. The Rust daemon is the long-term answer; a
socat/FIFO spike could validate the client/daemon protocol first without the
Rust commitment. The protocol (newline-delimited JSON request/response over a
UNIX socket, carrying the canonical `cosmic-goo:` URIs) is the part worth
designing carefully — both the bash spike and the eventual Rust daemon speak it.

## Suggested task breakdown

- **zenity picker backend** (small, real) — `lib/dialog.sh`. Unblocks GTK-only systems.
- **Plugin dependency tiers + install profiles** (doc + Makefile). Realizes goo/cosmic-goo split without a rewrite.
- **Move `focused_app` out of core** so the engine has no cos-cli reference.
- **`valid_when` per-verb predicate** (Kupfer's `valid_for_item`).
- **Content-dispatch rule table** (`[[dispatch]]`, plumber-style) — the deferred classifier.
- **Daemon protocol design** + a socat/FIFO spike, then Rust `good` — alongside Phase 2.
- **Ranking** (`rank_adjust`) — with the launcher.

## Decisions from the design discussion (2026-05-24)

### Command name vs package name

There is a dead-but-real Debian package named **`goo`** ("generic object-orientator
(programming language)", universe/devel, v0.155). Locally harmless (a
`~/.local/bin/goo` symlink wins on PATH). **For distribution: keep `goo` as the
everyday command, but ship the package artifact as `cosmic-goo` (or
`goo-standalone`) and expose `goo` via `update-alternatives`** — an opt-in
symlink, no hard `Conflicts: goo`. Same pattern as `vi`→`vim`.

### `valid_when` = a jq boolean expression (and it unifies with `?params`)

Add an optional per-verb predicate, evaluated against the subject JSON:

```toml
valid_when = ".id | test(\"\\\\.(zip|tar|gz)$\")"   # verb hidden unless this is true
```

- Default omitted ⇒ all items of an accepted type (today's behaviour).
- A **jq expression** is the primary form — in-process-fast, declarative, and
  subsumes the "regex / glob / map-of-mimetype→regex" options (branch on `.type`
  inside the expr). One mechanism, not three.
- Escape hatch `valid_when_cmd = "<shell test>"` for real I/O (git remote
  present, file size, device exists) — evaluated **lazily** (focused candidate /
  execute time), never in the bulk "list applicable verbs" pass.
- **rhai/embedded scripting is the Rust-era answer** (no process spawn); skip in bash.
- **Unification:** `?params` (filter a source's items at lookup, e.g.
  `@app:firefox?title=*Claude*`) and `valid_when` (filter verbs for a subject)
  are the same thing from opposite ends — *predicates over subject JSON*. Build
  one jq-predicate evaluator; both fall out. Spec `valid_when` as a jq expr so
  they share it.
- Perf: per-(verb×subject) jq spawns are cheap once, painful per-keystroke in the
  launcher → full value arrives with the in-process daemon/Rust engine.

### `object_source` — named + subject-dependent indirect objects

A two-step verb may name the source for its object, and that source may depend on
the subject (rendered with `{subject.*}` in scope):

```toml
[[verbs]]
name = "move-to-output-ws"
accepts = ["application/vnd.cos-cli.app"]
object_type = "application/vnd.cos-cli.workspace"
object_source = "workspaces"
object_list_cmd = "cos-cli info --json | jq '... | select(.output==\"{subject.metadata.output}\")'"
```

Reuses the template engine — "the object source sees the subject." This is
Kupfer's `object_source(for_item)`.

### The deferred GUI *is* Kupfer's three-pane model

The spec's libcosmic dialog mockup already draws Kupfer's GUI. Mapping the
deferred features onto panes:

| Kupfer GUI | cosmic-goo pane | powered by |
|---|---|---|
| object pane (fuzzy, ranked) | Subject | sources + `rank_adjust` |
| action pane = *valid* actions for the object | Verb | `verb_for_subject` + `valid_when` (live filter) |
| indirect-object pane `if requires_object` | Object (conditional) | `object_source` (subject-filtered) |

The compose **v0 sequential picker** is the one-pane-at-a-time degenerate case.
The native dialog "integrates ours" = build the side-by-side three-pane and wire
valid_when → verb pane, object_source → object pane, rank → ordering. Nothing to
invent; the model is settled.

### Reusing Kupfer plugins (a `kupfer-bridge`)

Feasible for the simple majority (FileLeaf/TextLeaf/AppLeaf + stateless actions
with stable string ids — most of Kupfer's catalog), via a Python harness that
imports `kupfer.*` + the plugin, exposes its `Source`s as goo sources and
`Action`s as goo verbs. **Caveats:** depends on Kupfer being importable; Leaves
holding live Python objects don't round-trip through goo's stateless string ids;
async/GUI-coupled actions need the Kupfer runtime. **Practical only with a warm
Python host ⇒ a daemon-era feature** (cold `import kupfer` per one-shot is
hundreds of ms). Good cheap way to bootstrap a plugin library once the daemon
exists.

### The Rust implementation (sketch)

Rust replaces the **engine**, not the **plugins** (plugins stay TOML + shell;
Rust assembles and `exec`s the rendered command). A cargo workspace:

- **`goo-engine`** (lib) — `Registry` (toml crate), `MimeMatcher`, `Resolver`
  (today's `address.sh`), `Dispatch` (template `{var|filter}` via
  `shell-escape`/`percent-encoding`), `DispatchRules` (plumber-style, `regex`).
  The canonical URI becomes `enum Address { Source{name,input,params},
  Scheme{scheme,value} }` with `FromStr`/`Display` — the IPC wire type.
- **`goo`** (bin) — thin client: argv → `Request`, try socket→`good`, else
  in-process engine, `exec`.
- **`good`** (bin) — warm engine + UNIX-socket server (newline-JSON), inotify on
  plugin dirs, async verbs (Kupfer late-results). Later: daemon-resident
  publisher sources (clipboard/now-playing/focus push updates instead of cold
  `list_cmd`s; the "sockets for plugins" idea).
- **`goo-compose`** (bin) — libcosmic/iced three-pane, over engine or socket.

**The bats suite is the conformance test for the port:** it drives `bin/goo`'s
observable behaviour, so a Rust `goo` that passes the same suite is provably
equivalent. The bash engine is the executable spec; Rust migrates crate-by-crate
against it.
