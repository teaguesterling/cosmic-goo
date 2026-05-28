# The negotiation engine

How goo turns *"`verb` this `subject`, deliver it `to` there, as what the caller
can take"* into a concrete pipeline — inserting type conversions where the pieces
don't line up, choosing the instrument that fits, and refusing cleanly when no
path exists.

This is the engine under [goo-protocol.md §12](goo-protocol.md) (the negotiation
*interface* — `Accept:`/`To:`/`Using:`, the CLI `--as`/`--to`/`--on`/`--using`)
and the concrete form of the coercion arc deferred in §13. It builds on the
**subtype lattice** (`is_subtype`) and **input inference** (`infer_for`) already
shipped, and on the **marshalling boundary + buffers** of §11.

> **Status: design.** Rust-engine only; the bash reference stays frozen at the
> pre-negotiation behavior, so negotiation bats tests skip on bash (as
> `infer.bats` / `goo-dispatch.bats` do). Built in slices (last section).

## 1. Framing — caps negotiation, not HTTP

HTTP is the right metaphor for the *interface*. For the *engine* the proven prior
art is **GStreamer's caps negotiation + auto-plugging** (cf. FFmpeg filtergraphs,
unit-conversion graphs):

- pads advertise **caps** (type sets) → our `accepts`/`emits`, with the **subtype
  lattice** as the subsumption relation;
- the pipeline **negotiates** a common format between adjacent elements;
- on a gap it **auto-plugs a converter** → our coercion channels;
- it resolves ties by **preference / cost**.

So the negotiation engine is **a typed auto-plugging pipeline planner**: weighted
shortest-path over a graph of converters, respecting the lattice. A solved class
of problem.

## 2. The model

### 2.1 Representation vs value — and what v1 simplifies

GStreamer separates **caps** (the static type description) from **buffers** (the
data unit that flows, with its own identity/timestamps/flags). Goo's analogs:

- **representation** — the MIME *type* (the nodes in the graph below);
- **value** — the actual thing carried: a file *path*, *bytes*, a *stream*, a
  `goo://buffer/<id>` *reference*, or a **live handle** (a window / Wayland
  `wl_surface` / X11 window — see §2.6).

A **live handle is not byte-materializable** — you can't `cat` a window — so
buffers (§5) don't apply to it; it's inherently by-reference, and normally
*terminal* in a route (a display consumes it; nothing flows onward). It's the
sharpest case of value ≠ representation: the *representation* is a surface type,
the *value* is a live compositor object.

**v1 models the representation as first-class and treats the value as a
side-annotation** (each transformer's *marshalling mode* — `stream|path|bytes` —
§2.3). That is a deliberate simplification, load-bearing-wrong at exactly two
future moments, documented here so v2 argues against a *known* gap, not a
silent one:

- **splits / joins** — `EMAIL a.pdf, b.pdf` or a `;all` fan-out has the *same
  representation* in two branches but two distinct *values*. With value as an
  annotation, that becomes a special case.
- **identity at the boundary** — a `goo://buffer/abc` is a value with an identity
  the engine must preserve across hops (§11's no-leak rule + GC). "Type +
  marshalling-mode" can't distinguish *same buffer, re-typed* from *different
  buffer, same type*, which the boundary needs to know to reap correctly.

When either lands, **value** becomes first-class (a `{repr, value}` pair flowing
the pipe). v1 doesn't need it; v1 must not contradict it.

### 2.2 Transformer

A `{process}` resource — and the *same* entity a `Using:` instrument is (used two
ways: auto-plugged on a gap, or pinned explicitly):

```
accepts  : [pattern]        # subtype-lattice pattern(s): what it consumes
emits    : type             # RULE: a CONCRETE type, never a pattern (§2.5)
cost     : tier             # free | cheap | normal | lossy | network (§4)
requires : [env-capability] # e.g. ["display"]; gates usability
consumes : stream|path|bytes # marshalling mode (drives buffer insertion, §5)
cmd      : "<template>"      # how it runs; {in.path|q}/{in.bytes}/stdin per §11
```

Example — `chafa`: `image/* → text/x-ansi`, `lossy`, `requires=[]`, `consumes=path`.

### 2.3 Verb

`accepts → emits`, carried out by one or more **channels** (a verb lists them in
`usage = [<channel>…]`; "instrument" is the case-word for the chosen one — see
goo-protocol §3 *Terminology*). The chosen channel fixes the actual `emits` and
owns the `cmd` (`fabric/inference` emits the result; `fabric/assemble` emits an
unrun prompt). A single-`cmd` verb has no `usage` list — it's carried out by its
own `cmd`. *Running* a chosen usage channel's `cmd` at the verb step
(multi-instrument execution) is deferred; the planner already selects among them.

**Presentation verbs are a kind, not a hack.** `view`/`play`/`open` have
`emits = accepts` (identity on type — the subject *is* the result) and **no
`cmd`**; all their work is the output route (negotiate the subject's
representation into the destination). They declare:

```toml
[[verbs]]
name = "view"
kind = "present"          # decided: no cmd; the verb edge is identity, work is delivery
accepts = ["image/*"]
default_for = "image/*"
```

The executor, seeing `kind = "present"`, runs no command — the verb edge is the
identity transition `A→B`, and delivery to the destination is the whole job.
(Alternative considered and rejected: full verbs with `cmd = ""`, which forces the
executor to special-case empty commands and tempts boilerplate `cmd = "cat …"`.)

### 2.4 Destination & Accept

The destination is a `goo://` resource with capabilities `{write|present|read|…}`
and an **Accept profile** (the types it can receive). Crucially, a `{present}`
target accepts a **surface**, not bytes (§2.6) — so a GUI app is just a converter
that emits one:

- a **file** `{write}`, Accept `*/*` — stores any bytes;
- a **display** `{present}`, Accept = **surface types** (`vnd.wayland.surface`,
  …); a **GUI viewer is a converter** into a surface — `eog: image/* → surface`,
  `mpv: video/* → surface`, `xdg-open: */* → surface` — each `requires:[display]`.
  So `xdg-open` is *literally one converter*, which is why `open` "is basically
  xdg-open" on a desktop and isn't on a tty (different Accept → different route).
- the **inherited terminal** `{present}` over a pty, Accept = `text/x-ansi`
  (chafa, `image/* → text/x-ansi`, is its converter);
- piped / redirected (non-tty) → `*/*` (a byte sink).

**Accept is preference-*ordered*, not a flat set** (the §12 "preferred
presentation," now operational). A cosmic-terminal session has *both* a pty and a
display, so its profile holds *both* `text/x-ansi` and `surface` — and ranks ansi
first (don't steal focus with a popped window). **The planner minimizes cost
*within* preference rank**: preference is the primary sort, cost the tiebreaker.
That's how the same machinery yields inline-in-a-terminal yet GUI-on-a-bare-desktop
without either being special-cased.

What *arriving* means is the destination's **capability**, not a separate slot:
`{write}` stores, `{present}` shows (goo-protocol §12 — `--to` vs `--on` are one
slot). `--as <type>` overrides the Accept; `--to`/`--on <resource>` overrides the
destination; both default per §12.

### 2.6 Surface types — live representations

`{present}` targets accept **surface types**: a small lattice family for live
compositor objects —

```
application/vnd.goo.surface              # umbrella
  ← application/vnd.wayland.surface      is_a goo.surface
  ← application/vnd.x11.window           is_a goo.surface
```

Their *value* is a live handle (§2.1), so they are **sink-only in practice**: they
appear as a `{present}` destination's Accept and as a GUI converter's `emits`,
never as an intermediate a further converter consumes. (A *capture* converter
`surface → image/png` — `grim` — is conceivable and would give them outgoing
edges; deferred, §7.) Delivery `surface → display{present}` is the compositor
mapping the window — a `free` terminal edge with no byte handoff, so the §11
marshalling boundary has nothing to deref or re-buffer here.

### 2.5 Schema rules (so the graph stays well-defined)

- **`emits` is a concrete type; `accepts` is a pattern.** A converter emitting a
  pattern (`text/*`) would leave Dijkstra without a concrete node to land on —
  rejected at load (`validate`).
- A transformer with `requires` unsatisfiable in the current environment is
  **pruned from the graph**, not failed at runtime.

## 3. The problem

Given a request `(verb, subject, using?, accept, destination)`, find a
minimum-cost **plan**:

```
subject ─[input coercions]→ verb.accepts ─[VERB·instrument]→ verb.emits ─[output coercions]→ t ∈ Accept ─[deliver via dest capability]
```

subject to: every hop lattice-checked (`emits ⊑ next.accepts`); `requires`
satisfied; buffers inserted where marshalling modes mismatch (§5); total cost
minimal (§4); final `t ∈ Accept`. No path → **`415`**; a material tie → **`300`**
(choose); the plan is **visible** (printable, like `EXPLAIN`).

## 4. The algorithm — two-layer Dijkstra

Double the graph by a "has the verb run yet?" bit:

- nodes `(type, A)` pre-verb and `(type, B)` post-verb;
- **converter edges** stay within a layer: converter `(pat→e)` + node `(t,L)`
  where `is_subtype(t, pat)` ⇒ edge `(t,L) → (e,L)`;
- **verb edges** cross `A→B`: instrument `(p_i → e_i)` + node `(t,A)` where
  `is_subtype(t, p_i)` ⇒ edge `(t,A) → (e_i,B)`. (`--using` pinned ⇒ only that
  instrument; `kind=present` ⇒ identity edge `(t,A) → (t,B)`.)
- **start** `(subject.type, A)`; **goal** any `(t,B)` with `t ∈ Accept`.

The node set is finite and tiny — `{subject.type} ∪ {converter emits} ∪ {verb
emits}` — and edges are computed by lattice-matching the current node's type
against each pattern. Dijkstra pops the first goal node → the minimum-cost plan;
reconstruct the path.

**Three behaviors fall out of one algorithm** — the test that the model is right:

1. **input coercion** = converter edges in layer A that reach a type the verb
   accepts (unlocking a verb edge). csv→json before a json verb: automatic.
2. **output negotiation** = converter edges in layer B reaching `Accept`.
   image→ansi for a tty: automatic. *Same algorithm, the other side of the verb
   edge.*
3. **`Using:` selection** = *which verb edge Dijkstra traverses.* Unpinned, every
   instrument is a candidate edge; the planner picks the one whose downstream
   route to `Accept` is cheapest. **"Accept drives `Using:`" (§12) is literally
   Dijkstra minimizing cost to a satisfiable Accept** — emergent, not special-cased.

## 5. Cost & materialization

**Cost is a named tier**, mapped to a numeric weight in one place at plan time —
authors declare *semantics*, not magic numbers:

| tier | when | rough weight |
|---|---|---|
| `free` | identity / no-op | 0 |
| `cheap` | lossless, local, fast (json pretty, base64) | low |
| `normal` | ordinary local transform | mid |
| `lossy` | fidelity loss (image→ansi, transcode-down) | high |
| `network` | remote round-trip | high + |

Minimum cost ⇒ **the most faithful representation the destination can accept**
(lossy is used only when nothing better is acceptable) — which is exactly content
negotiation's "best acceptable representation."

The tier is a *declared semantic*, deliberately coarse. Note a GUI launch
(`eog: image → surface`) is `normal`, **not `lossy`** — it's full-fidelity, just
heavy/async; "heaviness" is a different axis than "fidelity loss." That the single
tier collapses several real axes (fidelity, materialization, latency, heaviness)
is a known v1 simplification; preference order (§2.4) carries the weight a tier
can't, and a tuple-valued cost is a v2 refinement.

**Materialization is computed, not declared.** When a producer's marshalling mode
and the next consumer's don't line up (a `stream` producer into a `path`
consumer), the executor inserts a **buffer** (a temp file / memory, §11); the
planner adds a **materialization surcharge** so streaming/by-reference routes win
when they exist. Buffer insertion, deref-in / re-buffer-out at any foreign
boundary, and GC are the *executor's* job (§11's no-leak rule); the planner only
*accounts* for the cost.

### Executor v1 boundaries

The first executor (slice 4) runs a `Plan` hop by hop, with these pinned rules:

- **The initial value is the caller-supplied subject** (a path/bytes on disk).
  Buffering starts at the *first converter's output*, never the input — `view photo.png`
  passes the original path to the first converter, it doesn't temp-copy it first.
- **Intermediate steps capture stdout** (→ a temp-file buffer feeding the next
  hop); **the final step inherits stdout** — `chafa`'s ANSI goes straight to the
  inherited terminal (the "inherited channel is the default destination" rule),
  not through a buffer.
- **A present-verb identity step is elided explicitly** (type-in = type-out, no
  `cmd`). It is *not* skipped via "empty cmd" — a non-present step with an empty
  `cmd` is a **v1 error to surface**, not a silent no-op.
- v1 always buffers via temp files (correct, not minimal); mode-aware
  streaming/bytes is a later optimization.

## 6. Failure & visibility

- **`415`** — no path: report the gap (`can't present image/png as text/plain
  here`), the source and goal types, and what converters were considered.
- **`300`** — a material tie (two min-cost plans that differ): emit `{id,label,
  weight}` choices (goo-protocol §8); `;pick=first` takes rank #1.
- **`--explain`** — print the plan without running it, EXPLAIN-style. The *same*
  `goo view photo.png` plans three ways, by the destination's Accept profile —
  this is the whole model in one view:

  ```
  # bare tty            (Accept: text/x-ansi)
  view photo.png → image/png →[chafa: lossy]→ text/x-ansi → stdout{present}
  # cosmic-terminal     (Accept: text/x-ansi > surface; ansi preferred)
  view photo.png → image/png →[chafa: lossy]→ text/x-ansi → pty{present}
  # bare desktop        (Accept: surface)
  view photo.png → image/png →[eog: normal]→ vnd.wayland.surface → display{present}
  # redirected          (goo view photo.png > out.png — Accept: */*)
  view photo.png → image/png → file{write}                     (no converter)
  ```

  This is the §11 "visible in the planned route, not silent" requirement.

## 7. What v1 does *not* model (deferred, with why)

- **Type detection — the signal ladder** ([detection.md](detection.md)). The
  content type is *not* a single verdict — it's **weighted candidates** the verb's
  `accepts` selects among ("is this usable as what I need?", not "what is this?").
  Signals: explicit override / handle `emits` (certain) → extension / Content-Type
  / structural (strong) → libmagic (medium). The §3 gating rule applies only to
  *inferential* signals (libmagic/parse), not *authoritative* ones (extension);
  and **`emits` types the *handle*, not the content** — a `TEXT` column or
  `inode/file` is a *hint* the detectors refine. `infer_for`'s JSON-shape is the
  `json` checker under that model (detectors classify, checkers verify — both
  declared, `cmd` primary). HTTP Content-Type is deferred there.
- **value as first-class** (§2.1) — until splits/joins or buffer-identity force it.
- **surface as a *source*** (§2.6) — a capture converter (`grim: surface → image/png`)
  would give surface types outgoing edges (screenshot a window, then route it);
  v1 treats surfaces as sink-only.
- **dynamic emits / renegotiation** — v1 trusts declared `emits`; a handler whose
  *actual* output differs is a v2 re-negotiation-at-the-boundary concern.
- **full resource-Accept** — a display advertising its real renderable caps; v1
  uses the env-synthesized terminal/display heuristic (§2.4, §12).
- **the daemon wire form** — `Using:`/`To:`/`Accept:` as HTTP headers over a
  socket. The planner is a `goo-engine` library the **CLI drives today** (via the
  flags) and the `good` daemon drives later; no daemon needed to build this.

## 8. Build slices

Each slice is test-first, committed, and Rust-only (bash frozen; negotiation
tests skip on bash).

0. **This doc** — the model + contract.
1. **Planner** — `plan(subject_type, verb, accept, converters, reg) -> Option<Plan>`
   over an *in-memory* `Vec<Converter>` (no schema parsing, no exec). Two-layer
   Dijkstra, cost tiers, the three emergent behaviors. Engine unit tests with
   hand-rolled converter fixtures. *This is the heart; isolate it.*
2. **`[[channels]]` schema** — parse/validate the converter section (incl. the
   §2.5 rules); build the converter set from the registry. Parser tests.
3. **Accept derivation + flag wiring** — env-synthesized Accept (`isatty` /
   `$WAYLAND_DISPLAY`), `--as`/`--to`/`--on`/`--using`, destination→Accept; wire
   planner into resolution behind `--explain` (plan, don't run yet).
4. **Pipeline executor + marshalling** — run a `Plan` hop by hop; insert buffers on
   mode mismatch; deref/re-buffer at the boundary. `view photo.png` → chafa → ansi
   end-to-end. Rust-only bats.
5. **Real converters + presentation verbs** — ship the terminal converter
   (`chafa: image→ansi`) and the surface converters (`eog`/`mpv`/`xdg-open:
   * → surface, requires display`), plus `view`/`play`/`open` (`kind=present`);
   add data coercions (json↔csv, …) as consumers arise.
