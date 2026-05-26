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

**Why matrix (`;`) for search, query (`?`) for refine:** they're two namespaces,
so the engine's search key (`q`) can never collide with a user/plugin filter
key. Matrix params bind to a path *segment*, which also gives each segment of a
future hierarchy its own params.

Keep `//<domain>` as the authority (not domain-as-path-segment): it preserves
the `//` that auto-linkifies and that one `x-scheme-handler/goo` registration
needs. Value = `goo://app/firefox`; search = `goo://app/;q=firefox`; refine =
append `?…`.

## Domains (supersede `[[sources]]`/`[[scheme]]`)

A **domain** is a named resolver = *name · type(s) it yields · capabilities*.
Capabilities, not kinds — a domain may have either or both:

- **value**: `<rest>` *is* the identity/locator (`url`, `text`, `clip`; `file`
  by path; `app` by exact id). Deterministic.
- **search**: `<rest>` is a query over `list_cmd` output (`app`, `ws`, `repo`;
  `file` fuzzy). May match 0/1/many.

Resolution is **value-first, search-fallback**: an exact id wins, else fuzzy.
(`+` sigil forces value-only; `:` allows search.)

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

> **NOTE — under review (TBD).** The built-in sigil *selection* below predates the
> domain model and is being rethought. Direction: sigils are primarily a
> **user-defined alias tool** (`[[sigils]]`); the built-in set should be minimal.
> Value-first/search-fallback collapses value-vs-query in most cases, so the only
> distinction that genuinely needs a marker is *forcing* search; `^` is just a
> clipboard convenience; un-domained input routes through `infer`. A separate
> design pass will settle the final set — see the sigil-model discussion.

| you type | means | canonical |
|---|---|---|
| `:domain:query` | search a domain | `goo://domain/;q=query` |
| `+domain:id` | a determined member | `goo://domain/id` |
| `^` | clipboard (built-in) | `goo://clip/` |
| `./x`, `https://x` | native shapes → `infer` | `goo://file/…`, `goo://url/https://x` |

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
