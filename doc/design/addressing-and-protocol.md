# Addressing — the goo:// URI layer (design)

> **Status: the URI layer.** Defines what a goo address *is* —
> `goo://<domain>/<path>[;matrix][?refine]`, the domain model, and the human
> shorthands. The `goo://` URI shape is **shipped** (the Rust engine and bash
> both resolve it; see [cli-reference](../cli-reference.md#subject-addressing)).
> The `[[domains]]` **unification** below — folding the `goo+<scheme>:<value>`
> handoffs (`file`/`text`/`clip`/…) into `goo://<domain>/<path>` so there is one
> canonical form — is **not built yet**; it's the next addressing step.
>
> The **request/wire layer** — how a verb + this URI + headers *travel* (the
> meta-verbs, case headers, status codes) — is a separate concern, developed in
> **[goo-protocol.md](./goo-protocol.md)**. This doc defines the address; that
> doc consumes it.

## Thesis: references, not data

The core realization: **goo sentences usually pass *references*, not data — it's
resource-id negotiation with verbs.** A subject is normally a *locator* (a path,
a URL, an app handle); inline content is the exception, supplied only when there
is no addressable referent (typed text, a pipe). This recasts the `.id`/`.text`
convention (which shipped this session) as a principle:

- **`.id` = the address** (the reference — almost always present)
- **`.text` = the inline body** (the data — the exception, like an HTTP body)

Everything below follows from treating addressing as a small, programmable REST
API over typed resources, with goo verbs as the (open-ended) methods.

## The URI

```
goo://<domain>/<path>[;matrix][?refine]
```

REST/WebDAV-shaped, parsed as a normal URI:

| part | role | example |
|---|---|---|
| scheme `goo` | the API | — |
| `<domain>` (authority) | a named resolver / resource collection | `goo://app/…` |
| `<path>` | a member within the domain (the locator) | `goo://app/firefox` |
| `;matrix` | **engine** addressing params (search term, mode) | `goo://app/;q=firefox` |
| `?refine` | **user/plugin** refinement filters | `?title=*Cosmic*` |

**Matrix vs query — the rule of thumb (from the handler's seat):**

> **`;matrix` is the *engine's* selection vocabulary; `?query` is the *handler's*
> own parameters.**

- **Matrix `;`** — a small, **fixed** set the engine owns and reads to decide
  *which / how many* entities: `q=` (search term), `n=` / `all` / `pick`
  (cardinality), `sort=`. Same across every domain. A plugin author **never
  writes a matrix param** — that namespace belongs to the resolver.
- **Query `?`** — **open** and the handler's: field filters, channel config,
  write modes, formats. The engine doesn't interpret it; it hands it to the
  handler.

Implementer's test: *"Does this param choose/rank/count entities, or tell my
handler how to behave?"* Choose/count → `;` (engine). Behave → `?` (handler).
**Per-entity rule:** every entity (subject, channel, target) carries its own
config on its **own address's `?`** — so params are unambiguous by *whose*
address they ride. (`Using: goo://channel/fabric?model=x`,
`To: goo://buffer/log?mode=append`, `goo://things/;q=x?title=beta`.) Matrix binds
to a path *segment*, which also gives each segment of a future hierarchy its own
params.

Keep `//<domain>` as the authority (not domain-as-path-segment): it preserves
the `//` that auto-linkifies and that one `x-scheme-handler/goo` registration
needs. Value = `goo://app/firefox`; search = `goo://app/;q=firefox`; refine =
append `?…`.

## Domains (supersede `[[sources]]`/`[[scheme]]`)

A **domain** is a named resolver = *name · type(s) it yields · capabilities*.
Capabilities, not kinds — a domain may have either or both:

- **value**: `<path>` *is* the identity/locator — an **exact** id match (`url`,
  `text`, `clip`; `file` by path; `app` by exact executable id). Deterministic
  given state; may still yield **several** entities that share that exact id
  (e.g. two `firefox` windows) — exact-vs-fuzzy, not one-vs-many.
- **search**: `<path>` is a **fuzzy** query over `list_cmd` output, written
  **explicitly** as the `;q=` matrix param (`goo://app/;q=firefox`). May match
  0/1/many ("Firefox", "firefox-config-editor", …).

Resolution is **strict** — the syntax says which you mean, no implicit fuzzy
fallback: a bare path (`goo://app/firefox`) is the **exact value**; search is
**only** the explicit `;q=` form. (The human sigils mirror this: `:app/firefox` →
value, `:app:firefox` → `;q=` search — see Sigils.)

**The full capability set (subjects, destinations, instruments are all domains).**
`value`/`search` are the *subject-side* capabilities (how an address resolves to
a thing). Extend the same "capabilities, not kinds" idea to the action side, and
every participant in a sentence is just a domain with some subset of:

| capability | invoked when the entity is the… | examples |
|---|---|---|
| **value / search** | subject (resolve to a thing) | `app`, `file`, `ssh-host` |
| **read** | subject (yields content) | `clip`, buffers, `file` |
| **write** | `To:` destination (accepts content) | buffers, `file`, a chat |
| **process** | `Using:` instrument (transforms / routes input→output) | `fabric`, an LLM, a mailer |

So a **buffer** is a `{read, write}` domain (a subject *and* a `To:` destination —
bidirectional storage; `^` is the unnamed clipboard buffer); a **channel** is a
`{process}` domain (the `Using:` instrument). They are **siblings, not twins**:
same capability-bearing-domain parent, different capabilities, different
grammatical case (`To:` Goal vs `Using:` Instrument). They **compose** in one
sentence rather than unifying:

```
SUMMARIZE goo://clip/  Using: goo://channel/fabric  To: goo://buffer/scratch
#          ^read subject  ^process (instrument)        ^write (destination)
```

A **process** domain (channel) is typed like a verb — `accepts → emits` + its own
params — and *choosing the instrument picks the result type via its `emits`*
(fabric → `text/plain`, a duckdb macro → `application/json`). The request/slot
mechanics live in [goo-protocol.md](./goo-protocol.md).

> **`list` is not a separate capability** — it's `search` at the *collection*
> level: `goo://app/` (no query) is the directory of members. A `value`-only
> domain (`text`, `url`) can't list. Whether a use collapses the collection to
> one (a subject) or keeps the set (`GET` / `goo list` / `;all`) is the
> orthogonal *cardinality* phase (`;n=`), not a capability.

Domains are ordinary registry entities — `[[domains]]` with `emits`, optional
`list_cmd` (⇒ search) and value-construction rule (⇒ value). **No reserved
names**; merged with override-by-name; `goo validate` *warns* on collision. The
built-in value domains (`url`/`file`/`text`/`clip`/`sel`/`stdin`) just ship in a
core plugin. This is what kills the earlier "reserved value-handler names" idea.

**`infer` — the default domain.** Shape-dispatch over the value domains:
`./ ~/ /` → `file`, `scheme://…` → `url`, else → `text`. Bare CLI input and an
empty authority route through `infer`; searching is always an explicit act.

`file` path encoding: absolute by default, `~` = home, `.`/`..` = cwd-relative
(context-dependent, may not exist).

## Sigils (shorthand for humans typing; machines emit canonical `goo://`)

Sigils are a **terminal convenience** — shorthands a person types instead of a
full `goo://` URI. Machines (`goo-compose`, the launcher, any IPC) emit the
canonical `goo://` form directly; they don't go through sigils.

**Design rule:** every sigil's separators come from the **shell-unquoted set**
(`/ : + ^` …) — never `; ? & | < > * ~`-at-start — so addresses are typed raw,
no quoting, on the CLI and in the launcher.

The built-in set is deliberately tiny (three marks); everything else is a
**user-defined `[[sigils]]` alias** — e.g. `@` is conventionally left for the
user (`@mine` → `goo://my-long-domain/favorite-thing`).

| you type | means | canonical |
|---|---|---|
| `foo` · `./x` · `~/x` · `https://x` | **infer** — bare; shape-dispatch (`./ ~/ /`→file, `scheme://`→url, else text); weighted choices on ambiguity | `goo://text/foo` · `goo://file/…` · `goo://url/https://x` |
| `+foo` | **literal text**, no inference | `goo://text/foo` |
| `:dom/path` | **domained value** — exact id (first segment is the domain; `/`=value) | `goo://dom/path` |
| `:dom:query` | **domained search** — fuzzy (`:` after the domain = the `;q=` query) | `goo://dom/;q=query` |
| `^` · `^name` | **clipboard** / named buffer (built-in) | `goo://clip/` · `goo://clip/name` |
| *your chars* | **user alias** (`[[sigils]]`), e.g. `@` | their `expands` |

The first separator after the domain decides: `:dom/a:b` → value `goo://dom/a:b`
(later `:` are path); `:dom:a/b` → search `goo://dom/;q=a/b`. `:dom:query` keeps
today's "look it up" muscle memory unchanged; the `/` value form is the new
capability. `goo+<scheme>:<value>` is **gone** — its schemes are now value
domains (`+foo` = `goo://text/foo`, `^` = `goo://clip/`, `./x` = `goo://file/…`).

## The request layer → goo-protocol.md

A full goo invocation — a verb + this URI + the indirect slots — is structurally
an HTTP/WebDAV request (we converged on this, didn't copy it). That **request/wire
layer is a separate concern** from the addressing defined here, and is developed
in **[goo-protocol.md](./goo-protocol.md)**:

- meta-verbs `GET` / `HEAD` / `OPTIONS` plus the `GOO` default verb;
- the **`To:` / `Using:` / `With:`** grammatical-case headers (which **supersede**
  the earlier `Destination:` / `Depth:` WebDAV-header sketch this section once held);
- the reused-HTTP **status table** (`300` / `301` / `404` / `415` / `422` / `424`
  / `428`) — which **supersedes** the earlier "resolve = `301` / multi-step =
  `CONTINUE`" sketch;
- `OPTIONS`-as-completion-oracle, and the `{id, label, weight}` entity shape.

All daemon-era (#31): `good` speaks it over a unix socket (`curl --unix-socket`-able),
and the addresses on the wire are exactly the `goo://` URIs defined above.

## Scheme name

Recommend **`goo://`** over `cosmic-goo://`: it matches the portable engine and
command (`bin/goo`, `crates/goo`); `cosmic-goo` is the COSMIC *distribution*, not
the engine. To our knowledge `goo` is not in the IANA URI Schemes registry
(permanent or provisional); desktop `x-scheme-handler` has no central authority
(per-system, first-come). Confirm at <https://www.iana.org/assignments/uri-schemes/>
if certainty is wanted. `cosmic-goo://` could remain a registered alias.

## Migration cost — adopting the `[[domains]]` unification

The `goo://` URI shape already ships (Rust engine + bash). What's *not* built is
the **`[[domains]]` unification**: folding the `goo+<scheme>:<value>` handoffs
(`file`/`text`/`clip`/`sel`/`stdin`/`url`) into `goo://<domain>/<path>`, so there
is **one** canonical form, superseding today's split between `[[sources]]` and the
built-in scheme-handlers. Adopting it is a multi-commit arc:

1. rewrite the addressing layer — the Rust engine `address` module (canonical)
   **and** `lib/address.sh` (the reference) — to the domain model (value-first /
   search-fallback);
2. migrate plugins' `[[sources]]` → `[[domains]]`, shipping the built-in value
   domains (`url`/`file`/`text`/`clip`/`sel`/`stdin`) in a core plugin;
3. re-green the Rust `address` tests + `tests/address.bats` + `cli.bats`;
4. update docs (cli-reference, plugin-authoring) and the Rust scoping doc
   (`Address { domain, path, matrix, refine }`).

**Behavior change to flag:** today `:source:input` is search; value-first /
search-fallback subtly changes resolution semantics that existing habits depend on.

The payoff (xdg registration, OS handoff, auto-linkify, the HTTP daemon) compounds
with a consumer — so adopt it alongside `goo-compose`/the launcher, not in a vacuum.
