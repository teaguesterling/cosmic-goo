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
| **Multi-instrument execution** (`Using:` channels per verb — no per-instrument template in the schema yet) | **designed** (surfaced exec error) | [negotiation §3](negotiation.md) |
| **`--to`/`--on` destination override on the run path; mode-aware (non-temp-file) buffering** | **designed** | [negotiation §5](negotiation.md) |
| **Coercion as built** (auto-route on a type gap; the planner *plans* it, nothing *runs* it yet) | **designed** | [negotiation](negotiation.md), [goo-protocol §13](goo-protocol.md) |
| **Output value model** — value (path/bytes/stream/ref/**live surface**) as first-class vs a marshalling-mode annotation | **designed** | [negotiation §2.1](negotiation.md) |

Bash is frozen at the **pre-negotiation** behavior and is the reference for
everything below the arc; the lattice/inference/negotiation are Rust-only, so
their bats tests skip on bash (the suite is 250/250 on both engines).

## The interface / protocol layer

| Piece | State | Where |
|---|---|---|
| `goo://` addressing — domains, value/search, sigils, `?refine` | **built** | [addressing-and-protocol](addressing-and-protocol.md) |
| GOO default-verb dispatch (`goo <addr>` runs the type's `default_for`) | **built** | `dispatch.rs` |
| Content-dispatch `[[dispatch]]`, completion, filters, aliases | **built** | [plugin-authoring](../plugin-authoring.md) |
| Presentation negotiation — `Accept:`/`From:`, `--as`/`--to`/`--on`/`--using`, inherited-channel default | **designed** | [goo-protocol §12](goo-protocol.md) |
| Request/wire protocol — slots, OPTIONS, status codes; the `good` daemon | **designed** | [goo-protocol](goo-protocol.md) |
| `goo-compose-gui` (iced → libcosmic) | **scaffolded** | `crates/goo-compose-gui` |

## Surfaces

- **Engine + CLI** — the Rust `goo` is the default; bash is the reference (`make install` / `make install-bash`).
- **Plugins** — 25 (~88 verbs, 17 sources), incl. non-text handle domains and content-inspection verbs.
- **Tests** — bats conformance suite (250/250 both engines) + ~118 engine unit tests.

See [limitations.md](../limitations.md) for the user-facing roadmap.
