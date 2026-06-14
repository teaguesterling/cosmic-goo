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

## Security: names are validated, so injection is impossible by construction

A synthesized verb's `cmd` is the provider's `run` template, run via `bash -c`,
and the verb **name** is attacker-influenced data — it comes from a project-local
registry (e.g. `.bird/commands.toml`), and goo's whole pitch is pointing at *any*
directory. So the name could carry shell syntax.

The fix is structural, not a quoting convention. Two complementary rules close
**every** path by which a provider's runtime output could reach the bash-run cmd:

**1. A verb name must be a shell-neutral identifier** (`verbs::is_valid_verb_name`
— starts alphanumeric, then alphanumerics or `-_.:/+`; no whitespace, no bash
metacharacter). The same rule applies everywhere a verb name appears:

- **Dynamic (provider) names** are filtered at synthesis — a stub named
  `a;rm -rf ~` is dropped and never becomes a verb, so it can't reach the cmd.
- **Static names** are rejected by `goo validate` with the same rule (a plugin
  can't ship a sloppy name either).

**2. A dynamic verb exposes only its `name` to the template.** `description` (and
any other stub field) is untrusted free text from the same registry as the name,
and *cannot* be charset-restricted — it's prose. So `build_context` gives a
dynamic verb's cmd template a `verb` object containing **only the validated
`name`**: `{verb.description}` and friends resolve to empty, never the stub value.
(Static verbs still expose all their author-trusted custom fields, e.g.
`{verb.fabric_pattern}`.) The description is still shown in the *listing* — it's
display-only, never templated.

Because no invalid name exists and no other field reaches the template,
`{verb.name}` can't carry an injection and `{verb.description}` resolves empty —
**with or without `|q`**. The conformance suite proves both with *unquoted* run
templates (`tests/integration/providers.bats`): a hostile `a;touch pwned` name is
absent from the listing and invoking it is `unknown verb`; a `description` of
`$(touch …)` reaches the cmd as nothing. Quoting an interpolated value remains
good belt-and-suspenders hygiene, but it is no longer what makes this safe.

### Terminal display of untrusted text

A `description` (and any source/provider-derived title, id, or a rendered cmd that
interpolates them) is still *shown* in a terminal surface: the verb listing, the
confirm prompt, the ambiguous-subject picker, and the implicit-subject snippet
(which previews untrusted clipboard / PRIMARY content). That text is untrusted
too: a raw ANSI escape, an OSC title-set, or a CR/LF could recolor the terminal,
rewrite its title, or spoof other listing lines. This is the same class of bug one
interpreter over (the terminal, not the shell), so untrusted text is run through
`display_safe` (`main.rs`) — which strips all Unicode control characters (C0, DEL,
C1) and keeps printable text — at each of goo's human-readable display surfaces:
the verb listing, the confirm prompt (`subject:`/`runs:`/`about to`), the
ambiguous-subject picker, the implicit-subject snippet (clipboard / PRIMARY
preview), and the `--explain --explain-with shell` command view (the subject path
baked into the shown command, sanitized in `substitute_subject` — display-only;
the real run renders through the engine, and the colored route renderers carry
only controlled type/verb names, so the strip stays on the one untrusted value).
Machine output (`goo list`, `goo options`) stays JSON — already
control-char-escaped — so it's unaffected.

Excluded for cause (not gaps): `goo __complete` candidates are insert-values (the
shell inserts the chosen id literally), not display strings — sanitizing would
change the value, and a control char in a completable id is an unusable id anyway.
MIME/route type names are libmagic/registry-derived (controlled vocabulary);
plugin/verb descriptions in `describe`/`plugins` are author-trusted (like a verb's
`cmd`); and a verb name echoed in an `unknown verb` error is the user's own argv.

Negative tests: `tests/integration/display-safety.bats` (a hostile description, an
escape-laden subject, and a hostile filename in `--explain`) plus
`display_safe`/`implicit_snippet` unit tests.

### Why display safety is by enumeration, not by construction (deferred: typed wrapper)

The shell-injection guarantee above is **by construction** — a malicious value
can't exist (name validation) or can't reach the cmd (template-namespace
restriction). Display safety is **by enumeration**: we sanitize at each call site,
which is inherently more fragile (a *future* print site could forget
`display_safe`). The by-construction analogue would be a `Tainted<String>` newtype
that can't be `Display`'d without sanitizing. It is **deliberately deferred**, for
two reasons:

1. **Severity.** Display-escape injection is spoofing/cosmetic (recolor, retitle,
   a misleading listing line) — not code execution. The one surface where it could
   actually escalate (overwriting the confirm prompt's `subject:`/`runs:` to trick
   approval of a destructive verb on a different subject) is already sanitized.
   After the enumeration above, what remains is *future* regressions, not a present
   hole.
2. **It only works if it's total.** Provenance is erased at the
   `bash_stdout`→`serde_json` boundary: an untrusted `list_cmd` title and a trusted
   static verb description both land as `Value::String` in the same structures,
   read by the same `as_str()` used for addressing and `|q` templating. A taint
   newtype delivers "can't print unsanitized" only if the Value type is split *by
   provenance* (dynamic-source subjects vs static registry data) all the way
   through `resolve`/`load_all` — a large, architecture-defining refactor. Any
   cheaper partial (tainting a few accessors) degenerates back to enumeration with
   extra ceremony.

So the honest done-state for this class is **complete enumeration + this documented
residual**, not a wrapper. Revisit only if a threat model weights terminal-spoofing
high enough to justify the total refactor.

## Generalizes

blq is one provider. The same primitive turns any command registry into a goo
verb namespace on the right subject — `make` targets, `npm`/`pnpm` scripts, `just`
recipes, `cargo` aliases — each a `[[providers]]` entry with a `for_type` and a
`list_cmd`. Only blq ships as a worked example (`doc/examples/blq.toml`); core
stays free of third-party deps.
