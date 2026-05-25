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
