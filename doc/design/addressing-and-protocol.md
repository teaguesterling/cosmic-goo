# Addressing & protocol — design exploration

> **Status: considered, NOT implemented in v1.** This captures a design
> conversation (2026-05-25) about evolving goo's canonical URI into a
> REST/WebDAV-shaped addressing + protocol model. The *shipped* addressing is
> still `cosmic-goo:<source>:<input>` / `cosmic-goo+<scheme>:<value>` (see
> [cli-reference](../cli-reference.md#subject-addressing)). Revisit this when a
> real consumer exists (launcher meta-plugin #38, daemon #31, or xdg scheme
> registration) — not before. The Rust port (rust-port-scoping.md) targets the
> *current* form unless/until this is adopted.

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

## Sigils (unchanged in spirit — nobody hand-writes `goo://`)

| you type | means | canonical |
|---|---|---|
| `:domain:query` | search a domain | `goo://domain/;q=query` |
| `+domain:id` | a determined member | `goo://domain/id` |
| `^` | clipboard (built-in) | `goo://clip/` |
| `./x`, `https://x` | native shapes → `infer` | `goo://file/…`, `goo://url/https://x` |

## The request analogy (→ daemon protocol, #31)

A full goo invocation is structurally an HTTP/WebDAV request — we converged on
this, didn't copy it:

| goo | HTTP / WebDAV |
|---|---|
| verb | method (`open`↔GET; verbs ↔ extension methods) |
| subject (URI) | request URI |
| object (two-step target) | **`Destination:` header** (WebDAV `MOVE`/`COPY`) |
| adverbs | headers (we have `--depth`; WebDAV has `Depth:`) |
| inline content | body |

Consequences, all **daemon-era (#31)**, not the bash CLI:

- `good` can speak HTTP (or an HTTP-shaped line protocol) over its unix socket —
  debuggable with `curl --unix-socket`; the launcher/IPC just make requests.
- **Resolve = 301.** Resolving a *search* yields a *determined* reference with
  metadata: `goo://app/;q=firefox` → `301 goo://app/firefox?pid=1234&title=…`.
  The warm daemon does this id-negotiation; the one-shot doesn't need it.
- **Multi-step = CONTINUE / 3xx.** A verb needing an object, or a compose flow,
  is a continuation (`100 Continue` / `300 Multiple Choices`) the client
  satisfies. Maps compose's interactivity onto status-driven steps.

## Scheme name

Recommend **`goo://`** over `cosmic-goo://`: it matches the portable engine and
command (`bin/goo`, `crates/goo`); `cosmic-goo` is the COSMIC *distribution*, not
the engine. To our knowledge `goo` is not in the IANA URI Schemes registry
(permanent or provisional); desktop `x-scheme-handler` has no central authority
(per-system, first-come). Confirm at <https://www.iana.org/assignments/uri-schemes/>
if certainty is wanted. `cosmic-goo://` could remain a registered alias.

## Relationship to today / migration cost (if adopted)

This **supersedes** the shipped `cosmic-goo:<source>:<input>` /
`cosmic-goo+<scheme>:<value>` forms. Adopting it is a multi-commit arc:

1. rewrite `lib/address.sh` (canonicalize/resolve, the `[[domains]]` model),
2. migrate every plugin's `[[sources]]` → `[[domains]]`,
3. re-green `tests/address.bats` (+ likely `complete`/`plugins`/integration),
4. update docs (cli-reference, plugin-authoring), the addressing memory, and the
   Rust scoping doc (`Address { domain, path, matrix, refine }`),
5. then port the now-final shape to Rust.

**Behavior change to flag:** today `:source:input` is search; value-first /
search-fallback subtly changes resolution semantics that existing habits depend
on.

Because the payoff (xdg registration, OS handoff, auto-linkify, the HTTP daemon)
only lands when there's a consumer — and there isn't one yet — this stays a
considered design, deliberately ahead of its build.
