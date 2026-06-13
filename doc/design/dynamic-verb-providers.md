# Dynamic verb providers (`[[providers]]`)

> Status: **shipped** (Rust engine). A `[[providers]]` entry lets an external
> tool's command registry surface as goo verbs on a subject — the worked example
> is [blq](https://github.com/teaguesterling/blq)'s per-project commands appearing
> as verbs on `:cwd`.

## The idea

Every verb goo knows is normally a static `[[verbs]]` entry in a plugin TOML. A
**provider** is the dynamic counterpart: at the moment goo *lists* the verbs for a
subject, it runs the provider's `list_cmd`, and turns each emitted
`{name, description}` into a verb on that subject. The verbs aren't known until
runtime — they come from whatever the external tool currently has registered.

This is the verb analogue of a source's `list_cmd` (which enumerates *subjects*
dynamically) and an adverb's `values_cmd` (which enumerates *adverb values*).

```toml
[[providers]]
name = "blq"
for_type = "application/vnd.goo.cwd"   # which subjects these verbs attach to
list_cmd = "blq commands list --json | jq -c 'to_entries | map({name: .key, description: .value.description})'"
run = "blq run {verb.name}"            # the chosen verb's cmd; {verb.name} substitutes at run time
```

```
$ goo -c doc/examples/blq.toml do :cwd        # blq's registered commands, as verbs
applicable verbs for :cwd  (type: application/vnd.goo.cwd)
    test    Conformance suite (bats)
    docs    Build the docs site
    tiers   Plugins by dependency tier
$ goo do :cwd test                            # noun-first → blq run test (captured)
$ goo test :cwd                               # verb-first works too
```

## Fields

| Field | Meaning |
|---|---|
| `name` | Provider id (unique; later plugins override by name). |
| `for_type` | The subject type the dynamic verbs attach to (subtype-aware via `is_subtype`). Keep it **specific** — every matching subject pays a `list_cmd` exec when its verbs are listed. Don't use a broad glob. |
| `list_cmd` | Shell command emitting a JSON array of `{name, description}`. Runs in the user's cwd. |
| `run` | Command template for a chosen verb — becomes the synthesized verb's `cmd`. `{verb.name}` (and `{subject.*}`) substitute at run time. |
| `confirm` | Optional; marks synthesized verbs as needing confirmation. |

## How it works

- **One seam.** `verbs::for_subject` (the SSOT behind `what` / `do` / OPTIONS /
  completion / the compose GUI) appends `provider_verbs_for(reg, subject)` after
  the static verbs. Light one place, every surface lights up.
- **Synthesis.** Each stub becomes `{name, description, accepts:[for_type],
  cmd:<run>, dynamic:true, provider:<name>}`. Because `build_context` puts the
  verb into the substitution context, `{verb.name}` in `run` resolves with no
  template-engine change.
- **The `:cwd` subject** is an engine built-in (`address.rs`), a contextual
  subject like `:sel` / `^clip` — `goo://cwd/`. Its type
  `application/vnd.goo.cwd` is declared by the `working-directory` core plugin.
- **Verb-first execution.** `goo <dynverb> <addr>` (and `goo do <addr> <dynverb>`,
  which reorders to it) fails the static `verbs::lookup`, then retries via
  `dynamic_verb_for`: only if there are providers **and** the positional is an
  explicit address does it resolve the subject and search its provider verbs. A
  bare typo (`goo tset`) still dies fast — no subject resolution, no exec.

## Guarantees & limits

- **Graceful by contract.** This runs while *listing* verbs. A `list_cmd` that
  exits non-zero, names a missing tool, or emits non-JSON yields **no verbs** —
  it never breaks the listing. (No blq / no `.bird` ⇒ `:cwd` simply has no
  provider verbs.)
- **Static wins collisions.** A provider can't shadow a built-in verb of the same
  name on that type; the static one is kept.
- **Cost is bounded.** The fast path returns immediately unless `reg["providers"]`
  is non-empty *and* `subject.type` matches a provider's `for_type` (gated by
  `is_subtype` before any exec). In practice only `:cwd` pays — roughly one
  `list_cmd` per provider-backed invocation (no cross-invocation cache; goo is
  one-shot). Memoization is deliberately deferred until a hot double-call shows up.
- **Reserved-subcommand shadowing.** If a tool registers a command named `list` /
  `do` / `what` / `validate` …, `goo list :cwd` hits goo's subcommand arm, so that
  dynamic verb is only reachable noun-first (`goo do :cwd list`). Names are
  runtime, so this can't be validated at load — it's a documented hazard.

## Security: quote interpolated names

A synthesized verb's `cmd` is the provider's `run` template, run via `bash -c`.
The verb **name** is attacker-influenced data — it comes from a project-local
registry (e.g. `.bird/commands.toml`), and goo's whole pitch is pointing at *any*
directory. So a `run` template **must shell-quote** any interpolated name:

```toml
run = "blq run {verb.name|q}"   # |q is mandatory — NOT {verb.name}
```

Without `|q`, a command named `x;rm -rf ~` injects when the cmd runs. With `|q`
the name is passed as a single shell-quoted argument (the underlying tool sees the
same value). The conformance suite proves both directions (`tests/integration/
providers.bats`): a name like `a;touch pwned` creates no file under `|q`.

**Follow-up (not yet built):** the engine relies on the *author* remembering `|q`.
Harden it engine-side — either reject names containing shell metacharacters at
synthesis time, or pass dynamic arguments via argv instead of shell-interpolating
them — so a forgotten `|q` can't be exploited. Tracked as a hardening task.

## Generalizes

blq is one provider. The same primitive turns any command registry into a goo
verb namespace on the right subject — `make` targets, `npm`/`pnpm` scripts, `just`
recipes, `cargo` aliases — each a `[[providers]]` entry with a `for_type` and a
`list_cmd`. Only blq ships as a worked example (`doc/examples/blq.toml`); core
stays free of third-party deps.
