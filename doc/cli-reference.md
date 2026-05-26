# `goo` — CLI reference

## Synopsis

```
goo                                      # no args: prints this usage (a true CLI — never launches a GUI)
goo <verb> [POSITIONAL ...] [--FLAG=VALUE ...]
goo <address>                            # no verb: resolve the address, run its type's default verb
goo list <source>
goo describe <verb>
goo plugins
goo validate
goo compose                              # build a sentence (scripted via GOO_COMPOSE_ANSWERS)
goo --help | -h | help
```

## Subcommands

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

Exit 0 if everything looks good; non-zero with diagnostics on stderr otherwise. Prints a one-line summary on success: `goo validate: OK (N plugins, T types, S sources, V verbs, A adverbs)`.

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

> **Interactive** picker-driven compose (auto-detecting `fuzzel`/`rofi`/`wofi`/`fzf`, `zenity` fallback; override with `GOO_PICKER`) lives in the bash engine — `bin/goo compose` — and, ahead, in the native libcosmic `goo-compose` dialog ([#39](https://github.com/teaguesterling/cosmic-goo/issues/39)). The Rust CLI stays a pure command-line tool; spawning a launcher is a GUI front-end's job, not the CLI's.

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

## Verb invocation

Anything that isn't a known subcommand is interpreted as a verb name:

```
goo <verb> [POSITIONAL_1] [POSITIONAL_2] [--FLAG=VALUE ...]
```

### Subject addressing

A subject argument can take several forms. They all resolve through one model: each is rewritten to a canonical `goo://` URI, then dispatched.

| You type | Means | Example |
|---|---|---|
| bare text | literal text content | `goo upper "hello"` |
| `./x`, `../x`, `/x`, `~/x` | a **file** (read contents; path in `metadata.path`) | `goo summarize ./notes.md` |
| `https://…`, `claude://…` | a **URL** (`text/x-uri`) | `goo open https://example.com` |
| `:source:query` | item from a named **source** (by `name` or `prefix`) | `goo activate :app:firefox` |
| `:source:query?k=v` | …further filtered by field (case-insensitive substring; `*` optional) | `goo activate :app:firefox?title=*Claude*` |
| `:source` | the source's first/default item | `goo summarize :clip` |
| `+scheme:value` | explicit scheme handoff | `goo summarize +file:./notes.md` |
| `^` / `^name` | clipboard (a built-in **custom sigil** → `+clip:`; `^name` reserved) | `goo summarize ^` |
| `goo://…` / `goo+…` | the canonical URI directly (for scripts/IPC) | `goo summarize 'goo+file:///abs/x.md'` |

`:` and `+` are the two **core** sigils. `:source:input` rewrites to the canonical, registrable URL form **`goo://source/input`** (`?params` ride along); `+scheme:value` rewrites to **`goo+scheme:value`** (a direct handoff). Everything else is a **customizable sigil**: a single character that expands into one of those forms. `^` → `+clip:` ships as a default; `@` ships intentionally undefined (claim it in your own config). Define your own in any plugin TOML:

```toml
[[sigils]]
char = "@"
expands = ":app:"     # then @firefox -> :app:firefox -> goo://app/firefox
```

When **no** positional is given, the subject falls back in order: **stdin** (if piped) → PRIMARY selection → clipboard → focused app (for handle verbs).

```bash
echo "text from a pipe" | goo summarize     # stdin wins when piped
goo summarize                                # no pipe → PRIMARY selection
```

Resolution rules:

- A **file** address (`./x`, `+file:`) must exist — the handler errors otherwise. An explicit path is an unambiguous "I mean a file" signal.
- The verb's `accepts` type-checks the resolved subject. A file verb fed bare text fails the type check; a text verb fed bare text works. There's no separate "mode" — enforcement is the handler (existence) plus `accepts` (type).
- **The subject convention:** `.text` is the **content/value** (what it *is*); `.id` is the **address/locator** (how to *refer to* it — a path for a file, the URL for a link, the handle for an app). An entity is *addressable* iff it has an `.id`; pure text values have only `.text`. So `summarize ./x.md` reads the file's contents (`.text`), while `open ./x.md` (or `open https://…`) acts on its locator (`.id`) — one polymorphic `open` covers files and URLs because both carry an `.id`.

A second positional becomes the **object** (for two-step verbs like `move-to`), resolved the same way.

### The `GOO` default verb (running an address directly)

If the first argument is an **explicit address** (a `goo://` URL, a sigil/native shape — anything `is_explicit`) and **not** a verb, `goo` resolves it and runs the **`default_for` verb** of the resolved subject's type. This is the CLI form of the protocol's `GOO` verb — "do the sensible default with this thing":

```bash
goo goo://br/main      # → branch-log  (git-branch type's default_for)
goo :ps:1              # → proc-info    (process type's default_for)
goo ~/notes.md         # → the inode/* default verb (e.g. open)
```

If the resolved type has **no `default_for` verb**, `goo` errors (`no default verb for type '<type>'`) rather than guessing — it never picks among non-default verbs. A bare word that isn't an address stays a verb lookup (so `goo nope` is still "unknown verb", not a subject).

> This is only the **loose CLI surface** of [the goo request protocol](design/goo-protocol.md) — the `GOO` default verb over a `goo://` subject. The full wire form (HTTP-shaped methods, `Using:`/`To:`/`With:` headers, status codes, a unix-socket daemon) is designed there but **daemon-gated** — not built. `GOO` is the only protocol verb the CLI implements today.

> The canonical `goo://<source>/<input>` (source lookup) and `goo+<scheme>:<value>` (scheme handoff) URIs are what the launcher meta-plugin and any IPC will pass between processes. You can already run one directly — `goo goo://app/firefox` resolves it and runs the type's default verb (see [the `GOO` default verb](#the-goo-default-verb-running-an-address-directly)). The sigils (`@`, `^`, `+`) and native shapes are terminal-friendly shorthands that rewrite into the same URIs. Still **unbuilt**: registering `goo://` as `x-scheme-handler/goo` so `xdg-open goo://app/firefox` (or a browser click) routes to `goo`. The fuller [request protocol](design/goo-protocol.md) and the [REST/WebDAV-shaped addressing model](design/addressing-and-protocol.md) are considered designs, daemon-gated.

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

```bash
# Render the critique prompt and copy it to the clipboard
goo critique "this passage could use more concrete examples"

# Same, but use the current PRIMARY selection as the subject
goo critique --via=clipboard

# Multi-adverb invocation: think harder than usual, route to clipboard
goo think "the nature of recursion" --depth=ultra --via=clipboard

# Activate a running app by name (handle resolution from positional)
goo activate Firefox

# List the items a source surfaces, for debugging
goo list apps | jq .
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
| `COS_CLI` | Override the `cos-cli` binary used by `focused_app` and the apps plugin | Auto-resolved from PATH or `$HOME/.cargo/bin/cos-cli` |

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
| `goo VERB --<TAB>` | adverbs the verb opts into (`--name=`) |
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
