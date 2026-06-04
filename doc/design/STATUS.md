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
| **Earned-hops depth bounding** (§4.1) — auto-coercion is bounded: ≤1 converter hop per layer by default (a deeper route is *earned, not free*); `--hops N` raises input-coercion depth, `--force` lifts the bound; per-axis caps via `Hops`/`plan_bounded`. A 415 within budget **teaches**: re-searches deep and prints the deeper route + the exact flag (`--hops N` / `--force`) | **built** (Rust) | `negotiation::plan_bounded`, `Hops`; `deeper_route_hint` in `goo` bin; `hops.bats` |
| **Route enumeration** (§4.2) — `goo --explain <verb> <subj> --paths [--max-hops C] [--format text\|mermaid]` lists *all* routes A→B (cost-ranked via `pathfinding::yen` on a hopless node), drawn vertically (text) or as a merged `graph LR` DAG with shared nodes (mermaid) | **built** (Rust) | `negotiation::enumerate`; `render_paths_*` in `goo` bin; `explain.bats` |
| **Rich `--explain` rendering + detail modes** — the route line is colored on a TTY (cost by color; lossy/network marked; no inline `:cheap` noise), plain when piped; `--explain-with route\|steps\|shell` picks the detail view (adaptive default: runnable commands for a ≤2-hop route, annotated steps beyond) | **built** (Rust) | `render_route`/`render_steps`/`render_shell`, `use_color`; `explain.bats` |
| **`-c`/`--config` extra config** — merge an additional plugin TOML/dir last (highest precedence) for a single run, via `COSMIC_GOO_EXTRA_CONFIG` | **built** (Rust) | `registry::extra_config_files`; `config.bats` |
| **Polymorphic verbs across plugins** — registry's verb-merge accumulates by `(name, accepts)` instead of overriding by name alone; `verbs::lookup(name, type)` picks the most-specific impl (exact > lattice > glob), `for_subject` returns one verb per name (the best impl). One plugin can ship `connect` for ssh-hosts, another for bluez devices, both coexist; `goo connect :ssh/host` and `goo connect :bt/dev` dispatch the right one. **Rust-only** (bash reference stays at simple override-by-name, like negotiation). Side-effect: the pre-existing `switch` name-collision (tmux + workspaces both ship `switch`) is now resolved — both verbs survive the merge | **built** (Rust) | `registry::merge_verbs`, `verbs::{lookup, for_subject, verb_specificity}` |
| **Type detection — registry-driven checkers + OS lattice + extension signal + `--explain` provenance** (slices 1–5) — `[[detectors]]`/`[[checkers]]` collections (parity); the `json` check declared in an embedded `core.toml` (`builtin`, behavior-preserving) with `infer_for` now registry-driven; **OS-MIME-DB importer** (opt-in `COSMIC_GOO_MIME_DIRS`) pulls shared-mime-info `subclasses`→`is_a` (svg→xml→text into `is_subtype`) + `globs2`→`extensions` as `[[types]]`; **extension signal** — a file's extension → declared type, authoritative over libmagic (`resolve_file`, Rust-only; `None`-path == libmagic); **`--explain` `subject:` line** annotates which signal chose the type (explicit/extension/checker/libmagic/content), typed via the run's own path (fixes a 2a/4 divergence) | **built** (Rust) | `registry.rs`, `address.rs`, `mime.rs`, `goo` bin; `mimedb.bats`, `extsignal.bats`, `explain.bats` |
| **Type detection — remaining** — the signal *model* (weighted candidates the verb's `accepts` selects; `emits` types the handle not content; no privileged hardwired types): `emits` wiring (terminal-vs-container), the `cmd` runner (`input`/`ok`/`reads`), multi-candidate-for-files, the checker *name* in `--explain`, importer production default | **designed** | [detection.md](detection.md) |
| **`--to`/`-o` output routing (v1)** — the verb's result lands at a `{write}` destination instead of stdout: `--to <dest>`/`-o <file>`; v1 destinations **file + clipboard** (`address::write_to`, canonicalized via the addressing); `--to` ⇒ piped Accept (bytes, not a rendered surface); composes with `--using`/`--as`; no `--to` is byte-identical to stdout. Rust-only run-path | **built** (Rust) | `address.rs`, `selection.rs`, `goo` bin; `execute.bats` |
| **`--to`/`--on` — remaining** — `--on` `{present}` surfaces (same slot, target capability decides); chat/contacts/buffers, multi-recipient, lenient resolution, `Log:`/`From:`, type-matched `emits↔accepts`, the declared `{write}`-domain framework | **designed** | [goo-protocol §12](goo-protocol.md); `claude-routing.toml` |
| **One-context channel substitution** (unify usage `{subject.*}` and coercion `{in.path}`) + **mode-aware buffering** (stream/bytes, not always temp-file) | **designed** | [negotiation §2.3](negotiation.md), [§5](negotiation.md) |
| **Output value model** — value (path/bytes/stream/ref/**live surface**) as first-class vs a marshalling-mode annotation | **designed** | [negotiation §2.1](negotiation.md) |

**Engines.** Rust (`crates/goo`) is the **canonical** goo. Bash (`bin/goo` +
`lib/*.sh`) is a **legacy reference**, feature-frozen at pre-negotiation — kept
in-tree as a readable spec / no-rust fallback for the pre-negotiation subset,
but not the conformance gate anymore (`make test` runs the Rust engine; bash
runs are opt-in via `make test-bash`). New features land Rust-only and ~28% of
bats tests skip on bash by design. (The lattice / inference / negotiation /
OPTIONS / polymorphic-verb features are all Rust-only and accumulate as the
documented divergence.)

## The data-entry / sigil-less-inference arc

Making a bare token resolve to the right entity without sigils — confidence
bands, a picker, completion chips, and the confirm/cache safety rails around
them. Design: [data-entry-ux](data-entry-ux.md), [completion-polish](completion-polish.md).

| Piece | State | Where |
|---|---|---|
| **Completion-polish chips** (slices 1–3) — `[!]`/`[!!]` confirm/destructive chips + polymorphic `×N` chip in `__complete`; subjectless-verb announcement; GOO-default disambiguation + `goo what` (the chip vocabulary is single-sourced in completion-polish.md) | **built** (Rust + shell) | `goo` bin, `completions/goo.bash`; [completion-polish](completion-polish.md) |
| **Prefix-shape inference** (slice 4) — `app/firefox` ↦ `:app/firefox` (the cheap, deterministic inference prequel) | **built** (Rust) | `address::resolve` |
| **Entity-name inference — confidence bands** (slice 7) — a bare token is scored across enumerable sources and bucketed into **DEFINITIVE** (resolve silently) / **HIGH** (resolve + one-line nudge) / **MEDIUM** (numbered picker) / **LOW** (fall through to text); floors `EXACT 800 / HIGH 200 / MEDIUM 60`, a *relative* `2×`-dominance gate, and a `≤3`-count gate. Word-boundary scoring ratios against the matched **word/segment**, not the whole title (`cb1e2fc`), so a descriptive name (`api-gateway (production…)`) doesn't sink a whole-word match out of HIGH | **built** (Rust) | `inference.rs`; `goo` bin dispatch; §3.2; `inference.bats` |
| **Verb-aware bias** (slice 8) — `infer_entity_for_verb` narrows the scan to the sources the verb `accepts` before scoring, so the same token resolves differently per verb | **built** (Rust) | `inference::infer_entity_for_verb`, `resolve_subject`; §3.4 |
| **Subject-shape-aware listing** (slice 5) — `__complete verb-subject-items` ranks candidates by `accepts`-specificity + polymorphic-union (files first for `open`, network demoted) | **built** (Rust) | `verb-subject-items`; §5.1 |
| **Entity cache — watch/mtime invalidation, no-stale guarantee** — a source caches **only** if it declares `watch = [paths]`; an entry is valid iff `cmd` is unchanged AND every watch path's current mtime equals the value stat'd *before* list_cmd ran (false-STALE is safe; never false-fresh). `goo reload` / `clear_entity_cache()` is the manual drop. **Replaces the old TTL cache** (`cache_ttl` removed) | **built** (Rust) | `inference.rs`; `recent` plugin (`watch=[recently-used.xbel]`); `entity-cache.bats` |
| **Confirm UX** — friendly prompt (verb description + `[!]`/`[!!]` chip + subject label + a secondary `runs:` line, EOF cancels → 130); scoped `--confirm-dangerous=v1,v2` per-invocation pre-approval (flag-only by design — no env var — with a loud auto-approve note and a typo guard) | **built** (Rust) | `goo` bin; `tests/integration/confirm.bats` |
| **Implicit-subject preview** (§5.4 / #6, shell surface) — `goo <text-verb> <TAB>` with no subject shows a non-destructive stderr hint of what the implicit fallback *would* resolve to (`if Enter: '…'  (PRIMARY selection)`), so the user can avoid Enter when it'd grab the wrong thing. `__complete implicit-preview` peeks PRIMARY→clipboard with a 150ms `timeout` (a hung/absent compositor can't stall completion); `accepts_text` gates it (matches the run path). GUI caption deferred to #9 | **built** (Rust + shell) | `cmd_complete`, `selection::peek_*_timed`, `completions/goo.bash`; `implicit-subject.bats` |
| **Implicit-subject fallback nudge** — the run-time complement to the preview: when a text verb actually runs with **no** subject, the silent stdin→PRIMARY-selection→clipboard fallback is narrated on stderr (`no subject given — using the PRIMARY selection: '…'`), interactive/GUI-only and `GOO_INFER_NO_NUDGE`-suppressible; resolution is byte-identical in every context (transparency only) | **built** (Rust) | `resolve_subject`, `maybe_emit_implicit_nudge`; `implicit-subject.bats` |
| **Confirm gating — remaining** — destructive verbs reached via the **negotiation/coercion** path (`exec_negotiated`) are **not** gated; only the legacy render+exec path confirms (plain-cmd verbs, the common case, take the legacy path) | **designed** (known gap) | `exec.rs`; `45dc7ce` body |
| **No-watch warm caching — remaining** — command/dbus sources (apps, bluetooth, …) recompute every run on the one-shot CLI rather than risk staleness; true warm caching is a `good`-daemon job (inotify + dbus) | **designed** | daemon #31 |
| **Bands are calibrated, not proven** — the floors clear the *current* scoring-distribution gaps but aren't validated against a real corpus; treat band boundaries as tunable | **calibrated, not proven** | §3.2.2 |
| **Remaining roadmap** — #9 compose-GUI v2 noun-first flow (incl. the #6 GUI caption + #10 "speak it back"), #11 plugin-TOML JSON Schema, #13 "again"/recent-actions, #14 conversion-suggestions on 415, #15 `goo do <addr>` | **designed** | [data-entry-ux §8](data-entry-ux.md) |

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

- **Engine + CLI** — the Rust `goo` is the **canonical** engine (`make install`); the bash bin/goo is a **legacy reference** (`make install-bash`), feature-frozen pre-negotiation.
- **Plugins** — 25 (~88 verbs, 17 sources), incl. non-text handle domains and content-inspection verbs.
- **Tests** — bats conformance suite (391 tests; ~28% skip on bash by design, the Rust-only divergence) + 209 engine unit tests.

See [limitations.md](../limitations.md) for the user-facing roadmap.
