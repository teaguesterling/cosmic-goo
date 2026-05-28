# Status — what's built vs designed

A one-screen index of the engine's models. The *how* lives in the design docs
linked below; this is just "we've built up through here."

## The type-system / negotiation arc

| Piece | State | Where |
|---|---|---|
| **Subtype lattice** — `is_subtype` (glob + structured-suffix + declared `is_a`), wired into all `accepts` matching | **built** (Rust) | `mime.rs`; [plugin-authoring](../plugin-authoring.md) |
| **Input inference** — `infer_for` (JSON-shape → type, weighted, re-ranked by the verb's `accepts`) | **built** (Rust) | `mime.rs` |
| **Negotiation planner** — `plan` (two-layer Dijkstra over a virtual converter graph; input coercion + output negotiation + `Using:` selection from one algorithm) | **built** (Rust) | `negotiation.rs`; [negotiation](negotiation.md) |
| **Converter schema** — `[[channels]]` → `converters_from_registry`, `validate_channels` | **built** (Rust) | `negotiation.rs`, `registry.rs` |
| **Accept derivation** — `target_from_env` (isatty/$WAYLAND_DISPLAY heuristic), `plan_request` | **built** (Rust) | `negotiation.rs` |
| **Simulator** — JS mirror of the planner, self-checked against Rust golden plans | **built** | `simulator/goo-simulator.html` |
| **Plan explainer** — `goo --explain VERB [@TYPE] [--as] [--explain-env]` (Accept profile + route / 415) | **built** (Rust) | `goo` bin; `tests/integration/explain.bats` |
| **Executor** — run a plan hop by hop (temp-file buffers; final step inherits stdout); present verbs wired (`goo view` renders) | **built** (Rust) | `exec.rs`; `execute.bats` |
| **Shipped converters + `view`** (`kind=present`; chafa/eog/cosmic-edit/xdg-open/csv2json; `application/json is_a text/plain`) | **built** | `presentation.toml`, `content.toml` |
| **Real-verb input coercion** — `goo VERB X` negotiates when X's type isn't accepted (`json-keys data.csv` → csv2json → json-keys); no gap → unchanged legacy path; no route → 415 | **built** (Rust) | `exec.rs`, `cmd_verb`; `execute.bats` |
| **Terminology** — reconciled to two: **`channel`** (the `{process}` resource) + **`Using:`** (the slot); "instrument" = the case-word; `via` = legacy (decomposes to `Using:`+`To:`). Verbs declare `usage = [<channel>…]` (the noun of the use-axis) | **built** (Rust + simulator) | goo-protocol §3 Terminology; `verb_edges` |
| **Multi-instrument execution** — a `usage` verb's chosen channel `cmd` runs at the verb step, in the verb's context (`{subject.*}`/`{verb.*}`); usage verbs always negotiate | **built** (Rust) | `exec.rs`, `verbs::render_template_in_context`; `execute.bats` |
| **Structural-inference gating** (#3) — a sniffed/inferred structured type wins only for a verb that *specifically* wants it (not a generic `text/*` verb) | **built** (Rust) | `mime::infer_for` |
| **Tool-aware planner** (#2) — a channel declares `tool`; the planner routes around uninstalled tools, or 415s with an actionable "install: X" hint; `--explain` is tool-agnostic | **built** (Rust) | `negotiation::plan_request`, `channel_tools` |
| **`--using` run-path override** (#1) — pin the verb's `usage` channel, overriding the planner's pick (a constraint, validated) | **built** (Rust) | `plan_request_using`, `cmd_verb`; `execute.bats` |
| **Type detection — registry-driven checkers + OS lattice + extension signal** (slices 1–4) — `[[detectors]]`/`[[checkers]]` collections (parity); the `json` check declared in an embedded `core.toml` (`builtin`, behavior-preserving) with `infer_for` now registry-driven; **OS-MIME-DB importer** (opt-in `COSMIC_GOO_MIME_DIRS`) pulls shared-mime-info `subclasses`→`is_a` (svg→xml→text into `is_subtype`) + `globs2`→`extensions` as `[[types]]`; **extension signal** — a file's extension → declared type, authoritative over libmagic (`resolve_file`, Rust-only; `None`-path == libmagic) | **built** (Rust) | `registry.rs`, `address.rs`, `mime.rs`; `mimedb.bats`, `extsignal.bats` |
| **Type detection — remaining** — the signal *model* (weighted candidates the verb's `accepts` selects; `emits` types the handle not content; no privileged hardwired types): `emits` wiring (terminal-vs-container), `--explain` signal annotation, the `cmd` runner (`input`/`ok`/`reads`), multi-candidate-for-files, importer production default | **designed** | [detection.md](detection.md) |
| **`--to`/`--on` resource destinations** — route to a named `goo://` resource (display/chat/clipboard); the run-path `From:` named-return-channel that lets `via` decompose | **designed** | [goo-protocol §12](goo-protocol.md); `claude-routing.toml` |
| **One-context channel substitution** (unify usage `{subject.*}` and coercion `{in.path}`) + **mode-aware buffering** (stream/bytes, not always temp-file) | **designed** | [negotiation §2.3](negotiation.md), [§5](negotiation.md) |
| **Output value model** — value (path/bytes/stream/ref/**live surface**) as first-class vs a marshalling-mode annotation | **designed** | [negotiation §2.1](negotiation.md) |

Bash is frozen at the **pre-negotiation** behavior and is the reference for
everything below the arc; the lattice/inference/negotiation are Rust-only, so
their bats tests skip on bash (the suite is 273/273 on both engines).

## The interface / protocol layer

| Piece | State | Where |
|---|---|---|
| `goo://` addressing — domains, value/search, sigils, `?refine` | **built** | [addressing-and-protocol](addressing-and-protocol.md) |
| **Declared shape-dispatch** — `infer` becomes a data-driven dispatcher; per-domain `shape.match` regexes + sigils-then-shape-then-`text` pipeline replace `canonicalize`'s hardwired rules; type via `emits` (retires `detect_content`'s `looks_like_uri`); deterministic pick (ties = load-time `validate` warning, not `300`); Rust-first, bash-parity-checked | **designed** | [addressing-and-protocol §shape-dispatch](addressing-and-protocol.md) |
| GOO default-verb dispatch (`goo <addr>` runs the type's `default_for`) | **built** | `dispatch.rs` |
| Content-dispatch `[[dispatch]]`, completion, filters, aliases | **built** | [plugin-authoring](../plugin-authoring.md) |
| Presentation negotiation — `Accept:`/`From:`, `--as`/`--to`/`--on`/`--using`, inherited-channel default | **designed** | [goo-protocol §12](goo-protocol.md) |
| Request/wire protocol — slots, OPTIONS, status codes; the `good` daemon | **designed** | [goo-protocol](goo-protocol.md) |
| `goo-compose-gui` (iced → libcosmic) | **scaffolded** | `crates/goo-compose-gui` |

## Surfaces

- **Engine + CLI** — the Rust `goo` is the default; bash is the reference (`make install` / `make install-bash`).
- **Plugins** — 25 (~88 verbs, 17 sources), incl. non-text handle domains and content-inspection verbs.
- **Tests** — bats conformance suite (273/273 both engines) + 145 engine unit tests.

See [limitations.md](../limitations.md) for the user-facing roadmap.
