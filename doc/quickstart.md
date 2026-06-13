# goo quickstart

**goo** is a *grammar of operations* for your desktop: `goo <verb> <subject>`. One
sentence — a verb acting on a thing you address. (See **[The model](the-model.md)** for
the one-page idea.)

Why this isn't "a fancy way to run jq": **jq reads a file. goo operates on your
*desktop*** — a file, yes, but also a running app, a window, a git repo, a Bluetooth
device, your ssh hosts — with *one* grammar. Verbs adapt to whatever type the thing is
(it **coerces**), and the result lands **wherever you point it**. Four moves:

> **address → verb → coerce → route**

### Setup

goo is the Rust binary (a frozen bash reference, `bin/goo`, exists for conformance
but lacks the negotiation/coercion/routing features below — use the Rust `goo`).

- **Install:** `make install` puts `goo` on your `PATH` — see
  [`distribution.md`](distribution.md).
- **From a dev checkout:** `cargo build --release -p goo` (in `crates/`), then point
  the binary at the repo's plugins:
  `export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$PWD/plugins"`.

---

## 1. Address anything (the noun)

A subject is a **reference** to any desktop thing — an app, a window, a repo, a device,
a file, the clipboard — not just a string. You name it with a short **sigil** for the
domain, or the full `goo://` form:

| you type | is |
|---|---|
| `:app/firefox` | the Firefox app · `goo://app/firefox` |
| `:repo:goo` | a git repo (fuzzy-matched by name) |
| `:ws/0:1` · `:win/firefox/2` | a workspace · a specific window |
| `:bt:head` · `:ssh/prod` · `:svc/woollama` | a device · an ssh host · a service |
| `./report.md` · `~/notes.txt` · `https://…` | a file · a URL (native shapes — no sigil) |
| `^` · `+literal` | the clipboard · text, verbatim (no inference) |

`:dom/id` is an **exact** id; `:dom:query` **fuzzy-searches** the domain. **21 sources**
ship (apps, windows, workspaces, repos, branches, files, mounts, bluetooth, network, ssh,
services, containers, audio sinks, tmux, processes, clipboard-history, emoji, …) — see
[The model](the-model.md) for the full set, or `goo plugins` for what's loaded.

So you operate on *things*, not just bytes:

```
$ goo activate :app/firefox         # focus a running app   (jq can't do this)
$ goo switch :ws/0:1                 # switch workspace
$ goo status :repo:cosmic-goo        # short git status of a repo
$ goo do :app/firefox                # noun-first: name the thing, then pick a verb
```

Plain text and files are just other subject types — they work the same way:

```
$ goo upper "hello world"            # → HELLO WORLD
$ echo "from a pipe" | goo upper     # text from stdin
$ goo json-keys data.json            # a bare existing file resolves as that file
```

Force literal text with `+`: `goo upper +data.json` uppercases the *string*
`data.json`, not the file. `goo describe <verb>` shows what a verb takes; `goo what
<subject>` lists the verbs that apply to a thing.

## 2. Types just work (coerce)

Ask for what you want; goo finds the path. `json-keys` wants JSON — hand it a CSV
and goo routes it through a converter first. `goo --explain` shows that route (it's
your debug lens — *what would happen, and why*):

```
$ goo --explain json-keys people.csv
subject: text/csv (via libmagic)
text/csv →[csv2json: cheap]→ application/json →(json-keys)→ text/plain
```

With the converter's tool (`mlr`) installed, it just runs:

```
$ goo json-keys people.csv
age
name
```

…and if that tool *isn't* installed, you get an **actionable hint**, not a cryptic
error:

```
$ goo json-keys people.csv
goo: 415 · no route — can't route text/csv through 'json-keys' — install: mlr
```

`--explain` also shows *how* goo typed the subject — `via libmagic` / `via
extension` / `via checker`. The same coercion is why a `view` verb that wants an
image still works on a screenshot, and a verb that 415s suggests alternatives that
*do* accept your subject's type.

## 3. Route the result anywhere (`--to` / `-o`)

By default the result prints to stdout (pipe it like any tool). Or send it
somewhere: **`-o <file>`** writes a file, **`--to ^`** puts it on the clipboard.

```
$ goo json-pretty data.json -o pretty.json      # format → a file
$ goo upper "ship it" --to ^                    # → the clipboard
```

The destination is orthogonal to what produced the bytes — `--to` composes with the
verb and its options.

## 4. Extend it — your own verb (custom verbs)

Verbs are just TOML. Drop a file in `~/.config/cosmic-goo/plugins/` and it's live —
no rebuild:

```toml
# ~/.config/cosmic-goo/plugins/mine.toml
[[verbs]]
name    = "shout"
accepts = ["text/*"]
cmd     = "tr a-z A-Z <<< {subject.text|q}"
```

```
$ goo shout "hello from my plugin"
HELLO FROM MY PLUGIN

$ goo shout "and route it" -o out.txt          # your verb gets coercion + routing for free
```

A plugin can declare verbs, types, **sources** (new kinds of addressable thing), and
adverbs — the whole grammar is data. The **~30 built-in plugins (~92 verbs, 21
sources)** are just TOML shipped in the box. Full reference:
[`plugin-authoring.md`](plugin-authoring.md).

## 5. Instruments — *what performs* a verb (`--using`, preview)

A verb can be performed by different **instruments** — `goo <verb> X --using
<channel>` pins which one, and it composes with `--to` (instrument and destination
are orthogonal slots). The mechanism ships and is tested. But **no built-in verb
offers an instrument *choice* yet**, so there's nothing to demo it with on a stock
install — the headline instrument, **`woollama`** ("summarize this *via* the LLM
router"), still rides the legacy `--via` adverb rather than the `--using` slot it
will eventually decompose into. So treat `--using` as wired-and-waiting: real, but
without its first shipped instrument.

---

## What actually works today (and what doesn't yet)

- **Coercion is tool-aware.** A route may need a converter tool (`mlr`, `chafa`, …);
  if it's missing you get an `install: X` hint, and `--explain` shows the route
  regardless of what's installed.
- **Noun-first + history.** `goo do <subject>` names the thing first; `goo again`
  repeats your last verb+adverbs on a new subject; the `goo-compose-gui` launcher
  builds the same sentence with type-to-filter panes.
- **Extension-based typing is an opt-in power-up.** Point `COSMIC_GOO_MIME_DIRS` at
  the OS MIME database (e.g. `/usr/share/mime`) and goo types files by extension and
  knows the type lattice (an SVG is also text); without it, typing falls back to
  libmagic, which already handles most real files.
- **Not yet:** chat/contact destinations, the `woollama` *instrument* slot (the
  route ships as a `--via` adverb; the `--using` decomposition is pending), and
  multi-recipient routing. See [limitations.md](limitations.md).

> **Stuck?** `goo --explain <verb> <subject>` shows what would happen and why — the
> type it inferred, how it inferred it, and the route (or a clear `415`).

Next: [`tutorial.md`](tutorial.md) walks the CLI in depth.
