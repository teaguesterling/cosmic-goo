# Providers — a starter collection of dynamic verb providers

A `[[providers]]` entry turns an external command registry into goo verbs on a
subject. At verb-listing time goo runs the provider's `list_cmd`, and each
emitted `{name, description}` becomes a verb on subjects whose type matches
`for_type`. See [`doc/design/dynamic-verb-providers.md`](../doc/design/dynamic-verb-providers.md)
for the full design.

These are **examples first** — runnable, real, and meant to be read as templates
for your own providers as much as used directly. None are installed by default;
load one with `-c`:

```sh
goo -c providers/core-linux/make-targets.toml do :cwd          # make targets as verbs
goo -c providers/duckdb/column-profile.toml do ./data.csv      # a CSV's columns as verbs
goo -c providers/                                              # merge the whole dir
```

## Organized by dependency tier

Like the plugin tiers (`core`/`desktop`/`cosmic`), providers are grouped by the
**tool they require** — the gate is the reason to keep them out of the dep-free
core. Install only the tiers whose tools you have.

| tier | needs | provider | subject | shape |
|---|---|---|---|---|
| `core-linux` | `make` | `make-targets` | `:cwd` | ambient |
| `dev` | `just` | `just-recipes` | `:cwd` | ambient |
| `duckdb` | `duckdb` | `column-profile` | a CSV file | per-subject |
| `git` | `git` | `branch-log` | a git repo (`:repo`) | per-subject |
| `xdg` | `xdg-mime`, `gtk-launch` | `open-with` | any file (`inode/file`) | per-subject |

A provider whose tool is absent, or whose `list_cmd` errors / emits non-JSON,
yields **no verbs** — it never breaks the listing. An uninstalled tool is a
no-op, not an error.

## Two shapes: ambient vs per-subject

- **Ambient (`:cwd`).** `list_cmd` runs in your working directory and reads
  ambient state — `make-targets` parses `./Makefile`, `just-recipes` reads
  `./justfile`. No `{subject.*}` needed; the directory *is* the context.
- **Per-subject.** `list_cmd` is subject-substituted (`{subject.metadata.path|q}`),
  so the verb list depends on the *specific* subject — `column-profile` reads
  *this* CSV's columns, `branch-log` lists *this* repo's branches, `open-with`
  enumerates *this* file's MIME handlers. Different subject → different verbs. (This
  is the `list_cmd`-takes-the-subject capability; before it, providers could only key
  off the subject's *type*.)

> **`for_type = inode/file` matches any file.** A file subject carries an
> `inode/file` membership alongside its content type (a file is *both* a handle and
> a datum — see `address::resolve_file` / `verbs::subject_types`), so a provider
> keyed on `inode/file` attaches to every file regardless of its MIME. That's what
> lets `xdg/open-with` work across `.pdf`/`.png`/`.csv` alike.

## Two rules these examples exist to teach

1. **Quote untrusted subject fields with `|q`.** A subject field (a filename, a
   repo path) reaches `bash -c`, and can carry shell metacharacters.
   `{subject.metadata.path|q}` shell-quotes it — the same convention as everywhere
   a subject reaches a shell (`object_list_cmd`, a verb's `cmd`). A provider that
   interpolates a subject field *without* `|q` is a shell-injection bug.
   - **Nested quoting.** `|q` produces a POSIX *single-quoted* word — safe as a
     standalone argument, but it breaks if you embed it *inside* another quoted
     string (e.g. inside `duckdb -c "… '{path}' …"` when a filename contains a
     double quote). `column-profile` sidesteps this by passing the path through
     the **environment** (`GOO_CSV`) and reading it with DuckDB's `getenv()`,
     never interpolating it into the SQL text.
   - **Verb names are safe by construction.** A verb *name* is a validated
     identifier (no spaces/quotes/`;`), so `{verb.name}` in the `run` command —
     even dropped into SQL as a quoted identifier — can't carry an injection.
   - **jq gotcha.** In a `list_cmd` piped to `jq`, use **explicit** object
     construction `{name: .name}`, not jq's shorthand `{name}` — `{name}` collides
     with goo's `{placeholder}` grammar and renders empty.

2. **Keep `for_type` as specific as the provider allows — cost is real.** Every
   subject whose type matches pays one `list_cmd` exec when its verbs are listed,
   every time (goo is one-shot; there is **no cross-invocation cache yet**). The
   bounded examples (`text/csv`, `application/vnd.git.repo`) pay only on their own
   subjects; `xdg/open-with` is deliberately broad (`for_type = inode/file` matches
   *every* file) because "open with the right app" inherently applies to all files —
   so it forks `xdg-mime` on every file listing. Don't reach for `inode/file` unless
   your provider genuinely applies to everything.

## Known gaps (honest, not hidden)

- **No memoization.** A broad `for_type` (like `xdg/open-with`), or several
  providers on one type, fan out serially on every listing. The design doc defers
  the cache "until a hot double-call shows up" — this collection (and especially the
  broad xdg provider) is that forcing function.
- **DuckDB *database* files** (`.duckdb`/`.sqlite`) are `application/octet-stream`
  to libmagic, so "tables of a database file as verbs" needs an extension-based
  type declaration first. `column-profile` targets `text/csv` (cleanly typed)
  instead.
- **Non-identifier names drop.** A make target / CSV column / branch whose name
  isn't a shell-neutral identifier (e.g. a column `"first name"`) is silently
  skipped — it can't safely become a verb token.
