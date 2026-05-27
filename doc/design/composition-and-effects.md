# Composition, effects, and the launcher closure

> **Status: design notes from a session following [goo-protocol.md](goo-protocol.md) and [addressing-and-protocol.md](addressing-and-protocol.md).** Captures proposals for MCP mapping, parameter resolution beyond `With:`, multi-sentence requests with scoped state, the functional framing of verbs-as-effect-typed-operations, and the `.gogo` file form. Companion to the existing protocol/addressing docs — assumes familiarity with them.
>
> Build gate unchanged: revisit each proposal when a real consumer exists. None of this should land before pressure from an actual use case. The shipped CLI is the `goo://` URI layer; this doc, like goo-protocol.md, is mostly ahead of its build.

---

## 1. Where this picks up

A previous session produced [goo-protocol.md](goo-protocol.md) (the HTTP/1.1 wire layer) and [addressing-and-protocol.md](addressing-and-protocol.md) (the `goo://` URI layer). This document captures a follow-on design conversation focused on:

- How goo maps to MCP (Model Context Protocol) in both directions
- How parameter values cross the engine/handler boundary when references need to deref to content
- How multi-sentence requests work, with locally-scoped state
- The functional substrate underneath the REST surface
- What goo *is*, in terms of existing precedents, and what's novel about the combination
- The closure that makes `.gogo` files launchable artifacts — and the resolution of the original cosmic-scenes problem

Companion to the existing docs; depends on them.

---

## 2. MCP mapping

MCP exposes three primitives — tools, resources, and prompts. These map cleanly onto existing goo concepts. **No new abstractions needed on goo's side.**

| MCP | goo |
|-----|-----|
| Tool | Verb |
| Resource | Subject (addressable, typed) |
| Prompt | Subject in a `prompt` domain, with a parameterized `{read}` capability |

### Tools ↔ Verbs

Direct. Tool name → verb name; the tool's `inputSchema` → the verb's OPTIONS schema (subject/To/Using/With → params, destructive/confirm → annotations). The protocol doc already claims this mapping; nothing new.

### Resources ↔ Subjects

A resource at `file:///foo/bar.md` is `goo://file/~/foo/bar.md`. Reading the resource requires a verb — `GET` for safe retrieval, or any verb whose `accepts` matches the resource's MIME. MCP separates "thing" and "thing-action" into different primitives; goo unifies them: the resource IS a subject, the action is a verb.

### Prompts ↔ Subjects in a `prompt` domain

An MCP prompt is invoked via `prompts/get` — structurally that's a *read* with arguments returning messages. So a prompt is a subject with a parameterized `{read}` capability, arguments riding on the URI's `?` refine slot:

```
goo://prompt/summarize_paper?paper_url=https://example.com
```

The default action (`GOO` verb) for a prompt subject reads it and routes the message sequence to `goo://chat/current`:

```
GOO goo://prompt/summarize_paper?paper_url=https://example.com
```

Reading without invoking is `GET goo://prompt/…` — fetch the template literally.

### Bidirectional handoff

- **goo → MCP**: `good --mcp` daemon exposes verbs as tools, subjects as resources, prompt-domain entries as prompts
- **MCP → goo**: each MCP server becomes a domain in goo, with the server's tools surfacing as verbs and its resources as subjects under that domain

`cosmic-fabric` exposing itself via MCP means `goo://channel/fabric` works naturally; exposing goo to Claude Desktop is just `good --mcp` running an adapter.

### Resource subscriptions — open

MCP resources can update over time (`resources/subscribe`, `notifications/resources/updated`). goo subjects are mostly static-once-resolved. The update story either rides on SSE over the existing `GET` (probably fine — `good` already speaks HTTP/1.1 over a unix socket, and SSE is `Content-Type: text/event-stream` on a long-lived GET) or surfaces as a new primitive. Decide before committing to resource-as-subject as the load-bearing mapping.

### Prompts as composable subjects

A surprise that falls out of "prompts are subjects": **prompts compose with verbs.**

```
SUMMARIZE goo://prompt/summarize_paper?paper_url=X
Using: goo://channel/fabric
To:    goo://buffer/scratch
```

Apply a verb to a prompt subject, not just invoke it. The prompt's read produces messages (text); any `text/*`-accepting verb can chain off that. Whether this composition is useful in practice or just a curiosity is unclear — flagging it as a discovery, not committing to it as a designed feature.

---

## 3. Parameter resolution: `Attach:` and the resolution gradient

The protocol doc's §4 specifies a resolution gradient for header references (try / require / literal), with `With:` explicitly never protocol-resolved. This section refines what happens when parameter values need to dereference *to content*.

### The 2D nature of parameter values

A parameter value answers two orthogonal questions:

- **By-value vs by-reference**: literal value, or address of something else?
- **Identity vs content**: if reference, want the identity (URL/path string) or the content (fetched bytes)?

Cross-product:

| Form | Meaning | Slot |
|------|---------|------|
| `source=spanish` | Literal value | `With:` |
| `source=goo://url/foo` | Literal `goo://` string (handler interprets) | `With:` (opaque pass-through per §4) |
| `source=<content of goo://url/foo>` | Resolved content | **`Attach:`** (new) |
| `source=<canonical id from search>` | Resolved identity | Rare; `Attach:` with directive, or defer |

### `Attach:` header (proposal)

The third case — "fetch the content of this reference, pass the bytes as the parameter value" — is not cleanly covered by `With:` (opaque) or `Using:` (instrument, not content). Propose a new `Attach:` header:

```
OPEN goo://prompt/summarize
Attach: source=goo://url/http://foo.com
```

Semantics: resolve the reference, dereference to content, pass content to the handler under the named parameter. Composes naturally with buffers as the intermediate handle:

```
READ http://foo.com
To:    goo://buffer/page

OPEN goo://prompt/summarize
Attach: source=goo://buffer/page
```

### Eager vs lazy resolution

When does `Attach:` resolve?

| Mode | Behavior | Fit |
|------|----------|-----|
| **Eager (default)** | Engine resolves on receipt; hands content to handler | Foreign handlers (can't speak `goo://`); matches §11 marshalling rules |
| **Lazy** | Engine passes hint; handler resolves when needed | goo-native handlers (can speak `goo://`); more efficient when handler may not consume the attachment |

Recommendation: eager-as-default, with goo-native handlers opting into lazy via the same `speaks = "goo"` flag the protocol doc proposes (§11). This matches the marshalling boundary discipline — foreign handlers get materialized content; goo-native handlers get references.

### Bind variables (deferred)

A more explicit form was considered, taking inspiration from SQL prepared statements:

```
OPEN goo://prompt/summarize?%key=%data
Bind-Content: data=goo://buffer/temp/728
Bind-Value:   key=source
```

Useful for **meta-templates** — prompts that construct other goo sentences. A `do-on` prompt taking a verb and a target:

```
GET goo://prompt/do-on
Bind-Verb:    action=SUMMARIZE  
Bind-Subject: target=goo://buffer/scratch
```

Produces messages that are themselves a goo sentence. Composes with GOGO (§5).

**Deferred**: overkill for the basic prompt-instantiation case. Revisit if meta-template use cases actually surface in practice.

---

## 4. Multi-sentence requests, anonymous buffers, scope

Three connected questions:

1. **Do connections get private buffers?** **No** — connection-scoped state breaks "addressing is the namespace" and forces every handler to track connection identity. Connections stay stateless.
2. **Can multiple sentences chain or sequence in a single request?** **Yes**, with body framing.
3. **Within a request, are there anonymous buffers scoped to that request?** **Yes**.

Unified answer: **a request defines a scope.**

### `BATCH` verb (proposal)

`BATCH` takes a body of sentences delimited by `---`:

```
BATCH /  HTTP/1.1
Content-Type: text/x-goo-batch

OPEN http://foo.com
To: $raw
---
SAVE $raw
To: foo.html
---
OPEN goo://prompt/summarize
Attach: source=$raw
```

Preserves "one HTTP request = one BATCH" while letting `$ref` variables be shared across the inner sentences. HTTP pipelining (multiple requests on one connection) is rejected because it loses this shared scope.

### Anonymous buffers (`$ref`)

Within a BATCH body, `$name` introduces a request-scoped buffer. Distinct namespace from `goo://buffer/name` (global). Subtle decisions:

- **Namespace**: `$name` ≠ `goo://buffer/name`. The sigil makes the scope visible.
- **Shell quoting**: less concerning than it seems because BATCH submits via stdin / heredoc / file body, not via shell args:
  ```bash
  goo BATCH <<'EOF'
  OPEN http://foo.com To: $raw
  ...
  EOF
  ```
  The single-quoted heredoc keeps `$` opaque to the shell.
- **Lifetime**: cleaned at batch end. No GC pressure on the global buffer namespace.
- **Sub-scoping**: nested BATCH or GOGO (§7) get fresh scope; parent vars don't leak. Closures deferred.

### BATCH vs SEQUENCE

| Verb | Execution order | Analogue |
|------|-----------------|----------|
| `BATCH` | Dependency-driven; engine evaluates DAG of `$ref` dependencies | Haskell `let` |
| `SEQUENCE` | Order-explicit; sentences run in written order regardless of dependencies | Haskell `do` |

Most workflows want `BATCH` (let the engine determine dependencies). `SEQUENCE` is for when ordering of side effects matters and the dependency graph can't determine it: "write A then write B, in that order, even though neither depends on the other."

### Cycle detection

`BATCH` is dependency-driven, so cycles in the `$ref` DAG must be detected. If `$a` depends on `$b` and `$b` depends on `$a`, refuse with an error. Topological sort + cycle detection at parse time, before any sentences execute. Worth being explicit about as part of BATCH semantics.

### Result of a BATCH

Multi-value via addressing, not via multipart body. The BATCH result is itself a subject — a directory-like resource:

```
BATCH ...  →  201 Created
              Location: goo://result/batch-abc123/

goo://result/batch-abc123/
├── foo.html      # named via To:
├── summary       # named via To:
├── value         # implicit last-sentence result, if uncaptured
└── log           # diagnostic stream
```

Pass the result subject to other verbs; navigate into it for individual outputs. The batch result is composable — `SUMMARIZE goo://result/batch-abc123/value` works after the fact. Default ephemeral lifetime (lifetime of the response or short TTL); opt-in persistence via `To:` on the BATCH itself.

HTTP's `Location:` header is already the right pattern: `201 Created` with a `Location:` pointing at the result resource. Standard, not invented.

### The transaction metaphor

A batched request is structurally a transaction. SQL transactions have a scope, local temp tables (the anonymous buffers), ordered execution, isolation from other transactions, some flavor of atomicity. Full ACID is too aggressive for desktop ops (rollback-on-error on a desktop action is rarely useful), but the *scope* part is exactly right. A batch carves out a small region of namespace for ordered execution with local helpers.

---

## 5. The functional framing

The framing of "BATCH is shell-pipeline-like" is wrong. Goo is functional, not procedural.

### Effects are first-class in the type signature

- **Subjects** are values
- **Verbs** are functions: `accepts → emits`
- **`Using:`** selects which transformation (higher-order over verbs)
- **`With:`** is the parameters
- **`To:`** is an **explicit effect annotation** — "this verb will write to this location"

A verb without `To:` is "pure" (produces a result, doesn't change the world). A verb with `To:` declares its observable side effects. That's algebraic effects (Koka effect rows, Frank, Eff) in REST clothing — directly the substrate for the type-honesty / effect-row machinery the Ma framework theorized about. Not accidental architecture.

### `$ref` is Haskell's `<-`

```
OPEN http://foo.com  To: $raw
SAVE $raw           To: foo.html
SUMMARIZE           Attach: source=$raw
```

desugars to:

```haskell
do
  raw <- OPEN "http://foo.com"
  _   <- SAVE raw "foo.html"
  SUMMARIZE raw
```

Each `$ref` is a let-binding. The batch is the do-block. The HTTP wire format is the IR (intermediate representation) of an effect-typed functional language.

### Two-level design

| Layer | Style | Has |
|-------|-------|-----|
| Wire protocol | HTTP-ish (literal HTTP/1.1) | Stateless, REST verbs, headers, status codes |
| Batch body | Shell-ish + functional | Local variables, dependency-driven evaluation, effect annotations |

The two layers don't need to share semantics. HTTP requests don't have local variables; batches do. Each layer carries the abstractions it's good at without contaminating the other.

---

## 6. What goo *is* — analogues and what's novel

Candidate framings (none captures it alone):

| Analogue | Captures | Misses |
|----------|----------|--------|
| Fielding's REST (PhD dissertation) | HATEOAS, OPTIONS-as-discovery, hypermedia-driven application architecture | Most things called REST today are pale shadows of this |
| WebDAV taken seriously as UX | MOVE/COPY/PROPFIND, DEPTH, extended HTTP as application protocol | Built as file-sync protocol; never developed as interaction model |
| Effects-and-handlers languages (Koka, Frank, Eff) | `To:` ≈ `<eff>` annotation | Languages not protocols; no URI addressing |
| Apple Shortcuts / Quicksilver / Kupfer | Composable typed actions, even variables | GUI-only, no wire protocol, no distributed extension model |
| pop-launcher + MCP | TOML plugins with type-aware discovery + cross-process tool protocol | Goo unifies them |
| PowerShell + Smalltalk + HTTP | Typed pipelines + messages-to-objects + wire format | Goo adds effect typing and URI addressing |
| REBOL / Red | URLs as first-class values, function dispatch, dialects | Language not protocol; obscure |

### What's actually novel in the combination

1. **REST as surface syntax for a programming language**, not as an API style — Fielding REST with verbs as functions and OPTIONS as type-discovery
2. **Capability-bearing domains** as type unification — `{value, search, read, write, process}` as composable capability sets rather than parallel kinds (sources / destinations / instruments as separate taxonomies)
3. **MCP-equivalence falling out of the protocol** — not designed in; emerges because OPTIONS is structurally identical to MCP's `inputSchema`
4. **Buffers as materialization primitive** keeping the system reference-based end-to-end — adjacent systems handwave the data-vs-references problem

The deepest precedent is probably the unwritten one: **Fielding's REST as it was meant to be, finally instantiated as a user-facing programming substrate rather than an API style.** Most things called REST are one shadow of the original idea; goo is closer to the actual thing.

Whether that lineage matters for adoption is unclear. The HATEOAS community has been making this argument for 20 years without much uptake. Goo might land in the same place — appreciated by a small population, ignored by everyone else, deeply correct. Or the MCP equivalence might make it instantly useful as AI infrastructure in a way pure HATEOAS never was. Hard to predict.

---

## 7. `.gogo` files, GOGO, and the launcher closure

### The executable knowledge graph framing

Most knowledge graphs are read-only — you query relationships between nodes. Goo's nodes are addressable AND its edges are executable, with explicit effect declarations. That's "what can act on what, with what observable consequences." A category without a clean existing name — Linked Data Notifications + Solid gestures at it; Wolfram Language has the symbolic-everything flavor without the effect typing.

### `.gogo` files

A `.gogo` file is reified goo: data on disk representing a sequence of goo sentences.

```
#!/usr/bin/env goo
OPEN goo://file:///etc/motd
Using: goo://app/firefox
To:    goo://desktop/monitor/0/workspace/2
```

Invocable three ways:

1. **`GOGO my.gogo`** — explicit eval verb
2. **`GOO my.gogo`** — default verb for type `application/x-goo-script` is GOGO
3. **`./my.gogo`** — shebang + filesystem execute bit + type inference dispatch to GOGO

### `GOGO` as verb and as channel

`GOGO` is both:

- **A verb**: take a `text/x-goo-script` subject and execute its sentences
- **A `{process}` channel**: `goo://channel/gogo` accepts `text/x-goo-script` and emits whatever the script's last sentence emits

Both views are correct. The channel form composes with prompts that produce sentences:

```
GET goo://prompt/do-on
Using: goo://channel/gogo
```

Read prompt → get back a goo sentence → eval it. Compile-time meta-templating.

### Not monad return — reification

In Haskell, `return :: a -> m a` lifts a value into a computation context. The `.gogo` file isn't that. It's closer to Lisp's quote/eval:

```lisp
(quote (foo bar baz))    ; data representing code
(eval '(foo bar baz))    ; execute the quoted form
```

The `.gogo` file is **reified** goo. The corresponding type-theoretic concept is **first-class functions** — the protocol now has them. Scripts become values you can address, pass around, share, version. Higher-order goo: a verb taking a script via `Using:` or `Attach:`. Closures (later): a script that captures `$ref` from its defining batch.

The do-notation analogy still holds for BATCH:
- **`.gogo` file** = function definition
- **BATCH body** = function body
- **GOGO** = function application

### The launcher closure

The original cosmic-scenes problem was "use keyboard keys to launch curated actions via a launcher." That problem resolves cleanly through this protocol:

1. The launcher was originally for invoking apps and scenes
2. Goo generalized "invoking things" via verbs on addressable subjects
3. `.gogo` files are addressable subjects of type `application/x-goo-script`
4. The default verb for that type is GOGO
5. The launcher invokes `.gogo` files just like apps — it doesn't need to know they're scripts

End state: **any goo computation is launchable; any launchable thing is a goo computation.**

Users write arbitrarily complex goo programs, bind them to keys, save them in launcher catalogs, share them as files. No special-case launcher integration per scene-type. The launcher becomes a thin invocation surface over the protocol's full expressive power.

---

## 8. Build gate and implementation order

Same discipline as goo-protocol.md: revisit each proposal when a real consumer exists. Implementation order follows user need, not design completeness.

| Proposal | Build when |
|----------|-----------|
| MCP mapping (tools/resources/prompts) | A goo-MCP adapter has demand from either side |
| `Attach:` header | A real plugin needs content-deref of a reference (likely soon for fabric verbs taking URLs) |
| `BATCH` verb + `$ref` scope | Multi-step user flows accumulate friction the CLI can't handle |
| `SEQUENCE` verb | Side-effect ordering bugs appear in BATCH workflows |
| Multi-value via result-subject | Alongside BATCH |
| `.gogo` file form + `GOGO` verb | When ad-hoc shell scripts wrapping `goo` calls hit friction |
| Lazy `Attach:` for goo-native handlers | Eager has a clear performance ceiling |
| Bind variables (`Bind-Verb:` / `Bind-Subject:`) | Meta-template use case actually surfaces |
| Resource subscriptions over SSE | MCP integration is live and someone asks |
| Closures over `$ref` | Deferred to v2+ |

Implementing BATCH and GOGO will likely reveal more than further design will. The undiscovered architecture is in the implementation, not in more upfront formalism.

---

## 9. Deferred / open

- **Resource subscription semantics** — SSE-over-GET, or a new primitive?
- **Pipeline syntax** — shorthand for chained sentences (à la shell `|`)? Defer until BATCH friction is felt.
- **Closures over `$ref`** — scripts that capture environment from their defining batch. Powerful but not yet needed.
- **Formal type theory** — the connections (do-notation, monads, effect rows) are named here without development. Someone with motivation can produce the formalism later; it doesn't block implementation.
- **User-facing surface design** — most of what's in this doc is plumbing. Worth being clear in user-facing docs that BATCH / SEQUENCE / GOGO / effect typing / capability domains are designer-and-plugin-author concerns. End users see "bind a key to run this script."
- **Result-subject lifetime policy** — default ephemeral is right, but exact TTL and persistence semantics need to be specified before implementation.
- **`Attach:` and structured (non-text) content** — clear for text; less clear for binary buffers (images, PDFs) where the materialization form matters more. Probably falls out of the marshalling rules in §11 of goo-protocol.md, but worth verifying.

---

## Cross-references

- [goo-protocol.md](goo-protocol.md) — the HTTP/1.1 wire layer, status codes, slot mechanics, OPTIONS-as-discovery, the marshalling boundary (§11). This doc extends but does not supersede.
- [addressing-and-protocol.md](addressing-and-protocol.md) — the `goo://` URI layer, the capability-bearing domain model, the matrix-vs-query rule, sigils. This doc extends.
