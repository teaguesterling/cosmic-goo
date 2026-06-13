# `goo` — CLI reference

## Synopsis

```
goo                                      # no args: prints this usage (a true CLI — never launches a GUI)
goo <verb> [POSITIONAL ...] [--FLAG=VALUE ...]
goo <verb> <subject> [--as TYPE] [--to DEST | -o FILE] [--using CHANNEL] [--hops N | --force]
goo <address>                            # no verb: resolve the address, run its type's default verb
goo do <address> [verb] [args …]         # noun-first: list a subject's verbs, or run one
goo what <subject|=TYPE>                  # list the verbs applicable to a subject (recency-first)
goo again [subject]                       # repeat the last verb (+adverbs) on a new subject
goo forget                               # clear the recorded action history
goo reload                               # drop the entity-list cache (force a fresh re-read)
goo --explain <verb> [=TYPE|subject] …   # show the negotiation plan (route / 415) — never executes
goo -c <file|dir> <verb> …               # merge an extra plugin config for this run (repeatable)
goo list <source>
goo describe <verb>
goo plugins
goo validate
goo dispatch <input>                     # classify raw content and route it to a verb
goo compose                              # build a sentence (scripted via GOO_COMPOSE_ANSWERS)
goo options <subject|=TYPE>              # JSON: applicable verbs + their slots (discovery; unstable v1)
goo --help | -h | help
```

## Subcommands

### `goo do <address> [verb] [args …]`

Noun-first dispatch: name the **subject** first, then optionally a verb. With **no
verb**, it lists the verbs that apply to the subject (recency-first, the same view as
`goo what`); with a verb, it runs it. It's a pure reorder of `goo <verb> <subject>` —
handy when you have the thing in hand and want to see your options.

```
$ goo do :app/firefox            # name the app → see its verbs
$ goo do :app/firefox close      # → goo close :app/firefox
$ goo do .config.json json-keys  # → goo json-keys .config.json
```

### `goo what <subject | =TYPE>`

Lists the verbs applicable to a subject, **most-recently-used first**, with destructive
verbs flagged `[!]`. Read-only discovery — it resolves the subject's type and projects
the verbs that accept it (including those reached by type coercion). `=TYPE` asks the
question about a type virtually, with no real subject.

```
$ goo what :repo:cosmic-goo
applicable verbs for :repo:cosmic-goo  (type: application/vnd.git.repo)
    status       Show short status
    pull  [!]    Pull the current branch
    gh-pr-list   List open GitHub PRs
    open         Open in its default application   # a repo is_a directory, so it
    reveal       Open the containing folder        # inherits the file verbs too
    …
```

### `goo again [subject]` / `goo forget`

`goo again` repeats your **last verb (and its adverbs)** on a new subject — say it once,
then aim it at the next thing. With no subject it re-runs the whole previous invocation.
`goo forget` clears the recorded action history. History recording is off when
`GOO_NO_HISTORY=1` is set.

```
$ goo summarize ./report.md --model=big
$ goo again ./minutes.md          # summarize --model=big, new subject
$ goo forget                      # clear the history
```

### `goo reload`

Drops the entity-list cache, forcing a fresh re-read of source `list_cmd`s on the next
address resolution. Use it when the desktop changed underneath a warm cache (a new app
launched, a device paired) and a sigil isn't seeing it yet.

### `goo plugins`

Lists every plugin loaded from the search dirs, with file paths.

```
$ goo plugins
text-verbs — Selection-aware text actions (critique, summarize, think, draft-response)
  /home/you/.config/cosmic-goo/plugins/text-verbs.toml
```

Exit 0 always; prints a hint to stderr if the registry is empty.

### `goo validate`

Walks the registry and checks for structural problems:

- empty `accepts` patterns on verbs
- adverbs missing both `applies_to` and `applies_to_verbs` scope
- selector adverbs with no `values`

Exit 0 if everything looks good; non-zero with diagnostics on stderr otherwise. Prints a one-line summary on success — for the built-in set today:

```
$ goo validate
goo validate: OK (30 plugins, 19 types, 21 sources, 92 verbs, 5 adverbs, 2 sigils, 0 aliases, 5 channels, 0 dispatch)
```

### `goo describe <verb>`

Prints a human-readable summary of the verb: name, description, `accepts`, `object_type` if any, `default_for`, `uses_adverbs`, the `cmd` or `prompt` body, and which plugin contributed it.

```
$ goo describe critique
verb: critique
description: Expert review of the passage
accepts: text/*
uses_adverbs: via
prompt:
  You are providing expert review of the following passage.
  Deduce the desired intent and tone, then critique accordingly.

  ---
  {subject.text}
provided by plugin: text-verbs
```

### `goo list <source>`

Runs the named source's `list_cmd` and prints the raw JSON it produces. Useful for debugging plugins and for piping into other tools.

```
$ goo list apps | jq '.[].id'
"Alacritty"
"Claude"
```

### `goo compose`

Builds a sentence step by step — **subject** → **verb** (filtered to those that accept the subject's type) → **object** (if the verb takes one) → **adverb** values → confirm → execute — and runs it through the same path as a plain `goo <verb> <subject> …`.

The `goo` CLI itself is **non-interactive**: it drives compose only from the scripted `GOO_COMPOSE_ANSWERS` queue (a file with one choice per line — used by tests and automation). It deliberately **never launches a GUI**; with no answers file it just prints a hint.

```bash
printf '%s\n' :clip: wrap dump yes > answers
GOO_COMPOSE_ANSWERS=answers goo compose
```

> **Interactive** compose lives in dedicated front-ends, not the Rust CLI (which stays a
> pure command-line tool — spawning a launcher is a GUI's job). The shipped one is
> **`goo-compose-gui`** (iced): a keyboard-first, gnome-do/Kupfer-style launcher. Type to
> filter a **Subject**, then a **Verb** (filtered to those that accept it), then an
> **Object** and adverbs, watch the exact `goo …` command assemble live, then run it —
> with the result (or a recoverable error, with retry/edit) shown in place. The bash
> engine also keeps a picker-driven `bin/goo compose` (auto-detecting
> `fuzzel`/`rofi`/`wofi`/`fzf`, `zenity` fallback; override with `GOO_PICKER`). A native
> **libcosmic** port of the compose GUI is planned; the iced version ships today.

### `goo dispatch <input>`

Classifies raw content and routes it to a verb — the plumber-style "just do the
sensible thing with this datum" entry point. It reads a positional or piped
stdin, then:

1. tries each `[[dispatch]]` rule in load order; the **first** whose `matches`
   ERE hits wins. Its `type`, `set` (with `${N}` capture interpolation), `verb`,
   and `adverbs` build and route the subject. Matching is single-shot — a
   rewritten subject is not re-fed through the table.
2. if no rule matches, falls back to native subject detection plus the detected
   type's `default_for` verb.

```bash
goo dispatch "RFC 2616"              # a rule → open on the rfc-editor URL
echo "https://example.com" | goo dispatch   # no rule → text/x-uri default verb (open)
```

Rules live in plugin TOML (see [plugin authoring](plugin-authoring.md#content-dispatch)); none ship by default — dispatch only does what your config tells it to. `goo validate` checks every rule has a `matches` and a `verb` that exists.

### `goo options <subject | =TYPE>`

The OPTIONS discovery surface ([goo-protocol §7](design/goo-protocol.md)): the verbs
applicable to the subject and, per verb, the slots a caller can fill — `Using:`
(instrument channels), `With:` (adverbs + their choices, mirroring the run-path
`uses_adverbs` gate), and `object_type` for two-step verbs. Emits JSON — the single
composable shape the compose-gui's verb-pick, completion, and (later) the `good`
daemon all consume. Read-only.

```
$ goo options @text/markdown
{ "schema_version": "0.1", "stable": false, "type": "text/markdown",
  "default": null,
  "allow": ["critique", "summarize", "think", …],
  "verbs": { "think": { "using": [],
    "with": { "via":   {"kind":"selector","default":"woollama","values":["woollama","clipboard",…]},
              "model": {"kind":"selector","default":"fast","values":["fast","local","code","big"]},
              "depth": {"kind":"selector","default":"normal","values":["normal","ultra"]} },
    "object_type": null } … } }
```

The JSON shape is **unstable through v1** — consumers gate on `schema_version`.
`to:` (write-destination choices) is intentionally absent; it ships with the
`{write}`-domain framework. The output never includes verb internals (`cmd`,
`prompt`, `description`) — that's the projection contract.

## Verb invocation

Anything that isn't a known subcommand is interpreted as a verb name:

```
goo <verb> [POSITIONAL_1] [POSITIONAL_2] [--FLAG=VALUE ...]
```

### Subject addressing

A subject argument can take several forms. They all resolve through one model: each is rewritten to a canonical `goo://` URI, then dispatched.

| You type | Means | Example |
|---|---|---|
| bare text | literal text content (shape-inferred) | `goo upper "hello"` |
| `./x`, `../x`, `/x`, `~/x` | a **file** (read contents; path in `metadata.path`) | `goo summarize ./notes.md` |
| `https://…`, `claude://…` | a **URL** (`text/x-uri`) | `goo open https://example.com` |
| `+text` | **literal text**, no inference | `goo upper +./not-a-path` |
| `:dom/path` | a **value** in a domain — the **exact** id | `goo activate :app/firefox` |
| `:dom:query` | a **search** in a domain — **fuzzy** id/title | `goo activate :app:firefox` |
| `:dom:query?k=v` | …refined by field (case-insensitive substring; `*` optional) | `goo activate :app:firefox?title=*Claude*` |
| `:dom` | the domain's first/default item | `goo summarize :clip` |
| `^` / `^name` | clipboard / named buffer (built-in) | `goo summarize ^` |
| `=<mime>` | **virtual-type assertion** (subject is *just* that type — no content, no id) | `goo options =text/markdown` |
| `goo://dom/path` | the canonical URI directly (machines/IPC) | `goo summarize 'goo://file//abs/x.md'` |

**One canonical form:** everything rewrites to `goo://<domain>/<path>[;q=<query>][?refine]`. A **value** (`goo://app/firefox`, sigil `:app/firefox`) is the **exact** id; a **search** (`goo://app/;q=firefox`, sigil `:app:firefox`) is **fuzzy** over the domain's items. Resolution is strict — the form says which you mean. The built-in **value domains** `text` / `file` / `clip` / `sel` / `stdin` / `url` cover the non-source subjects; every other domain is a `[[sources]]` entry (by `name` or `prefix`).

#### The built-in domains

Each `[[sources]]` plugin contributes a domain you address with `:dom/exact-id` (value) or
`:dom:query` (fuzzy). The built-in set today:

| sigil | addresses | sigil | addresses |
|---|---|---|---|
| `:app:` | a running app | `:bt:` | a Bluetooth device |
| `:win:` | a window | `:net:` | a network connection |
| `:ws:` | a workspace | `:ssh:` | an SSH host (`~/.ssh/config`) |
| `:repo:` | a git repository | `:svc:` | a systemd user service |
| `:br:` | a git branch | `:ctr:` | a container (docker/podman) |
| `:file:` | a file in the cwd | `:sink:` | an audio output |
| `:recent:` | a recently-opened file | `:tmux:` | a tmux session |
| `:mnt:` | a mount point | `:ps:` | a process |
| `:hist:` | a clipboard-history entry | `:emo:` | an emoji |
| `:sel:` | the PRIMARY selection (text) | `:clip:` / `^` | the clipboard (text) |

A few **id shapes** are worth knowing, because the exact (`/`) form needs the literal id:
app ids are the app id (`:app/firefox` ✓), but **repo** ids are filesystem **paths**, so
`:repo/cosmic-goo` won't resolve — use the fuzzy `:repo:cosmic-goo`. When unsure, fuzzy
(`:dom:query`) is the forgiving form. `goo list <source>` dumps a domain's raw ids.

Sigils are terminal shorthand (machines emit `goo://` directly). The built-ins — `:` (domained: `/`=value, `:`=search), `+` (text), `^` (clip) — use only shell-unquoted characters, so you never quote an address. Everything else is a **user alias**: a single char that expands into a goo:// form. `@` ships undefined — claim it:

```toml
[[sigils]]
char = "@"
expands = "goo://app/"     # then @firefox -> goo://app/firefox (a value);
                           # use ":app:" to expand to a search instead
```

When **no** positional is given, the subject falls back in order: **stdin** (if piped) → PRIMARY selection → clipboard → focused app (for handle verbs).

```bash
echo "text from a pipe" | goo summarize     # stdin wins when piped
goo summarize                                # no pipe → PRIMARY selection
```

Resolution rules:

- A **file** address (`./x`, `~/x`, `:file/x`) must exist — the handler errors otherwise. An explicit path is an unambiguous "I mean a file" signal.
- The verb's `accepts` type-checks the resolved subject. A file verb fed bare text fails the type check; a text verb fed bare text works. There's no separate "mode" — enforcement is the handler (existence) plus `accepts` (type).
- **The subject convention:** `.text` is the **content/value** (what it *is*); `.id` is the **address/locator** (how to *refer to* it — a path for a file, the URL for a link, the handle for an app). An entity is *addressable* iff it has an `.id`; pure text values have only `.text`. So `summarize ./x.md` reads the file's contents (`.text`), while `open ./x.md` (or `open https://…`) acts on its locator (`.id`) — one polymorphic `open` covers files and URLs because both carry an `.id`.

A second positional becomes the **object** (for two-step verbs like `move-to`), resolved the same way.

### The `GOO` default verb (running an address directly)

If the first argument is an **explicit address** (a `goo://` URL, a sigil/native shape — anything `is_explicit`) and **not** a verb, `goo` resolves it and runs the **`default_for` verb** of the resolved subject's type. This is the CLI form of the protocol's `GOO` verb — "do the sensible default with this thing":

```bash
goo goo://br/main      # → log    (git-branch type's default_for)
goo :ps:1              # → info   (process type's default_for; polymorphic)
goo ~/notes.md         # → the inode/* default verb (e.g. open)
```

If the resolved type has **no `default_for` verb**, `goo` errors (`no default verb for type '<type>'`) rather than guessing — it never picks among non-default verbs. A bare word that isn't an address stays a verb lookup (so `goo nope` is still "unknown verb", not a subject).

> This is only the **loose CLI surface** of [the goo request protocol](design/goo-protocol.md) — the `GOO` default verb over a `goo://` subject. The full wire form (HTTP-shaped methods, `Using:`/`To:`/`With:` headers, status codes, a unix-socket daemon) is designed there but **daemon-gated** — not built. `GOO` is the only protocol verb the CLI implements today.

> The single canonical `goo://<domain>/<path>` URI is what the launcher meta-plugin and any IPC pass between processes. You can already run one directly — `goo goo://app/firefox` resolves it and runs the type's default verb (see [the `GOO` default verb](#the-goo-default-verb-running-an-address-directly)). The sigils (`:`, `+`, `^`, `@`) and native shapes are terminal-friendly shorthands that rewrite into the same URI. Still **unbuilt**: registering `goo://` as `x-scheme-handler/goo` so `xdg-open goo://app/firefox` (or a browser click) routes to `goo`. The fuller [request protocol](design/goo-protocol.md) (the wire/daemon layer) and the [domain model](design/addressing-and-protocol.md) (the URI layer) are the design docs behind this.

### Command aliases

An **alias** is a whole-invocation shortcut: a name that expands, at the verb
position, into a verb plus any adverb flags and/or a subject. Where a *sigil*
abbreviates a **subject** (`@firefox` → `:app:firefox`), an alias abbreviates the
**whole sentence**. Define them in any plugin TOML (usually your user config):

```toml
[[aliases]]
name = "g"
expands = "search --engine=google"   # then: goo g "rust traits"
description = "Google web search"      # optional, shown in completion/help

[[aliases]]
name = "note"
expands = "append-to ~/notes.md"      # an alias may bake in a subject/object too
```

`goo g "rust traits"` becomes `goo search --engine=google "rust traits"` — the
alias tokens come first, your trailing arguments follow. The expansion is
re-dispatched, so an alias may chain to another alias; a depth guard breaks
cycles. Aliases can never shadow a subcommand (`list`, `describe`, …); an alias
sharing a verb's name *does* win (that's the point), and `goo validate` warns
when one does. Alias names also complete at `goo <TAB>` alongside verbs.

### Flag forms

| Form | Example | Meaning |
|---|---|---|
| `--flag=value` | `--via=clipboard` | named adverb with explicit value |
| `--flag value` | `--via clipboard` | same — two-token form |
| `--flag` (no value) | `--confirm` | bare flag, value becomes `true` |

Adverbs accumulate into a single JSON object passed to the verb dispatcher.

### Examples

The grammar is one shape — `goo <verb> <subject>` — across every kind of desktop thing:

```bash
# Apps & windows
goo activate :app/firefox            # focus a running app (default verb — just `goo :app/firefox`)
goo move-to :app:alacritty :ws/0:1   # verb + subject + object: move an app to a workspace

# Git repos & branches
goo status :repo:cosmic-goo          # short git status (fuzzy-match the repo — its id is a path)
goo gh-pr-list :repo:cosmic-goo      # list open GitHub PRs for the repo
goo log :br/main                     # a branch's default verb

# Files & directories
goo open ./report.pdf                # open a file in its default app
goo tree ./doc                       # two-level listing of a directory

# Devices — one polymorphic verb across domains
goo connect :bt/headphones           # pair a Bluetooth device
goo connect :ssh/prod                # …or open an SSH session — same verb

# Coercion: ask a JSON verb for a CSV; goo finds the route and takes it
goo json-keys data.csv               # text/csv → csv2json → application/json → json-keys

# Text is one more subject family (borrows the selection/clipboard with no subject)
goo summarize ./notes.md --model=big # an LLM verb with a model adverb
goo upper "hello"                    # a pure text util

# Discovery & debugging
goo what :sink:speakers              # what can I do with this audio output?
goo list apps | jq .                 # raw items a source surfaces
```

## Presentation & coercion (the negotiation engine)

`goo` doesn't only run a verb's `cmd`. When the subject's type isn't one the verb
`accepts`, goo inserts type conversions **before** the verb (input coercion); when
the result needs to reach a particular place or representation, it inserts
conversions **after** (output negotiation). The conversions are declared
`[[channels]]` (see [plugin authoring](plugin-authoring.md#channels-type-coercion))
and goo plans the cheapest route through them. This is why

```bash
goo json-keys data.csv      # json-keys accepts application/json; csv is coerced first
```

works — `data.csv` is routed `text/csv → [csv→json channel] → application/json`,
then the verb runs (given the channel's converter tool, here `mlr`, is installed; if it
isn't, the 415 names it). No matching route ⇒ a clean **`415`** (it never runs the verb
on the wrong type).

### Coercion & routing flags

| Flag | Meaning |
|---|---|
| `--as TYPE` | Pin the output **representation** (the Accept). `goo view img.png --as text/x-ansi` forces the inline-ANSI rendering even on a desktop. |
| `--to DEST` | Route the result to a **destination** instead of stdout. `DEST` is an address: a file (`goo://file/…` or a path), or `^` for the clipboard. |
| `-o FILE` | Sugar for `--to` a file. `goo upper "hi" -o out.txt`. |
| `--using CHANNEL` | Pin which **channel** carries out a verb that's implemented by `usage` channels (the instrument). A constraint, validated against the verb's `usage`. |
| `--hops N` | Allow up to **N** input-coercion hops (default 1). For routes that need a longer conversion chain. |
| `--force` | Lift the hop bound entirely (unbounded coercion, both directions). |

`--to` makes the result **bytes** (it's going to a sink, not a terminal), so binary
flows intact — with the demo config's `qr-png` verb,
`goo -c doc/examples/goo-demo.toml qr-png "wifi" -o code.png` writes a real PNG, not
a rendered surface. The flags compose: `goo view image.png --as text/x-ansi -o frame.txt`
coerces the image to inline ANSI and routes that to the file.

### Earned hops — auto-coercion is bounded

By default goo takes **at most one** converter hop on each side (input, output): a
single, obvious conversion is convenient; a deep chain should be deliberate. When a
route needs more than the budget, goo refuses with a `415` that **teaches** — it
re-searches deeper and prints the route it found plus the exact flag to allow it:

```
$ goo up2 notes.txt
goo: 415 · no route within 1 input hop(s) — a deeper route exists:
    text/plain → up → text/x-up → upup → text/x-upup → (up2) → text/plain
  allow it with --hops 2 (or --force)
```

`--hops N` raises the **input**-coercion budget; a longer **output** chain needs
`--force` (the message says which). A 415 caused by an uninstalled tool instead
prints `— install: <tool>` naming the tools on the would-be route.

### `goo --explain` — the plan explainer

A read-only view of what goo *would* do: the Accept profile, the typed subject (and
which signal chose the type), and the planned route — or the `415`. It never runs
anything, and `=<mime>` (or `:type/<mime>`, or `goo://type/<mime>`) lets you assert
a subject type virtually (no file needed).

```
goo --explain <verb> [=TYPE | <subject>] [flags]
```

| Flag | Meaning |
|---|---|
| `--as TYPE`, `--using CHANNEL`, `--hops N`, `--force` | Preview the run under these constraints (same semantics as the run path). |
| `--explain-env tty\|cosmic\|desktop\|piped` | Override the detected environment (default: isatty + `$WAYLAND_DISPLAY`). |
| `--explain-with route\|steps\|shell` | Detail view. `route` = the type-route one-liner; `steps` = numbered transitions + each `cmd` template; `shell` = the commands in run order with the subject filled in. Default: **adaptive** (commands for a ≤2-hop route, annotated steps beyond). |
| `--paths [--max-hops C] [--format text\|mermaid]` | Enumerate **all** routes to a satisfiable Accept (the route-graph debugger), not just the chosen one. `--max-hops` bounds depth (default 3); `--format mermaid` emits a `graph LR` diagram. |

The route line is **richly rendered on a TTY** — cost shown by color, `lossy`/`network`
edges flagged — and plain ASCII when piped.

```bash
goo --explain view @image/png --explain-env cosmic      # how would an image render here?
goo --explain json-keys data.csv --explain-with shell    # show the commands it'd run
goo --explain json-keys @text/csv --paths --format mermaid | <mermaid viewer>
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | usage error, unknown verb / source / subject, validation failure, route failure |
| 130 | user cancelled a confirmation prompt (`confirm = true` on the verb) |

## Environment variables

| Var | Purpose | Default |
|---|---|---|
| `COSMIC_GOO_BUILTIN_PLUGINS_DIR` | Where built-in plugins live | `$REPO/plugins` for a dev checkout; `/usr/share/cosmic-goo/plugins` for a system install |
| `XDG_CONFIG_HOME` | Standard XDG; user plugin dir is `$XDG_CONFIG_HOME/cosmic-goo/plugins` | `$HOME/.config` |
| `COSMIC_GOO_EXTRA_CONFIG` | Extra plugin config(s) to merge last (highest precedence). Set by `-c`/`--config`; colon-separate multiple. | unset |
| `COSMIC_GOO_MIME_DIRS` | Opt-in: import the OS [shared-mime-info](https://specifications.freedesktop.org/shared-mime-info-spec/) DB (its `subclasses` → `is_a` lattice, `globs2` → extension map) at registry load. Colon-separated dirs (e.g. `/usr/share/mime`). | unset (no OS-MIME import) |
| `COS_CLI` | Override the `cos-cli` binary used by `focused_app` and the apps plugin | Auto-resolved from PATH or `$HOME/.cargo/bin/cos-cli` |

### `-c` / `--config <file\|dir>`

Merge an extra plugin TOML (or a directory of them) for this run only, applied
**last** so it overrides everything else — handy for trying a config without
installing it. Repeatable. Equivalent to setting `COSMIC_GOO_EXTRA_CONFIG`.

```bash
goo -c ./doc/examples/duckdb-formats.toml json-keys data.parquet
```

## Shell completion

A bash completion script lives at [`completions/goo.bash`](https://github.com/teaguesterling/cosmic-goo/blob/main/completions/goo.bash). It completes subcommands, verb names, source names, adverb flags, and adverb values — driven by the hidden `goo __complete <stage>` surface so the candidate list always matches the loaded registry.

Install to the standard XDG user location:

```bash
make install-completion
```

Or source it directly from your shell init:

```bash
. /path/to/cosmic-goo/completions/goo.bash
```

What it completes:

| At cursor | TAB completes |
|---|---|
| `goo <TAB>` | subcommands + verb names |
| `goo describe <TAB>` | verb names |
| `goo list <TAB>` | source names |
| `goo VERB --<TAB>` | adverbs the verb opts into (`--name=`) — **subject-aware** when a subject is on the line, via the [OPTIONS surface](#goo-options-subject-type) so the keys match `uses_adverbs` at the run path |
| `goo VERB --flag=<TAB>` | selector values for that adverb |
| `goo VERB :<TAB>` | source prefixes (`:app:`, `:ws:`, `:file:`, …) |
| `goo VERB :source:<TAB>` | items from that source (runs its `list_cmd`) |
| `goo HANDLE-VERB <TAB>` | items for a handle verb (e.g. `goo activate <TAB>` → running apps) |

Handle-verb and `@source:` completions invoke source `list_cmd`s on demand, so they're as fast as the underlying tool (cos-cli ~80ms). Text-verb positionals aren't completed (the subject is freeform prose, a path, or stdin).

Zsh completion piggybacks on the same `__complete` backend — script TBD.

## Plugin discovery order

`goo` loads plugins from these directories in order. Later wins for name collisions, so user plugins override built-ins:

1. `COSMIC_GOO_BUILTIN_PLUGINS_DIR` (or `/usr/share/cosmic-goo/plugins`)
2. `/etc/cosmic-goo/plugins/`
3. `$XDG_CONFIG_HOME/cosmic-goo/plugins/` (typically `~/.config/cosmic-goo/plugins/`)
4. `$PWD/.cosmic-goo/plugins/` (project-local override, last-wins)

A plugin is either a single TOML at `<dir>/<name>.toml` or a directory `<dir>/<name>/` containing `plugin.toml` plus binaries/scripts.
