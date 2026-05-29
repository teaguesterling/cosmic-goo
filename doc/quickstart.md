# goo quickstart

**goo** is a *grammar of operations* for your desktop: `goo <verb> <subject>`. One
sentence — a verb acting on a thing you address.

Why this isn't "a fancy way to run jq": **jq reads a file. goo operates on your
*desktop*** — a file, yes, but also a running app, a window, the clipboard, a URL,
your ssh hosts — with *one* grammar. Verbs adapt to whatever type the thing is (it
**coerces**), and the result lands **wherever you point it**. Four moves:

> **address → verb → coerce → route**

### Setup

goo is the Rust binary (a frozen bash reference, `bin/goo`, exists for conformance
but lacks the negotiation/coercion/routing features below — use the Rust `goo`).

- **Install:** `make install` puts `goo` on your `PATH` — see
  [`distribution.md`](distribution.md).
- **From a dev checkout:** `cargo build --release -p goo` (in `crates/`), then point
  the binary at the repo's plugins:
  `export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$PWD/plugins"`.

Every example below is real — copy-paste it.

---

## 1. Address anything (the noun)

A subject is a **reference**, not just a filename. Plain text and files work:

```
$ goo upper "hello world"
HELLO WORLD

$ goo sha256 "goo"
eeea394806ada30568999051…

$ echo "from a pipe" | goo upper
FROM A PIPE
```

…but the point is what *else* you can address — with short **sigils** for the
things you reach for, or the full `goo://` form:

| you type | is |
|---|---|
| `./report.md`, `~/notes.txt` | a file |
| `^` | the clipboard |
| `+literal text` | text, verbatim (no inference) |
| `:apps/firefox` | the Firefox app · `goo://apps/firefox` |
| `:ssh-hosts/prod`, `:tmux`, `:processes` | a source entity (17 sources ship) |

A relative file needs the leading `./` (or `~/`, or an absolute path): `goo
json-keys ./data.json`. A bare `data.json` is read as *literal text*, not a file.

```
$ goo upper ^                       # uppercase whatever's on the clipboard
$ goo activate firefox              # focus the app  (operate on a running app — jq can't)
$ goo list processes                # a source, as JSON
[{"id":"1","title":"systemd"}, …]
```

`goo describe <verb>` shows what a verb takes; `goo --help` lists the rest.

## 2. Types just work (coerce)

Ask for what you want; goo finds the path. `json-keys` wants JSON — hand it a CSV
and goo routes it through a converter first. `goo --explain` shows that route (it's
your debug lens — *what would happen, and why*):

```
$ goo --explain json-keys ./people.csv
subject: text/csv (via libmagic)
text/csv →[csv2json: cheap]→ application/json →(json-keys)→ text/plain
```

With the converter's tool (`mlr`) installed, it just runs:

```
$ goo json-keys ./people.csv
age
name
```

…and if that tool *isn't* installed, you get an **actionable hint**, not a cryptic
error:

```
$ goo json-keys ./people.csv
goo: 415 · no route — can't route text/csv through 'json-keys' — install: mlr
```

`--explain` also shows *how* goo typed the subject — `via libmagic` / `via
extension` / `via checker`.

## 3. Route the result anywhere (`--to` / `-o`)

By default the result prints to stdout (pipe it like any tool). Or send it
somewhere: **`-o <file>`** writes a file, **`--to ^`** puts it on the clipboard.

```
$ goo json-pretty ./data.json -o pretty.json    # format → a file
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

A plugin can declare verbs, types, sources, and adverbs — the whole grammar is
data. The 25 built-in plugins (~88 verbs) are just TOML shipped in the box. Full
reference: [`plugin-authoring.md`](plugin-authoring.md).

## 5. Instruments — *what performs* a verb (`--using`, preview)

A verb can be performed by different **instruments** — `goo <verb> X --using
<channel>` pins which one, and it composes with `--to` (instrument and destination
are orthogonal slots). The mechanism ships and is tested. But **no built-in verb
offers an instrument *choice* yet**, so there's nothing to demo it with on a stock
install — the headline instrument, **`fabric`** ("summarize this *via* the LLM
tool"), is still being decomposed out of the legacy `--via` adverb. So treat
`--using` as wired-and-waiting: real, but without its first shipped instrument.

---

## What actually works today (and what doesn't yet)

- **Coercion is tool-aware.** A route may need a converter tool (`mlr`, `chafa`, …);
  if it's missing you get an `install: X` hint, and `--explain` shows the route
  regardless of what's installed.
- **Extension-based typing is an opt-in power-up.** Point `COSMIC_GOO_MIME_DIRS` at
  the OS MIME database (e.g. `/usr/share/mime`) and goo types files by extension and
  knows the type lattice (an SVG is also text); without it, typing falls back to
  libmagic, which already handles most real files.
- **Not yet:** chat/contact destinations, the `fabric` instrument, and
  multi-recipient routing. See [limitations.md](limitations.md).

> **Stuck?** `goo --explain <verb> <subject>` shows what would happen and why — the
> type it inferred, how it inferred it, and the route (or a clear `415`).

Next: [`tutorial.md`](tutorial.md) walks the CLI in depth.
