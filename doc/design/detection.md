# Type detection — the signal ladder

How goo decides the **content type** a verb operates on. This is the *classify*
half of "the domain *resolves*, the MIME *classifies*"
([addressing-and-protocol.md](addressing-and-protocol.md)), feeding inference +
negotiation ([negotiation.md](negotiation.md), [goo-protocol §6](goo-protocol.md)).

Its sibling is the **shape** layer — routing a raw *token* to a domain (`./x`→file,
`https://x`→url) before any byte is read — which follows the same
*no-privileged-hardwired-types* principle via declared shape rules
([addressing-and-protocol §shape-dispatch](addressing-and-protocol.md)).
`detect_content`'s old `looks_like_uri → text/x-uri` sniff retires *there*: shape
routes URL-shaped tokens to `url` (which `emits text/x-uri`), leaving this layer to
type only resolved bytes.

> **Status: design.** This doesn't rewire the planner — it makes *uniform and
> explicit* what's already half-built. `infer_for(verb, content)` already returns
> weighted choices and #3 gating already discriminates specific-vs-generic; this
> doc puts every signal (extension, Content-Type, structural, libmagic, handle
> `emits`) into that one candidate model and corrects how `emits` is read.
> Detectors and checkers are **declared** in a shipped `core.toml` (`cmd` primary —
> *no privileged hardwired types*); the native `builtin` speed registry and HTTP
> fetch are deferred (last section).

## The model: "is this usable as what I need?", not "what is this?"

The instinct is to *classify* a subject — compute its one true type, then route.
That instinct breaks on real content. An SVG is, simultaneously and correctly,
`image/svg+xml`, `text/xml`, and `text/plain` — there is no single right answer to
"what is this." There *is* a right answer to **"is this an image I can render?"**
(yes) and **"is this text I can grep?"** (yes), asked by *different verbs*, both
true on the same bytes.

So detection produces **weighted candidates**, not a verdict, and the verb's
`accepts` *selects* among them. Two kinds answer the two questions:

- a **detector** *classifies* — "what is this?" → a mimetype, for *unambiguous*
  content. A PNG is just a PNG; libmagic answers and we're done.
- a **checker** *verifies* — "is this `<target>`?" → yes/no, for a *specific*
  target. This carries the contextual / multi-typed load: an `image/svg+xml`
  checker asked by `view`, a `text/plain` one asked by `grep` — both pass, the
  operation picks the question. A checker is **demand-driven**, so it's inherently
  gated (it runs only for a type a verb asked for) — which also *bounds* the cost
  of deep inspection (see below).

## The ladder

Each signal proposes a `(type, tier)` candidate:

```
signal              source                       tier      authoritative?
------              ------                       ----      --------------
explicit override   the user                     certain   yes   (flag spelling = flag-surface pass; currently @type)
handle `emits`      the resolver                 certain   handle only — a HINT for content (see below)
extension           the path (`.json`, `.csv`)   strong    yes
HTTP Content-Type   the response header          strong    yes   (deferred — no fetch yet)
checker (verify)    a checker proves the target  strong    no    (inferential; demand-driven; default tier)
detector (classify) a detector classifies bytes  medium    no    (inferential; libmagic; default tier)
```

`certain` / `strong` / `medium` (/`weak`, reserved for genuinely-fuzzy signals —
none today) are **discrete tiers** mapped to a weight in one place (like converter
cost `Tier`). A tier is a property of the signal's **nature** — *does a yes mean
yes?* — **not its implementation**: a checker's parse-success is `strong` whether
the parser is in-process `serde` or a shelled `jq -e .`. So there is no "`cmd`
tier"; `cmd` is an *impl* (below), and a `cmd`-backed checker lands at whatever
tier its signal warrants. Tiers are **defaulted per kind** — a checker is `strong`,
a detector `medium` — so authors rarely write one; override only the odd case. No
numeric confidence scale.

Cheap signals cover what structural parsing can't: **CSV and YAML are detected by
extension or libmagic — no parser.**

## `emits` types the handle, not the content

The correction that motivated this rewrite. A `[[sources]] emits` declaration is
authoritative — **about the handle**. For `apps`, the handle's value *is* its
type (`application/vnd.cos-cli.app`): terminal, nothing to refine. But for `files`
the handle is `inode/file`, and for a `database` column the handle is a
text/blob-valued cell — and **the bytes inside are a different question.** A
`TEXT` column can hold SVG, JSON, or CSV; the schema types the *column*, not the
*value*.

So `emits` is **content-authoritative only when it's terminal** (a specific,
opaque type). When it's a **container** type — `inode/file`, or a generic
`text/*`/`application/octet-stream` cell — it's a *hint*, and content detection
refines it. This is the same rule as "refinement fires when `emits` is generic,"
now stated with the container insight: a `TEXT` column is generic *with respect to
its bytes* even though `text/plain` looks like a real MIME. **No new schema** —
`emits` stays one declaration; this is purely how it's read for content questions.

## Authoritative vs inferential — and where gating applies

- **Authoritative** (the explicit override, *terminal* `emits`, extension,
  Content-Type) state ground truth — candidates **bypass gating**.
- **Inferential** (detectors like libmagic, and checkers) are
  *guessing* — candidates go through the **#3 gating rule**: a structured
  candidate wins only for a verb that accepts it *specifically* (a pattern that
  doesn't also subsume `text/plain`), never a generic `text/*` verb.

A `.json` file handed to a `text/*` verb is correctly `application/json`
(authoritative extension; `application/json is_a text/plain`, so the text verb
consumes it) — gating doesn't fire. A bare `{"k":1}` literal handed to a `text/*`
verb stays `text/plain` (inferential structural-parse, gated out). Same JSON,
different signal *nature*, correct both ways. (libmagic is *always* inferential —
even on a path it reads magic bytes; when an extension is also present, the
extension carries the authoritative load.)

## Multi-membership is only as real as the lattice declares

The SVG "it's both an image and text" claim does **not** fall out for free. It
works only if the registry encodes the relationships — either:

- detection produces **independent candidates** from different signals (the
  libmagic detector → `text/xml`, an `image/svg+xml` checker → yes), so a `text/*`
  verb matches the text candidate and `view` matches the image candidate; **or**
- the lattice carries the **subtype chain** (`image/svg+xml is_a … is_a
  text/plain`) via a declared `is_a` relation.

Neither is in the registry today (only `json → text/plain`; the `+xml` suffix rule
is same-top-level only — `image/svg+xml` does **not** reach `text/plain` without
an explicit declaration). So SVG is the *illustrative principle*; making goo
actually treat one as both requires those declarations. The doc's promise is the
**mechanism** (candidates + `accepts`-selection), not that any given type is
pre-wired multiply.

## Resolution — one procedure

Cheap signals run **eagerly**; checker probes run **lazily**, triggered by the
verb's `accepts`. (That's all "interleaving" means: cheap upfront, expensive
on-demand — not detection calling the planner or vice-versa.)

1. Gather authoritative candidates eagerly: explicit override, terminal handle
   `emits`, extension, Content-Type (and libmagic if content is already
   materialized).
2. For each pattern in the verb's `accepts`, if no authoritative candidate
   satisfies it, run the matching checker (guarded; #3-gated for non-specific
   patterns) — on yes, add its `(target, tier)` candidate.
3. Keep only candidates whose type satisfies `accepts` (subtype-aware via the
   lattice).
4. Among those, prefer by tier (`certain` → `strong` → `medium` → `weak`), then a
   signal-priority tiebreak.
5. Equal tier **and** equal specificity → `300` (ambiguous — "the file, or the
   literal?").

Detection's job **ends** at "here are the candidates the verb could use." When a
verb's `accepts` lists several patterns and the content matches more than one
(`image/*, text/*` on an SVG), detection does **not** break that tie — the
existing **planner's cost model** does (render-from-SVG-as-image vs -as-text →
cost picks). That keeps detection small and avoids a second routing brain.

**Materialization:** a checker needs the content in hand — for a file a
read, for a column the query you were already making to use the value, for a
stream it composes with the executor's buffer/peek. The cheap authoritative
signals (extension, Content-Type, `emits`) deliberately type *without* fetching;
introspection is the on-demand fallback, so "cheap first" is also "fetch-last."

## Detectors — classify content ("what is this?")

A **detector** classifies bytes → a mimetype, for *unambiguous* content. No
detector is privileged: the libmagic classification once baked into Rust is just a
declared entry in a shipped `core.toml`.

```toml
[[detectors]]
name = "libmagic"          # tier defaults to `medium`
cmd  = "file --mime-type -b"
```

## Checkers — verify content ("is this `<target>`?")

A **checker** verifies a *specific* target → yes/no, and on **yes** yields a
candidate `(target, tier)` (on no, nothing). It is the declared, reusable form of
the verb-level **`valid_when` jq** predicate already in the codebase. Demand-driven:
the engine runs the `application/json` checker only when a verb's `accepts` wants
JSON — which is exactly what makes it inherently #3-gated.

```toml
[[checkers]]
name   = "json"            # tier defaults to `strong`
target = "application/json"
guards = { peek = "{" }    # cheap skip — don't shell out unless bytes start with `{`
cmd    = "jq -e ."
```

A checker proves **arbitrarily specific** targets, including schema-bearing types —
`application/geo+json`, or an `application/vnd.foo+json` a plugin declares — by
making the predicate as specific as the type:

```toml
[[checkers]]
name   = "geojson"
target = "application/geo+json"
cmd    = "jq -e '.type==\"FeatureCollection\"'"
```

The target must be a real, lattice-carried type; there is **no inline-schema
field** — specificity lives in the target type + the predicate, nowhere else.

### Calling convention (`input` · `ok` · `reads`)

Detectors and checkers shell out the **same way the executor already runs pipeline
hops** — so they reuse its marshalling and add only *how to read the verdict*. Two
defaulted axes:

- **`input`** — how content reaches the cmd: `stdin` *(default — piped)*, or
  `path` (a file path: the real file if the subject is one, else the executor's
  existing temp-file path-marshalling), for tools that can't stream.
- the **verdict**, by kind:
  - checker → **`ok`**: `exit-zero` *(default)* · `stdout-nonempty` · `{ stdout = "ok" }` (trimmed stdout equals).
  - detector → **`reads`**: `stdout` *(default — trimmed stdout is the mimetype)*.

So the common cases need nothing but `cmd` (above); awkward tools override:

```toml
[[checkers]]                 # exits 0 always, prints a word → exit code is useless
name = "csv"
target = "text/csv"
cmd  = "csvclean --report"
ok   = { stdout = "ok" }

[[checkers]]                 # can't read stdin → wants a path
name  = "pdf"
target = "application/pdf"
input = "path"
cmd   = "pdfinfo {in.path}"
```

**These value sets are closed.** The engine implements each member (`stdin`/`path`;
`exit-zero`/`stdout-nonempty`/`stdout-equals`; `stdout`); a new member lands **only
when a real tool forces it** — never an `ok` expression language. `cmd` is a single
string split into argv by the executor's shell-words convention — no separate
`args`.

**Promotion to native.** An entry ships with `cmd`; if a benchmark later shows the
~5–10 ms fork matters, core (or a plugin) adds a `builtin = "…"` to the *same*
entry — same `target`, `tier`, `input`, just a faster impl, no rewrite. That native
registry **starts empty**. Today's `infer_candidates` JSON is the live example: it
ships as the `json` checker with `cmd = "jq -e ."`, and could later gain
`builtin = "serde-json"`.

## Mechanism — metadata readers vs content inspection

Two distinct things produce candidates, and only one of them inspects bytes:

- **Metadata readers** (extension, Content-Type, handle `emits`) — the engine
  *extracts* these from the resolved handle; their type **data** is already
  plugin-declared (`[[types]].extensions`, the response header, `[[sources]]
  emits`). Authoritative; trivial; not a `[[detectors]]`/`[[checkers]]` entry. What
  a domain/source contributes here is *signal availability* — a path exposes an
  extension, a response a Content-Type — not detection logic.
- **Content inspection** (`[[detectors]]` + `[[checkers]]`, above) — read bytes;
  inferential; declared, `cmd` (or native `builtin`). libmagic and the JSON check
  live here. They apply **generally**, gated by their guards — **not** copy-declared
  per domain.

`infer_candidates` (today's hardwired JSON-shape inference) ships as the `json`
`[[checkers]]` entry in `core.toml` — `cmd = "jq -e ."` (later optionally
`builtin = "serde-json"`), **not** Rust referenced by name. `--explain` annotates
each candidate with its signal: `application/geo+json (via geojson checker)`,
`image/png (via libmagic detector)`, `application/json (via .json extension)`.

## Deferred (with why)

- **HTTP Content-Type** — needs goo to fetch the URL; no fetch path yet.
- **The native `builtin` registry** — added only on *measured* need; the schema
  slot exists for forward-compat but ships empty (`cmd` everywhere first). (`cmd`
  detectors/checkers themselves are **not** deferred — they're primary.)
- **Coercion-reachable detection** — typing bare CSV so a JSON verb can consume it
  *via* `csv2json`. v1 probes only for *directly* wanted types.
- **Same-tier-and-specificity conflict → `300`** — v1 may pick higher-listed.
- **Context-demotion** (a `.json` extension whose libmagic disagrees *and* parse
  fails dropping below `strong`) — v1 keeps extension `strong`.
- **The flag surface** (the explicit-override spelling; `--as` in / `--to` out /
  `-o` file) — decided in a consolidated CLI-surface pass, not here. detection.md
  only asserts that the `certain` user-override *tier* exists.

## Build sequence

1. **This doc** — the contract.
2. Refactor `infer_candidates` → a **detector/checker registry loaded from plugin
   TOML**; the JSON check ships as the `json` `[[checkers]]` entry in `core.toml`
   (`cmd = "jq -e ."`, behavior-preserving), **not** as Rust referenced by name.
   `[[detectors]]`/`[[checkers]]` schema + the `cmd` runner (`input`/`ok`/`reads`).
3. **Extension-reader** — read `[[types]].extensions`; emit a `strong`,
   authoritative candidate (bypasses gating).
4. Wire **handle `emits`** into the same model as the `certain` candidate, with
   the terminal-vs-container read (generic `emits` leaves room to refine).
5. **`--explain`** annotates each candidate with its signal source.
