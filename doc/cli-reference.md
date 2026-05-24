# `goo` — CLI reference

## Synopsis

```
goo                                      # opens compose dialog (Phase 4 stub)
goo <verb> [POSITIONAL ...] [--FLAG=VALUE ...]
goo list <source>
goo describe <verb>
goo plugins
goo validate
goo compose [partial-sentence]           # Phase 4 stub
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

A Phase 4 stub. Prints a message and exits non-zero. The compose dialog (a libcosmic/iced binary) is implemented later.

## Verb invocation

Anything that isn't a known subcommand is interpreted as a verb name:

```
goo <verb> [POSITIONAL_1] [POSITIONAL_2] [--FLAG=VALUE ...]
```

### Subject addressing

A subject argument can take several forms. They all resolve through one model: each is rewritten to a canonical `cosmic-goo:` URI, then dispatched.

| You type | Means | Example |
|---|---|---|
| bare text | literal text content | `goo upper "hello"` |
| `./x`, `../x`, `/x`, `~/x` | a **file** (read contents; path in `metadata.path`) | `goo summarize ./notes.md` |
| `https://…`, `claude://…` | a **URL** (`text/x-uri`) | `goo open https://example.com` |
| `@source:query` | item from a named **source** (by `name` or `prefix`) | `goo activate @app:firefox` |
| `@source` | the source's first/default item | `goo summarize @clip` |
| `^` | the clipboard (alias for `@clip`) | `goo summarize ^` |
| `^name` | a named clipboard buffer *(reserved — not yet implemented)* | — |
| `+scheme:value` | explicit scheme handoff | `goo summarize +file:./notes.md` |
| `cosmic-goo:…` / `cosmic-goo+…` | the canonical URI directly (for scripts/IPC) | `goo summarize 'cosmic-goo+file:///abs/x.md'` |

When **no** positional is given, the subject falls back in order: **stdin** (if piped) → PRIMARY selection → clipboard → focused app (for handle verbs).

```bash
echo "text from a pipe" | goo summarize     # stdin wins when piped
goo summarize                                # no pipe → PRIMARY selection
```

Resolution rules:

- A **file** address (`./x`, `+file:`) must exist — the handler errors otherwise. An explicit path is an unambiguous "I mean a file" signal.
- The verb's `accepts` type-checks the resolved subject. A file verb fed bare text fails the type check; a text verb fed bare text works. There's no separate "mode" — enforcement is the handler (existence) plus `accepts` (type).
- `.text` is always the textual **content**; `.metadata.path` / `.id` carry identity. So `summarize ./x.md` reads the file's contents, while `open ./x.md` uses its path.

A second positional becomes the **object** (for two-step verbs like `move-to`), resolved the same way.

> The canonical `cosmic-goo:<source>:<input>` (source lookup) and `cosmic-goo+<scheme>:<value>` (scheme handoff) URIs are what the launcher meta-plugin and any IPC will pass between processes. The sigils (`@`, `^`, `+`) and native shapes are terminal-friendly shorthands that rewrite into them.

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
| `goo VERB @<TAB>` | source prefixes (`@app:`, `@ws:`, `@file:`, …) |
| `goo VERB @source:<TAB>` | items from that source (runs its `list_cmd`) |
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
