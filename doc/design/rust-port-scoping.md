# Rust port — scoping

> Status: design pass, no code yet. Companion to the crate sketch in
> [prior-art-and-architecture.md](prior-art-and-architecture.md#the-rust-implementation-sketch).
> This document pins down what the bash reference leaves **underspecified** —
> the decisions the port must make that the shell made implicitly.

The bash engine is the executable spec. Rust replaces the **engine**, not the
**plugins**: plugins stay TOML + shell, and Rust assembles and `exec`s the
rendered command exactly as `bash -c` does today. The conformance contract is
the bats suite (227 tests) — a Rust `goo` that passes it against the same plugin
set is provably equivalent.

## 1. Crate split (confirmed)

| Crate | Role | Maps to bash |
|---|---|---|
| `goo-engine` (lib) | registry, types, addressing, templating, dispatch, verb resolution | `lib/*.sh` |
| `goo` (bin) | thin CLI: argv → request → in-process engine (or socket) → `exec` | `bin/goo` |
| `good` (bin) | warm daemon: UNIX-socket server, inotify reload, async verbs | (none yet; #31) |
| `goo-compose` (bin) | libcosmic/iced three-pane dialog | `lib/dialog.sh` + `cmd_compose` |

`goo-engine` module breakdown (1:1 with the shell libs):

| Module | bash source | Notes |
|---|---|---|
| `registry` | `plugin-loader.sh` | TOML load + merge-by-name (sigils by char, dispatch concatenated) + mtime cache |
| `mime` | `types.sh` | glob-style `text/*` matcher |
| `address` | `address.sh` | canonicalize + resolve; the `Address` enum is the IPC wire type |
| `template` | `verbs.sh::_substitute` | `{path|filter}` rendering |
| `verbs` | `verbs.sh` | lookup / default_for / for_subject / apply / `valid_when` |
| `dispatch` | `dispatch.sh` | plumber rule table |
| `adverbs` | `verbs.sh` (adverb resolution) | selector/flag → template_var + route |

## 2. The jq question — much smaller than it looks

There are three *kinds* of jq in the codebase. Only one is the engine's problem.

1. **jq inside plugin shell commands** (`list_cmd`, `object_list_cmd`, `cmd`,
   adverb `template` — e.g. `cos-cli info --json | jq '[.apps[] | …]'`). This
   runs in the user's shell. Rust shells out to `bash -c` (which calls the
   system `jq`) exactly as today. **No engine jq needed.**
2. **Internal jq the engine uses to manipulate its own JSON** (building the
   registry, matching object candidates by id/title substring, applying
   `?params`). These have *simple, fixed semantics* and become plain Rust over
   `serde_json::Value` — no expression evaluation. `?params` = per-key
   case-insensitive substring; object match = `id`/`title` `contains`.
3. **User-authored jq the engine must evaluate** — exactly one surface:
   **`valid_when`** (`verbs.sh:99`), a boolean jq expression over the subject
   JSON. (Plus the future `valid_when_cmd` escape hatch, which is shell, not jq.)

So the engine needs a jq *evaluator* only for `valid_when`. **Recommendation:
embed [`jaq`](https://crates.io/crates/jaq)** (pure-Rust jq, no subprocess) for
that one surface. This keeps `valid_when` expressions byte-compatible with the
bash version so the bats tests pass unchanged. Do **not** swap `valid_when` to
rhai/another DSL in the first port — that would break conformance; revisit as a
post-parity enhancement (the architecture doc already flags rhai as the Rust-era
answer for the *escape hatch*, not the primary form).

**Parity floor (the spike's checklist).** Shipped plugins use *zero*
`valid_when` today; the only expression in the suite is the verbs fixture's
`.text | endswith(".zip")`. So the concrete constructs the spike must confirm
`jaq` supports, byte-for-byte against `jq`:

- `|` pipe, `//` alternative, `select(...)`
- `endswith` / `startswith`
- `test("regex")` (the architecture doc's `valid_when` example uses it)
- `contains`, `ascii_downcase` (used by the internal object-match jq, which we
  *won't* hand to jaq — but if a `valid_when` author reaches for them, they must
  work)
- field/index access `.a.b`

Run the spike: feed each construct a sample subject through both `jq` and
`jaq`, assert identical boolean output. If any is missing or diverges, fall back
to shelling `jq` for `valid_when` only (correctness over the no-subprocess win).

## 3. Template substitution `{path|filter}`

`_substitute` is now a hand-written scanner (handles literal `{`, supports
`{a.b.c}` dotted paths and `|q`/`|uri`/`|raw` filters). The Rust equivalent:

- **path lookup** `subject.metadata.index` → walk `serde_json::Value` by dotted
  key. Pin the semantics so Rust doesn't silently diverge from jq: **object keys
  only — no numeric array indexing** in template paths (the bash tests never use
  it; document as a v1 limitation), and a missing key *or* a null intermediate
  short-circuits to **empty string** (matches jq `.a.b.c // empty`).
- **`|q`** → the [`shell-quote`](https://crates.io/crates/shell-quote) or
  `shell-escape` crate, matching `printf %q` semantics (verify the edge cases
  the hostile-content test exercises: quotes, newlines, `$()`, backticks).
- **`|uri`** → [`percent-encoding`](https://crates.io/crates/percent-encoding)
  with a component-safe set, matching `jq @uri`.
- literal-`{` handling and the single-pass left-to-right scan port directly.

This is the most behaviorally fiddly module; the `_substitute` bats cases are
the spec. **`printf %q` ↔ Rust shell-quote parity is the thing most likely to
diverge** — pin it with the existing hostile-content fixtures first.

## 4. Exit-code surface (formalize)

Bash uses three today (from `cli-reference.md`): `0` success, `1`
usage/unknown/resolution/validation/route failure, `130` user cancelled a
`confirm` prompt. The port should make this an explicit enum so it's a stable
contract, not an accident of `die`:

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | generic failure (usage, unknown verb/source/subject, resolution, route) |
| 130 | user cancelled (a `confirm` prompt **or** an empty compose pick — both asserted by the suite) |

Open question to decide *before* coding: do we **split `1`** into finer codes
(e.g. `2` usage vs. `3` not-found vs. `4` validation) for scriptability? The
bats suite only asserts `0` / non-zero / `130`, so finer codes are
**conformance-neutral** — safe to add, but commit to the mapping up front and
document it. Recommendation: keep `1` as the catch-all for parity, reserve
`2`–`9` as a documented future-use range, don't subdivide in the first port.

## 5. On-disk registry / cache format

Bash writes the merged registry as JSON to
`$XDG_RUNTIME_DIR/cosmic-goo/registry.json` (atomic temp+rename; mtime-fresh
check against plugin dirs/files). **Keep JSON as the on-disk format**, even in
Rust:

- it's the daemon↔client and bash↔Rust interchange format during the migration
  (a Rust `good` and a bash `goo` can share one cache);
- it's debuggable (`jq . registry.json`);
- the parse cost is paid once and cached.

Internally Rust holds a typed `Registry` struct; the cache is its
`serde_json` serialization. The freshness check (newest plugin-dir/file mtime ≥
cache mtime) ports verbatim. A faster binary cache (bincode) is a *later*
optimization, gated behind keeping the JSON path for interop.

**Add a `"schema_version": 1` field to the cache** so a bash↔Rust schema skew
during migration is a cache miss (full reload), not a silent misparse. Cheap
insurance: bash writes it in `plugin_load_all` (one line) and the freshness
check treats a missing/mismatched version as stale; Rust requires it. Land this
in the bash side when the first Rust reader appears, so they never share an
unversioned cache.

## 6. Shell execution & trust model (unchanged)

Verbs (`cmd`), adverb routes (`template`), aliases (`expands`), and dispatch
targets all execute rendered shell via `bash -c`. Rust keeps this:
`std::process::Command::new("bash").arg("-c").arg(rendered)`. The trust model is
explicit and unchanged — plugin TOML is as trusted as a shell profile. No
sandboxing in the port (it would break the model and the tests).

`stdin` capture (non-TTY → subject), PRIMARY/clipboard fallback (`wl-paste`),
and the `confirm` prompt all port directly as subprocess calls.

**Injection contract.** Subject-derived content flows into shell commands —
including dispatch-rule-rewritten text into a plugin-authored `cmd`. The guard
is the `|q` filter; safety depends on authors quoting subject values. **v1 keeps
the bash contract: author opts in with `|q`, `|raw` is the default.** This is
required for conformance (the hostile-content test asserts `|q` makes embedded
quotes/`$()`/backticks safe, and other templates rely on `|raw` for URLs/ids).
Making `|q` the *default* and requiring explicit `|raw` is a sound hardening
move — same mechanism, flipped default — but defer it to a post-parity pass; it
would silently change rendering for every existing template.

## 7. The `Address` type (IPC wire type)

```rust
enum Address {
    Source { name: String, input: String, params: Vec<(String, String)> }, // cosmic-goo:src:input?k=v
    Scheme { scheme: String, value: String },                              // cosmic-goo+scheme:value
}
```

`FromStr` / `Display` give the canonical `cosmic-goo:` ↔ struct round-trip.
Sigil expansion and native-shape detection (`./` `~/` `/` → file, `scheme://` →
URL, else text) happen *before* `FromStr`, in a `canonicalize(raw) -> Address`
step mirroring `address_canonicalize`. This enum is what the daemon protocol and
the launcher meta-plugin pass on the wire.

**`params` semantics — parity-first.** Bash's `_addr_params_to_json` builds a
JSON *object*, so repeated keys are last-write-wins and order is not preserved.
For v1 the Rust type matches that: a map (`BTreeMap<String,String>`), repeat
keys → last wins, order unspecified. (Order-preserving `Vec<(String,String)>`
with repeat-key semantics is a *post-parity* enhancement — it would change
observable behavior, so it's out of scope for the conformance port.) The struct
above shows `Vec` for illustration; the v1 impl uses the map to match bash.

## 8. Migration order

Crate-by-crate, each step staying green against the bats suite (point
`COSMIC_GOO_BUILTIN_PLUGINS_DIR` at the same `plugins/`, run the same `.bats`
against the Rust `goo`):

0. **Make the suite target-configurable** (do this first, it's free). The bats
   files hardcode `GOO="$REPO_ROOT/bin/goo"`; lift to
   `GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"` so the *same* integration suite can be
   pointed at a Rust artifact (`GOO_BIN=target/release/goo bats …`) without
   forking the tests. Without this, step 3's parity milestone isn't runnable.
1. **`goo-engine` registry + mime + address** — no behavior change observable
   yet; unit-test against `tests/*.bats` expectations ported as Rust tests.
2. **template + verbs (incl. `valid_when` via jaq) + dispatch** — now a Rust
   `goo` can resolve and render.
3. **`goo` bin** — wire argv parsing + the dispatch `case`; run the *integration*
   bats (`tests/integration/cli.bats`) against it. Parity milestone.
4. **`good` daemon** — only once Phase 2 (launcher) needs warm latency.
5. **`goo-compose`** — Phase 4, over the engine or socket.

**First concrete milestone:** Rust `goo` passing `tests/integration/cli.bats`
unmodified. That single file exercises addressing, verbs, adverbs, two-step
objects, aliases, and dispatch end-to-end — clearing it means the engine core is
at parity.

## 8a. Settled decisions

- **Workspace layout:** the Rust workspace lives in a `crates/` directory inside
  *this* repo (`crates/goo-engine`, `crates/goo`, …) — not a sibling repo. The
  tiers discussion holds: one repo until there's an external consumer of
  `goo-engine`; revisit a split only then.

## 9. Open questions for the next session

- **jaq vs. shell-jq for `valid_when`** — settle with the parity spike (§2).
- **`printf %q` ↔ Rust shell-quote** — confirm with the hostile-content fixtures
  before building on `template` (§3).
- **Exit-code granularity** — keep `1` catch-all or subdivide (§4). Conformance-
  neutral; pick a policy and document.
- **`toml` crate fidelity** — verify it parses every construct the plugins use
  (inline tables for `set`/`template_var`/`adverbs`, arrays-of-tables, the
  `[adverbs.values.NAME]` form) identically to the `tomlq` path.
