# Type detection — the signal ladder

How goo decides a subject's MIME type. This is the **classify** half of "the
domain *resolves*, the MIME *classifies*" ([addressing-and-protocol.md](addressing-and-protocol.md)),
and the input-typing layer that feeds inference + negotiation
([negotiation.md](negotiation.md), [goo-protocol §6](goo-protocol.md)).

> **Status: design.** Supersedes the "sniffers" bullet in negotiation §7. v1 is
> built-in signal-readers + plugin-declared extension data; the heavier pieces
> (external-parser sniffers, HTTP fetch) are deferred (last section).

## The model: weighted candidates from cheap signals

Detection is **not** "parse the bytes to figure out what they are." It's reading
the **cheap signals that come *with* the subject** — each proposes a weighted
`(type, tier)` candidate — and letting the verb's `accepts` re-rank them. A
"sniffer" is just a **signal-reader**. Signals are wrong sometimes; *right enough*
mostly; and wrongness is absorbed by the re-ranking, by `@type` (override), and by
a `300` on a genuine tie.

### The ladder

```
signal              source                       tier      authoritative?
------              ------                       ----      --------------
@type               the user (`@image/png`)      certain   yes
[[sources]] emits   the resolver (apps→app type) certain   yes
extension           the path (`.json`, `.csv`)   strong    yes
HTTP Content-Type   the response header          strong    yes   (deferred — no fetch yet)
structural parse    `serde_json` parses          strong    n/a (proof)
libmagic            magic bytes (`file`)         medium    no
[future] cmd sniffer an external parser           weak      no    (deferred)
```

`certain` / `strong` / `medium` / `weak` are **discrete tiers**, mapped to a
weight in one place (like converter cost `Tier`). A tier is a property of the
**signal**, not the match — extension is `strong` because *extensions are
strong*, not because a particular `.json` matched. No numeric confidence scale.

The cheap signals cover what structural parsing can't: **CSV and YAML are
detected by extension (`.csv`) or libmagic — no parser** — which is why those
formats don't need the deferred external-parser tier.

## Authoritative vs inferential — and where gating applies

The single distinction that keeps detection honest:

- **Authoritative** signals (`@type`, source `emits`, extension, Content-Type)
  state ground truth — the user said so, the resolver said so, the filesystem/
  server said so. Their candidates **bypass gating**.
- **Inferential** signals (libmagic on bytes, structural parse, future heuristics)
  are *guessing*. Their candidates **go through the §3 gating rule**: a structured
  candidate wins only for a verb that accepts it *specifically* (a pattern that
  doesn't also accept `text/plain`), never a generic `text/*` verb.

This resolves the asymmetry: a `.json` file handed to a `text/*` verb is correctly
`application/json` (authoritative extension; `application/json is_a text/plain`,
so the text verb consumes it) — gating doesn't fire. But a bare `{"k":1}` literal
handed to a `text/*` verb stays `text/plain` (inferential structural-parse, gated
out). Same JSON, different signal *nature*, correct outcome both ways.

**libmagic is always inferential** — even on a file path it's reading magic bytes,
i.e. guessing. When the file also has an extension, *extension* carries the
authoritative load; libmagic is a corroborating (or conflicting) inferential
candidate. Don't make libmagic conditionally authoritative.

## Resolution

1. Gather candidates from every applicable signal.
2. Drop inferential candidates that fail gating for this verb.
3. Re-rank by the verb's `accepts` (subtype-aware), then by tier.
4. **Highest tier wins**; ties broken by signal order (extension > Content-Type >
   structural > libmagic). `@type` always wins (it's the override).
5. A genuine same-tier conflict between *authoritative* signals (extension says
   `text/csv`, a Content-Type says `application/json`) is a `300` — **deferred**;
   v1 takes the higher-listed signal and documents it. (Most "conflicts" aren't:
   extension `text/csv` + libmagic `text/plain` agree that it's text, libmagic
   just less specific.)

## Where sniffing adds value (refinement falls out)

A resolved entity already carries a `certain` type from its source's `emits`.
**Sniffing only adds value when that `emits` is *generic*** — `files` emits
`inode/file`, which extension/libmagic refine to `image/png` — or when there's
**no source at all** (bare content). So the earlier "bare content vs entity
refinement" scope question isn't a separate decision: refinement is simply "a
generic `emits` leaves room for a more-specific signal to win." A source emitting
a specific type (`apps` → `application/vnd.cos-cli.app`) is already `certain`;
nothing refines it.

## Mechanism

- **Signal *types* are built-in** (Rust): extension-reader, libmagic, structural-
  parse. libmagic and structural-parse are hardcoded (no reason to make libmagic
  pluggable).
- **Signal *data* is plugin-declared** where it makes sense: a `[[types]]` entry
  declares `extensions = [".json", ".jsonl"]`, feeding the extension-reader. No
  new section — extensions are data on the type the plugin already defines.
- `infer_candidates` (today's JSON-shape inference) **is** the structural-parse
  signal-reader under this model — refactored into it, not added alongside.
- `--explain` annotates each candidate with its signal: `application/json (via .json extension)`.

## Deferred (with why)

- **HTTP Content-Type** — needs goo to fetch the URL; no fetch path yet.
- **External-parser sniffers** (`[[sniffers]]` with a `cmd`, the `weak` tier) —
  for exotic types a built-in signal can't touch; earns its fork/exec only then.
- **Coercion-reachable sniffing** — typing bare CSV so a JSON verb can consume it
  *via* `csv2json`. v1 sniffs only for *directly* wanted types
  ([negotiation.md](negotiation.md) inference⨯coercion).
- **Same-tier authoritative conflict → `300`** — v1 picks higher-listed.
- **Context-demotion of a signal** (a `.json` extension whose libmagic disagrees
  *and* parse fails dropping below `strong`) — v1 keeps extension `strong`.

## Build sequence

1. **This doc** — the contract.
2. Refactor `infer_candidates` → a **tiered signal-reader registry**; existing
   JSON behavior preserved as the `structural-parse` signal.
3. **Extension-reader** — read `[[types]].extensions`; emit a `strong`,
   authoritative candidate (bypasses gating).
4. Wire **`[[sources]] emits`** as the `certain`-tier candidate in the same model
   (it's used already; make it consistent so `--explain`/`300` see it).
5. **`--explain`** annotates each candidate with its signal source.
