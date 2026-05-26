# Plugin authoring

A cosmic-goo plugin is a TOML file (or directory containing one) that contributes any combination of **types**, **sources**, **verbs**, and **adverbs** to the global registry. Plugins are stateless data with embedded command templates; the dispatcher does the work.

## The smallest useful plugin

```toml
# ~/.config/cosmic-goo/plugins/shout.toml
name = "shout"

[[verbs]]
name = "uppercase"
accepts = ["text/*"]
cmd = "tr a-z A-Z <<< {subject.text|q}"
```

Save, then `goo uppercase "hello world"` prints `HELLO WORLD`.

A few things happened there:

- The plugin's `name` becomes its identifier in the registry; collisions with built-ins are resolved by load order (user wins).
- `accepts = ["text/*"]` registers the verb as applicable to anything text-typed. `text/*` is a MIME glob (`text/plain`, `text/markdown`, `text/x-python`, ...).
- `cmd` is a command template. `{subject.text|q}` is substituted with the subject's text content, shell-quoted, before bash runs the result. The `|q` filter makes it safe against arbitrary content (quotes, spaces, newlines); a bare `{subject.text}` would be inserted raw. See [Filters](#filters-making-substitutions-safe).

## File layout

A plugin can be:

| Form | Layout | When to use |
|---|---|---|
| Single file | `<dir>/<name>.toml` | Most plugins. Just declarations. |
| Directory | `<dir>/<name>/plugin.toml` plus siblings | When you ship helper scripts alongside (e.g. `<dir>/<name>/bin/list-things.sh`). |

For the directory form, templates can reference `{plugin.dir}` (planned — currently you use the absolute path or `$HOME`-relative path; relative-to-plugin support comes with template filters).

## Sections

A plugin file is a flat TOML document with these optional sections:

```toml
name = "my-plugin"
description = "one-line description"

[[types]]    # vendor MIME type declarations
[[sources]]  # places to enumerate typed items
[[verbs]]    # named actions
[[adverbs]]  # modifiers on verbs
```

Each section is an array of tables. A plugin can contribute any combination, including just one section.

### Types

A type lets you give a stable name to a kind of object the system isn't going to detect by sniffing. Standard content types (`text/plain`, `application/json`, `image/png`) you don't need to declare — libmagic / `wl-paste --list-types` know about them. Declare **vendor types** for handle-like objects (a running app, a workspace, a scene):

```toml
[[types]]
name = "application/vnd.my-tool.thing"
display = "my-tool thing"
kind = "handle"     # "handle" = something to find; "content" = bytes (rarely declared)
```

Naming convention: `application/vnd.<tool-name>.<subtype>`. Vendor namespaces are first-come-first-served by convention — don't squat on names you don't maintain.

#### `is_a` — declaring a supertype

`accepts` matching is **subtype-aware**. A verb's `accepts` patterns match not only by MIME glob but up a subtype lattice. Three things make `X` a subtype of an `accepts` pattern `P`:

1. the glob (`P = "text/*"` matches `text/markdown` — as always);
2. the structured-suffix rule (`application/vnd.foo+json` is a subtype of `application/json`, same top-level type);
3. **declared `is_a`** — list one or more supertypes on a `[[types]]` entry and the lattice walks them transitively:

```toml
[[types]]
name = "application/vnd.my-tool.note"
is_a = ["text/markdown"]   # any verb accepting text/markdown (or text/*) now applies
```

`is_a` is a DAG (cycles are guarded). Use it to plug a vendor handle/content type into existing verb vocabularies without re-declaring `accepts` everywhere. (Lattice resolution is in the Rust engine; the bash reference matches by glob + suffix only.)

### Sources

A source is a place to enumerate typed items. Each source declares one primary `emits` type and a `list_cmd` that produces JSON:

```toml
[[sources]]
name = "things"
prefix = "thing"               # for :thing addressing (and launcher scoping)
icon = "applications-other"    # freedesktop icon-theme name
emits = "application/vnd.my-tool.thing"
list_cmd = "my-tool list --json"
preview_cmd = "my-tool show {subject.id}"   # optional
enumerate = false              # optional; default true
```

`list_cmd` must produce JSON on stdout — an array of objects, each at minimum with `id` and `title`. Optional fields: `subtitle`, `metadata` (free-form, opaque to the dispatcher but available to verb templates as `{subject.metadata.field}`).

**`enumerate`** (default `true`) controls whether the source is *bulk-listed*. Contexts that gather candidates from many sources at once — the `goo compose` subject picker, and bare-positional tab completion (`goo VERB <TAB>`) — run every enumerable source's `list_cmd`. Set `enumerate = false` for sources that are slow (a network probe), huge (clipboard history), or noisy (every file in the tree): they're then **reachable on demand** via `:prefix:query` and `:prefix:<TAB>`, but never run in bulk. The built-in `bluetooth`, `files`, `services`, `repos`, and `clipboard-history` sources use this.

```json
[
  {"id": "stable-id-1", "title": "Display name", "subtitle": "More info", "metadata": {"path": "/..."}},
  {"id": "stable-id-2", "title": "Another"}
]
```

The `id` must be stable across invocations — it's what verb templates receive as `{subject.id}`.

### Verbs

The action layer. A verb has at minimum a name, an `accepts` list (one or more MIME globs), and either a direct `cmd` or a `prompt` plus adverbs.

**Direct verb** with a single command template:

```toml
[[verbs]]
name = "uppercase"
accepts = ["text/*"]
cmd = "tr a-z A-Z <<< {subject.text|q}"
```

**Two-step verb** taking an object. Declare `object_type`; the object is then
available as `{object.*}` in `cmd`, resolved the same way subjects are:

```toml
[[verbs]]
name = "move-to"
accepts = ["application/vnd.my-tool.thing"]
object_type = "application/vnd.my-tool.workspace"
cmd = "my-tool move --thing {subject.id} --workspace {object.id}"
```

Where the object's candidates come from, in priority order:

1. an explicit address as the second positional (`goo move-to :thing:x :ws:2`) — resolved directly, bypassing the pool;
2. **`object_list_cmd`** — a shell snippet emitting a JSON array, with `{subject.*}` substituted in first, so candidates can *depend on the subject*;
3. **`object_source`** — a named source (by `name` or `prefix`) whose `emits` matches `object_type`;
4. failing those, **any source whose `emits` matches `object_type`** (so declaring just `object_type` is often enough).

With no object argument, the **first** candidate is taken (mirroring how a subject defaults to the first item) — so narrow the pool deliberately:

```toml
object_type       = "application/vnd.cos-cli.workspace"
object_source     = "workspaces"
object_valid_when = '.metadata.output == "{subject.metadata.output}"'
# only workspaces on the same output as the app — {subject.*} is substituted
# into the predicate, then it's run as a jq filter over each candidate.
```

`object_valid_when` is the object-side analogue of `valid_when` (below): a jq predicate, evaluated per candidate (the candidate is `.`), with `{subject.*}` available. It prunes the pool before matching/first-pick.

**Adverb-routed verb** — the `cmd` is supplied by a selector adverb the verb opts into:

```toml
[[verbs]]
name = "critique"
accepts = ["text/*"]
uses_adverbs = ["via"]
fabric_pattern = "analyze_claims"   # available in templates as {verb.fabric_pattern}
prompt = """You are reviewing:

---
{subject.text}"""
```

The `via` adverb (defined in `claude-routing.toml`) selects which route runs the rendered prompt: `fabric`, `claude-desktop`, `claude-code`, or `clipboard`. The verb owns the prompt; the adverb owns the routing.

**Destructive verb** with a confirmation prompt:

```toml
[[verbs]]
name = "delete"
accepts = ["application/vnd.my-tool.thing"]
cmd = "my-tool delete {subject.id}"
confirm = true       # prompts y/N before executing
```

#### Default verb for a type

If a verb is the obvious default for items of a given type, declare it. `goo` uses this for implicit-default selection in the launcher and the CLI:

```toml
default_for = "application/vnd.cos-cli.app"
```

#### `valid_when` — applies only to *some* items of a type

`accepts` gates by MIME type; `valid_when` is an optional finer predicate — a **jq boolean expression** evaluated against the subject JSON. The verb is offered (and accepted by `verb_apply`) only when it's truthy. Absent ⇒ always applies.

```toml
[[verbs]]
name = "unzip"
accepts = ["inode/*"]
valid_when = '.text | test("[.](zip|tar|gz)$")'   # only archive-looking files
cmd = "..."
```

Evaluated in-process (jq is already loaded), so it's cheap to run while listing applicable verbs. It's the same kind of subject-JSON predicate that the `?params` source filter compiles into (see [cli-reference](cli-reference.md#subject-addressing)). For checks needing real I/O (a remote exists, a file's size), prefer keeping the verb broad and failing in the `cmd` itself — a heavier shell-predicate form is future work.

### Adverbs

Adverbs modify *how* a verb runs. Two flavors:

**Selector adverb** — picks among a known set of alternatives, each contributing its own template fragment:

```toml
[[adverbs]]
name = "via"
kind = "selector"
applies_to = ["text/*"]       # scope: any verb accepting these types
default = "clipboard"

[adverbs.values.clipboard]
description = "Copy assembled prompt to clipboard"
template = "wl-copy <<< {verb.prompt|q}"

[adverbs.values.fabric]
description = "Pipe through Fabric"
template = "fabric -p {verb.fabric_pattern} <<< {verb.prompt|q}"
```

**Convention**: selector values live at `[adverbs.values.NAME]` (attached to the most-recent `[[adverbs]]` entry). The dispatcher reads them as `adverbs[i].values.NAME.template`.

Selector values can also inject *template variables* that the verb's prompt template can use:

```toml
[[adverbs]]
name = "depth"
kind = "selector"
applies_to_verbs = ["think"]
default = "normal"

[adverbs.values.normal]
template_var = { depth_prefix = "Think carefully about" }

[adverbs.values.ultra]
template_var = { depth_prefix = "Ultrathink: exhaustively analyze every angle of" }
```

The verb's prompt can then write `"{depth_prefix} the following..."` and the dispatcher swaps in the chosen value's prefix.

**Fill adverb** — takes a free-form value, no enumerated alternatives:

```toml
[[adverbs]]
name = "name"
kind = "fill"
applies_to_verbs = ["rename", "create-scene"]
prompt = "New name:"
```

The user-supplied value is available in the verb template as `{adverbs.name}`.

#### Adverb scope

Every adverb declares scope via exactly one of:

| Field | Effect |
|---|---|
| `applies_to = ["text/*", ...]` | adverb is offered for any verb accepting these types |
| `applies_to_verbs = ["think", ...]` | adverb is offered only for these named verbs |

`goo validate` rejects adverbs that declare neither.

## Sigils

A sigil is a single leading character that expands into a canonical `goo://<domain>/<path>` address when you type a subject. The **built-in** sigils are fixed (and use only shell-unquoted characters):

- `:dom/path` → `goo://dom/path` — a **value** (the exact id in a domain)
- `:dom:query` → `goo://dom/;q=query` — a **search** (fuzzy id/title in a domain)
- `+text` → `goo://text/text` — force literal text (no inference)
- `^` / `^name` → `goo://clip/` — clipboard (built-in)

Bare input and native shapes (`./ ~/ /` → file, `scheme://` → url, else text) infer without a sigil. **Everything else is a customizable sigil**, declared with `[[sigils]]` in any plugin — the leading char is replaced by the expansion and re-canonicalized:

```toml
# ~/.config/cosmic-goo/plugins/my-sigils.toml
[[sigils]]
char = "@"
expands = "goo://app/"     # @firefox -> goo://app/firefox (a value)
description = "my apps"     # use ":app:" to expand to a search instead
```

`@` ships intentionally undefined — claim it for whatever domain you reach for most. User config wins over built-ins (later-loaded plugins override by `char`).

User config wins over built-ins (later-loaded plugins override by `char`). `goo validate` rejects sigils whose `char` is multi-character, missing an expansion, or collides with a reserved/native prefix (`:`, `+`, `.`, `/`, `~`, or any alphanumeric — those would break URL/path/text detection).

## Aliases

Where a sigil abbreviates a **subject**, an alias abbreviates a **whole
invocation** — a verb plus adverbs and/or a baked-in subject. Declare with
`[[aliases]]`:

```toml
[[aliases]]
name = "g"
expands = "search --engine=google"   # goo g "rust traits" -> goo search --engine=google "rust traits"
description = "Google web search"      # optional

[[aliases]]
name = "today"
expands = "append-to ~/journal.md"    # bake in the object; goo today "got the loader fast"
```

The alias's `expands` string is shell-tokenized (quotes and spaces honored) and
prepended at the verb position; your trailing arguments follow. Expansions are
re-dispatched, so an alias may chain to another alias — a depth guard breaks
cycles. Override is by `name` (later-loaded wins), like verbs.

`goo validate` rejects an alias with no `expands` or one whose `name` shadows a
subcommand (`list`, `describe`, `plugins`, `validate`, `compose`, `help`); it
*warns* — but allows — an alias that shares a verb's name, since deliberately
shadowing a verb is a valid use (the alias wins). Because `expands` is run with
the same trust as a verb's `cmd`, only define aliases in plugins you trust.

## Content dispatch

A `[[dispatch]]` rule classifies raw text by a regex and routes it to a verb —
a plumber-style "this content → that verb" table, used by `goo dispatch <input>`.
It's the content-aware layer on top of type-based `default_for`: rules are tried
in load order, the first whose `matches` hits wins, and if none match, dispatch
falls back to native detection + the type's default verb.

```toml
[[dispatch]]
matches = 'RFC:?[[:space:]]*([0-9]+)'    # ERE; capture groups -> ${1}, ${2}, …
type    = "text/x-uri"                    # type assigned to the resulting subject
set     = { text = "https://www.rfc-editor.org/rfc/rfc${1}.txt" }  # subject overrides
verb    = "open"                          # verb to route to
adverbs = { engine = "google" }          # optional adverb seed (omit if none)
```

`set` is deep-merged over a base `{ type, text: <input> }` subject, so you can
rewrite `text` or add `metadata = { line = "${2}" }`. `${0}` is the whole match,
`${1}…` the groups. Matching is **single-shot**: a rewritten subject is not
re-classified, so no cycles.

The regex is bash ERE — use POSIX classes, not Perl shorthands: `\s` →
`[[:space:]]`, `\d` → `[[:digit:]]`, `\w` → `[[:alnum:]_]`.

Rules are ordered, not keyed: within a plugin they fire in file order; **don't**
depend on ordering *across* plugins. No dispatch rules ship by default — copy
[`plugins/dispatch.toml.example`](https://github.com/teaguesterling/cosmic-goo/blob/main/plugins/dispatch.toml.example)
into your config and adapt it. `goo validate` requires each rule to have a
`matches` pattern and a `verb` that exists.

## Template substitution

The dispatcher substitutes `{path.to.var}` placeholders before running the command. Paths are dotted into a context dict containing:

| Top-level | What's in it |
|---|---|
| `subject` | the full subject JSON (`subject.type`, `subject.text`, `subject.id`, `subject.title`, `subject.metadata.*`, etc.) |
| `object` | the object JSON if the verb takes one; `null` otherwise |
| `verb` | the verb's TOML fields, with `verb.prompt` updated to the rendered version after subject substitution |
| `adverbs` | a dict of selected adverb values (`adverbs.via = "clipboard"`) |
| `cwd` | the current working directory |
| `<injected>` | any `template_var` from a selected selector adverb is spread at the top level (e.g. `{depth_prefix}`) |

### Rendering order

For verbs with a prompt and an adverb-supplied route:

1. Render `verb.prompt` with `{subject.*}` / `{object.*}` / `{adverbs.*}` / `{cwd}` / injected vars.
2. Re-inject the rendered prompt as `verb.prompt` in the context.
3. Render the chosen adverb's `template` (the *route*) with the now-populated context.
4. Execute via `bash -c`.

For verbs with a direct `cmd`, step 1 is skipped: substitute directly into `cmd` and execute.

### Filters: making substitutions safe

Append `|filter` to a placeholder to transform the value before it's inserted:

| Filter | Effect | Use when |
|---|---|---|
| `|q` (aliases `|sh`, `|shell`) | shell-quote via `printf %q` | the value is a bare argv token or `<<<` here-string body — immune to embedded quotes, newlines, `$(...)`, backticks |
| `|uri` (alias `|url`) | percent-encode via `jq @uri` | the value goes inside a URL query string |
| `|raw` (or no filter) | insert verbatim | numeric ids, URL prefixes, anything that must *not* be escaped |

The default (no filter) is **raw** — required for things like `cos-cli activate -i {subject.metadata.index}` (a bare number) and `{engine_url}` (a literal URL prefix). Reach for `|q` or `|uri` whenever the value is arbitrary user content.

```toml
# shell-quote arbitrary content as a here-string body — safe against any input
cmd = "wl-copy <<< {verb.prompt|q}"

# shell-quote as a single argv token
cmd = "notify-send 'goo' {subject.text|q}"

# percent-encode into a single-quoted URL (no inline jq dance needed)
template = "xdg-open 'claude://claude.ai/new?q={verb.prompt|uri}'"

# mix raw prefix + encoded query
cmd = "xdg-open '{engine_url}{subject.text|uri}'"
```

Without a filter, arbitrary content breaks the shell: a selection containing a single quote ends a `'...'` literal, and the rest gets parsed as commands. `|q` and `|uri` are the principled fix — prefer them over hand-rolled quoting.

## Validation

`goo validate` walks the registry and reports:

| Check | Why |
|---|---|
| verbs have non-empty `accepts` patterns | a verb that accepts nothing is unreachable |
| adverbs declare scope (`applies_to` or `applies_to_verbs`) | otherwise the dispatcher has no way to know when to offer the adverb |
| selector adverbs have a non-empty `values` table | a selector with no values has no routes to pick |

The checker is conservative — it doesn't yet verify that referenced types exist in the registry, or that command templates' `{var}` paths resolve. Both are planned.

## Discovery order recap

| Order | Path | Use for |
|---|---|---|
| 1 | `$COSMIC_GOO_BUILTIN_PLUGINS_DIR` (or `/usr/share/cosmic-goo/plugins`) | built-in plugins shipped with cosmic-goo |
| 2 | `/etc/cosmic-goo/plugins/` | system admin overrides |
| 3 | `$XDG_CONFIG_HOME/cosmic-goo/plugins/` (typically `~/.config/cosmic-goo/plugins/`) | your personal plugins |
| 4 | `$PWD/.cosmic-goo/plugins/` | project-scoped overrides |

Later wins. So you can override a built-in `text-verbs` plugin's `critique` verb by dropping your own `text-verbs.toml` (or a smaller plugin defining just `critique`) into `~/.config/cosmic-goo/plugins/`.

## Worked example: a new verb

Goal: a verb `uppercase-shout` that takes any text and pipes it through `tr`, with a `mode` adverb selecting between SCREAMING and Title Case.

```toml
# ~/.config/cosmic-goo/plugins/shout.toml
name = "shout"
description = "Loudness-themed text verbs"

[[verbs]]
name = "loud"
accepts = ["text/*"]
uses_adverbs = ["mode"]
cmd = "{tr_command} <<< {subject.text|q}"

[[adverbs]]
name = "mode"
kind = "selector"
applies_to_verbs = ["loud"]
default = "scream"

[adverbs.values.scream]
template_var = { tr_command = "tr a-z A-Z" }

[adverbs.values.title]
template_var = { tr_command = "sed -E 's/(\\w)(\\w*)/\\u\\1\\L\\2/g'" }
```

Test:

```
$ goo loud "hello world"
HELLO WORLD
$ goo loud "hello world" --mode=title
Hello World
```

`goo validate` should pass; if it errors, fix and re-run.
