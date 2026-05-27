# The goo request protocol — design exploration

> **Status: design — the request/wire layer.** A goo invocation as a verb + a
> typed subject + grammatical-case headers, spoken as **literal HTTP/1.1**.
> Companion to [addressing-and-protocol.md] (the **URI layer** — what a `goo://`
> address *is*); this file is the **request layer** — how a verb + that URI +
> headers *travel*. It refined that doc's earlier `Destination:`/`Depth:`
> WebDAV-header sketch toward `To:`/`Using:`/`With:`, and its `301`-for-resolve /
> `CONTINUE` sketch toward the status table below — and that doc's "request
> analogy" section has now been **removed and handed off here**, so the two no
> longer overlap: addressing defines the address, this defines the request.
>
> Build gate unchanged: revisit when a real consumer exists (launcher meta-plugin
> #38, daemon #31, xdg scheme registration) — **not before.** The shipped CLI
> addressing is the `goo://` URI layer; this request/wire layer is deliberately
> ahead of its build.

[addressing-and-protocol.md]: ./addressing-and-protocol.md

## 1. Two layers: strict canonical / loose surface

goo has a **strict canonical** layer (fully-typed `goo://` URIs, explicit headers —
what machines, agents, the wire, caches, and logs use) and a **loose surface**
layer (CLI flags, bare values, sigils, the `infer` domain — what humans and
tab-completion use). **Inference canonicalizes loose → strict before dispatch.**
This is just naming what sigils + `infer` already do.

```
goo open ~/page.html --with firefox          # loose (CLI)
  ─ inference ─▶
OPEN goo://file/~/page.html  HTTP/1.1         # strict (wire)
Using: goo://app/firefox
```

## 2. The sentence

A full invocation is one HTTP request. The slots are grammatical **cases**
(the precedent is SIP's `To:`/`Via:`, not invented):

| slot | case role | carries | example |
|---|---|---|---|
| **verb** (method) | the action | a method token | `SUMMARIZE`, `MOVE`, `GOO` |
| **subject** (request-target URI) | Theme / Patient | what is acted on | `goo://file/~/article.md` |
| **`Using:`** | Agent / Instrument | what performs / through what channel | `goo://channel/fabric` |
| **`To:`** | Recipient / Goal (terminative) | what receives / where the *result* lands / a target value | `goo://chat/new`, `spanish` |
| **`From:`** | Source / origin (ablative) | the calling session — a named return address + origin kind, when the inherited channel isn't enough (deferred, §12) | `goo://session/current` |
| **`Log:`** | (secondary Goal) | where *diagnostic / secondary* output lands | `goo://file/~/goo.log`, `^scratch` |
| **`With:`** | Manner | opaque `key=value` config | `depth=brief model=iq4xs` |
| **body** | inline Theme | data, when there's no addressable referent | piped text |

> **Thesis (from [addressing-and-protocol.md]): references, not data.** A subject is
> normally a *locator* (`.id`); inline content (`.text`, the body) is the exception.

```
SUMMARIZE goo://file/~/article.md   HTTP/1.1
Using: goo://channel/fabric?model=iq4xs&context=1024
To:    goo://chat/new?title=Q3%20brief
With:  depth=brief
```

`curl`-able, because it's literal HTTP/1.1 over a unix socket:

```bash
curl --unix-socket /run/user/$UID/goo.sock -X SUMMARIZE 'goo://file/~/article.md' \
     -H 'Using: goo://channel/fabric?model=iq4xs' -H 'To: goo://chat/new'
```

*(Transport detail: the request-target may be absolute-form `goo://domain/path`,
or decompose to `Host: domain` + origin-form `/path`; either is valid HTTP/1.1.)*

**The version token is optional (loose form).** The minimal valid invocation is
just `VERB <subject>` — `GOO ~/foo.html`, `OPEN goo://app/firefox` — which is
*literally the CLI* (`goo ~/foo.html` → `GOO ~/foo.html`). A parser missing the
`HTTP/1.1` (or `goo/1.0`) token assumes the current version; the strict wire form
includes it.

## 3. Verbs

**Meta verbs are standard HTTP** (they keep their HTTP meaning, which happens to
be exactly what goo needs — so no `GOO-` prefix):

| verb | HTTP meaning | goo job | safe | idempotent |
|---|---|---|---|---|
| `GET`  | retrieve representation | **resolve / search / list** (`;q=…` → entity or ranked choices) | ✓ | ✓ |
| `HEAD` | headers only | cheap **existence / type / count** probe | ✓ | ✓ |
| `OPTIONS` | what can I do here | **discovery + completion** (§7) | ✓ | ✓ |

**`GOO` is the default verb.** Bare `goo <thing>` / Enter / double-click →
`GOO <subject>`: resolve the subject, look up its type's `default_for` verb,
dispatch. `GET` resolves (no side effect, returns the list); `GOO` *acts* (runs
the default action). They are distinct: `GET goo://app/firefox` lists/identifies;
`GOO goo://app/firefox` launches. If the resolved type has **no applicable verb**
→ `415` (can't handle this type); applicable verbs but **no single default** (or
several claim it) → `300` (pick a verb).

**Everything else is an extension method** (the open-ended action set, à la
WebDAV's `MOVE`/`COPY`/`PROPFIND`): `OPEN`, `MOVE`, `EMAIL`, `SUMMARIZE`,
`REBOOT`, `SOLVE`, … declared by plugins.

**Verbs are case-insensitive.** The wire method (`SUMMARIZE`, uppercase by HTTP
convention) and the CLI verb (`goo summarize`, lowercase) are the *same verb*,
case-folded. So the method token carries no information the verb name doesn't.

**A verb is an abstract operation; instruments implement it.** `summarize` is
one verb that the `fabric` channel, a direct LLM, and a `duckdb` macro can each
provide — selected via `Using:` (§4), *not* multiplied into `summarize-fabric` /
`summarize-duckdb` verb names. Channels are "verbs on the *how* axis": same
`accepts → emits` typing, composed with the verb rather than enumerated against
it.

A channel may offer **sub-channels (modes)** as path segments — each a distinct
`{process}` instrument with its own `emits`: e.g. `goo://channel/fabric/inference`
(default, → the *result*) and `goo://channel/fabric/assemble` (→ an unrun
*prompt*, to hand off). These are valid `Using:` targets and are listed by
`OPTIONS goo://channel/fabric/`. This makes `Using:` (what's produced) and `To:`
(where it lands) fully orthogonal — `Using: fabric/inference  To: claude://desktop`
("run, seed Claude with the result") is expressible, where a "the `To:` decides"
rule couldn't.

But a path segment under a channel is an **instrument mode, never a verb.** The
verb is the method; to introspect a channel's params *for* a verb, **scope with
`Goo-Verb:`** — `OPTIONS goo://channel/fabric/assemble` + `Goo-Verb: SUMMARIZE`.
Folding the verb into the path (`PUT goo://channel/fabric/summarize`) would invert
the grammar — channel as request-target, subject demoted to a body — and break
references-not-data + noun→verb. **Modes in the channel path: yes. Verbs: no**
(they're the method; `Goo-Verb:` scopes discovery). Invocation always keeps the
grammar: `SUMMARIZE <subject> Using: goo://channel/fabric/assemble` — subject is
the target, channel is the instrument.

## 4. Slots, the param map, and pass-through

`Using:`/`To:`/`With:` **flatten into one parameter map** with `using`/`to`
reserved as promoted keys:

```
… Using: X  To: Y  With: a=1 b=2   ⇒   params = { using: X, to: Y, a: 1, b: 2 }
```

`To:`/`Using:` get dedicated headers for ergonomics (near-universal, carry
references, read as a sentence) but are mechanically members of the same bag as
`With:`. **The handler's OPTIONS schema assigns meaning** to each key — `email`
reads `to` as a contact; `translate` reads `to` as the target language; a verb
with no `to` concept doesn't read it.

**Resolution gradient** — how eagerly the *protocol* resolves a slot before the
handler sees it. Each ref-slot's policy is **`try | require | literal`**, declared
per verb in OPTIONS:

- **subject — MUST resolve** (it's the request-target). Inference allowed.
  Not found → `404`; ambiguous → `300`.
- **`Using:` — resolve only if the verb *requires* a channel and declares no
  default.** Absent when required-with-no-default → `422` (`Goo-Missing: Using`);
  present but unresolvable → `424` (failed dependency). Optional / has a default →
  may be omitted entirely.
- **`To:` — TRY to resolve** to an entity (or entities; may be multiple). Ambiguous
  → `300`. **On failure, fall through as the literal value** into the param map —
  `To: teague@foo.com` (no such contact) stays the literal address; `MOVE … To:
  /new/path` (doesn't exist yet) stays the literal path for the handler to create.
  A verb may set `resolve = "require"` (unresolved → `424`) or `"literal"` (never
  resolve) instead of the default `"try"`.
- **`Log:` — a second `{write}` destination**, resolved exactly like `To:`
  (try-resolve; fall through to a literal path). Same machinery, different
  *stream*: `To:` lands the **result**, `Log:` lands **diagnostic / secondary**
  output ("if you have logs, put them here"). The destination's own `{write}`
  capability does the landing — a file appends, a buffer accumulates, an `s3` /
  log-service channel ingests — so `Log:` needs **no separate instrument** for
  plain landing. CLI: `--log`. (A `Log-Using:` — a `{process}` channel that
  *transforms* the log stream before it lands, e.g. json-format or level-filter —
  is plausible by symmetry with `Using:`/`To:`, but **deferred**: logs are mostly
  produced as a byproduct of the verb's own execution and just need a sink; the
  transform case is rare and is the same `Using:` pattern applied to the log
  stream when it's actually needed.)
- **`With:` — NEVER resolved by the protocol.** Opaque `key=value`. A value MAY
  be a `goo://` URI; resolving it is the **handler's** discretion. This is the
  definition of `With:`: the catch-all manner/config slot.

An **omitted `Using:`** means the verb's declared default channel (verbs may
declare one); with no default declared, the handler's built-in path. **Repeated
`To:`** is multi-recipient (cardinality `1..*` per the verb's schema, §7).

**Pass-through is the default** (HTTP-idiomatic: servers ignore/forward unknown
headers; the schema documents the known ones). An undeclared `To:`/`Using:`/key
is deposited in the param map and the handler reads or ignores it. Verbs may opt
into **strict**:

```toml
[[verbs]]
name = "rot13"
strict = true     # reject params not in the schema → 422 Goo-Unexpected
```

So: *discover via OPTIONS to do the right thing; pass-through so you're not
punished for not discovering.*

**Two faces of the param map — loose (human) ↔ strict (canonical).** The flat
bag above is the *loose* surface: a human throws `With: depth=brief model=sonnet`
(CLI `-v`) in and the engine routes each key to its owner by the composed OPTIONS
schema. The *strict/canonical* form attaches each param to the entity it
configures, on **that entity's own address** (the [matrix-vs-query
rule](./addressing-and-protocol.md#the-uri)): channel config on the channel
(`Using: goo://channel/fabric?model=sonnet`), target config on the target
(`To: goo://buffer/log?mode=append`), subject filters on the subject. So:

- `Using:` is a **typed instrument** — its `emits` decides the *result type*
  (fabric → text, a duckdb macro → JSON). Picking the instrument picks the
  mechanism. (No separate `From:`: the provider *is* the instrument.)
- `To:` is a **typed destination** (a `{write}` domain — buffers, files, a chat).
  `emits`(instrument) ↔ `accepts`(destination) is a second type-match in the
  sentence; mismatch hard-fails (`415`) in v1 (no implicit coercion).
- `With:` is the **method's own params** — the verb is the one participant with
  no address to hang `?` on, so its params get a header. It stays **loose and
  unchecked (pass-through) by default**: the whole map reaches the handler, which
  *peels off what it understands and forwards the rest* — so a verb→channel→tool
  chain can propagate params the engine never knew about. **Handlers shouldn't
  have to know goo's internal shape** (see §11 on param-passing conventions).

**Valid `With:` keys are determined by the resolved `Using:`** (and verb): a
manner param is really an *(verb × instrument)* param — `depth` is
fabric-summarize's, which is exactly why it's meaningless for `duckdb` (not in
that implementation's schema). So `OPTIONS(verb, Using:)` is the *composed*
schema; it's **advisory** (powers GUI forms + completion), not a gate — unknown
keys 422-only-if-`strict`, else pass through. Consequence for tooling: a GUI
renders the manner form **after** `Using:` resolves (re-populating if you switch
instruments), and static `goo validate` is necessarily *partial* here — full
validity is an OPTIONS-time check, not load-time.

## 5. Header-naming convention

- **Sentence headers are bare** (part of the spoken request): `To:` `Using:` `With:`.
- **Meta/introspection headers are `Goo-`-prefixed** (about the request, not part
  of it): `Goo-Verb:` (scope OPTIONS, §7), `Goo-Missing:` / `Goo-Unexpected:` (hints).

This guarantees zero collision with standard HTTP as the meta-header set grows.
(`Via:` was rejected for the Agent slot precisely because RFC 9110 already defines
it — hence `Using:`.)

## 6. Inference, resolution, and cardinality

**Inference is one engine, used everywhere:** (a) resolve a bare subject, (b)
decide whether a loose `--with <token>` is a handler (→ `Using:`) or a flag, (c)
drive content-dispatch verb routing. Two layers feed it — **shape** (`./ ~/ /` →
file, `scheme://` → url, else → text) and **content** (libmagic / `xdg-mime` →
MIME). Inference may be uncertain → it returns **weighted choices**, and an
ambiguous inference is just a `300` ("the file, or the literal text?").

**A query subject denotes a set; resolution is a phase.** Cardinality lives in the
matrix (engine-addressing namespace; keep the explicit `q=` key so other engine
params — `mode`, `sort`, `n` — can coexist):

| matrix marker | meaning | on ambiguity |
|---|---|---|
| *(none)* — default | **resolve-to-one** | many → `300`, disambiguate (picker) or error |
| `;pick=first` (`;n=1`) | **top-ranked, no prompt** | take rank #1 (trusts the sort) |
| `;all` (`;n=*`) | **fan-out / batch** | apply to every match (explicit & gated, §9) |

- **"this match"** = act on the resolved **value** URI (e.g. `goo://app/firefox?pid=1234`).
  A launcher pick *is* this: query → `300` → pick → determined URI → verb.
- **Batch pushdown**: cardinality is in the subject, but *who iterates* is a handler
  capability advertised in OPTIONS — a handler may accept a collection/glob subject
  and batch itself; otherwise goo iterates client-side over the resolved members.
- **Sort is weighted, Kupfer-style** (relevance + optional learned usage); the
  handler computes it and reports the basis in OPTIONS. Stable weights + stable ids
  make `GET …;q=firefox` and `GOO goo://app/firefox` agree on "the first one."

## 7. OPTIONS — discovery *and* completion oracle

`OPTIONS` returns `Allow:` (the applicable verbs) plus, in the body, a **per-verb
slot schema**. Scope it to one verb with `Goo-Verb:` to drive tab-completion of a
*partial request*:

```
# user typed:  goo send my-report.pdf --using email --to alice -w<TAB>
OPTIONS goo://file/~/my-report.pdf   HTTP/1.1
Goo-Verb: SEND
Using:    goo://channel/email
To:       goo://contact/alice
→ 200  Content-Type: application/vnd.cosmic-goo.options+json
  { "with": [ {"value":"subject=","label":"Subject line","type":"string"},
              {"value":"body=","label":"Body text"},
              {"value":"from=","label":"From address"} ] }
```

Full schema shape:

```jsonc
OPTIONS goo://file/~/report.pdf
→ 200  Allow: GOO, OPEN, EMAIL, PRINT, MOVE
  { "EMAIL": {
      "subject": { "accepts": ["application/pdf","*/*"] },
      "to":   { "accepts": ["application/vnd.*.contact"], "card": "1..*",
                "resolve": "try" },                 // try | literal — see §4
      "using":{ "accepts": ["application/vnd.*.mailer"], "required": false },
      "with": { "subject": {"type":"string"} },
      "confirm": "never", "destructive": false, "strict": false, "sort": "relevance" } }
```

Two payoffs: every `<TAB>` is an OPTIONS introspection (returning labeled choices);
and **this slot schema *is* an MCP tool's `inputSchema`** (subject/to/using/with →
params; `destructive`/`confirm` → annotations) — so a goo↔MCP proxy is mechanical.
Clients cache `OPTIONS(resource)` + `OPTIONS(channel)` and complete locally; only
dynamic slots (contact search) need a live call. `Using:`/`To:` params are
discovered recursively: `OPTIONS goo://channel/fabric` returns fabric's own params.

## 8. Choices & entities — `{id, label, weight}`

The universal entity shape, used for search results, adverb enum values, channels,
verbs — everything:

```
id     = goo:// URI, locally-unique (pid-qualified: goo://app/firefox?pid=123)   — canonical identity
label  = human display string                                                     — first-class; defaults to id if absent
weight = sort score (Kupfer-style)                                                — ranking
```

```jsonc
// structured (canonical)
GET goo://app/;q=firefox  →  300 Multiple Choices  application/vnd.cosmic-goo.choices+json
[ {"id":"goo://app/firefox?pid=123","label":"Firefox — Reddit (Display 1)","weight":0.91},
  {"id":"goo://app/firefox?pid=5345","label":"Firefox — Reddit (Display 2, min.)","weight":0.77} ]
```
```
// compact form: url-encoded id = url-encoded label, one per line
goo://app/firefox?pid%3D123=Firefox%20%E2%80%94%20Reddit%20(Display%201)
```
Plus a transient **local index** (1, 2, 3…) for terse selection when echoing ids
isn't worth it. **Locally-unique ids beat a resolution cache** for consistency: a
follow-up action targets the same entity by id, so caching is a pure optimization.
Labels being first-class extends `[[types]]`'s `display` to verbs, adverb-values,
and channels.

## 9. Status codes & safety

**Reuse standard HTTP numeric codes — never mint custom numbers** (a `6xx` breaks
curl / proxies / caches, which defeats "literally HTTP"). Carry goo-specific
precision in the **reason-phrase + `Goo-*` headers**, not the number.

| status | meaning |
|---|---|
| `200` | resolved / acted; representation or result in body |
| `300 Multiple Choices` | **subject** ambiguous — `Alternatives:`/body lists `{id,label,weight}` |
| `301` | search resolved to one canonical reference (`Location:`) |
| `404 Not Found` | **subject** not found (certain) |
| `415 Unsupported Media Type` | no verb handles the subject's **type** ("don't know how to handle this") |
| `406 Not Acceptable` | handler can't produce a representation matching the client's `Accept:` (output negotiation) |
| `422 Unprocessable` | request **incomplete** — a *required* slot absent (e.g. required `Using:`, no default) — `Goo-Missing: Using` (+ `Link: <…>; rel="describedby"` → OPTIONS). *(`400` if you prefer the blunt code.)* |
| `424 Failed Dependency` | a **header reference** (`To:`/`Using:`/a `With:` `goo://` value) was present but **didn't resolve** — `Goo-Unresolved: To`. Absorbs "channel not found" (no separate `421`). |
| `428 Precondition Required` | verb needs **confirmation** — resend with a confirm token |

Note the absent-vs-unresolvable split: a *missing required* slot is `422`; a
*present-but-unresolvable* reference is `424`.

**Safety is declared in the verb TOML** (not derived from idempotency), surfaced in
OPTIONS, and mapped to MCP annotations:

```toml
[[verbs]]
name = "close-windows"
confirm = "multiple"    # always | multiple (only on fan-out >1) | never
destructive = true      # warning UI; blocks silent ;pick=first; → MCP destructiveHint
```

Interactive clients prompt locally from this metadata before sending; non-interactive
clients get `428` until they resend with confirmation.

## 10. Worked examples — the inspirations mapped to goo://

**gnome-do / Quicksilver** (noun → verb → indirect object)
```
GET   goo://app/;q=firefox            → 300 → GOO goo://app/firefox     # "firefox" → Run (default)
MOVE  goo://file/~/Downloads/doc.pdf    To: goo://file/~/Archive/        # "Move To…" (locative)
EMAIL goo://file/~/report.pdf           To: goo://contact/;q=alice       # "Email To…" (recipient, resolves)
                                        To: goo://contact/;q=bob         #   repeated To: = multi-recipient (card 1..*)
```

**Kupfer** (object → action → indirect object; the indirect object is searchable)
```
SEARCH goo://sel/        Using: goo://engine/duckduckgo                  # "Search With…" (instrument → Using)
MOVE   goo://window/current  To: goo://workspace/;q=code                # proxy object + searchable target
```

**ulauncher / pop-launcher** (primary + secondary actions)
```
# Enter / Activate = GOO (default action). Alt+Enter / Context = the other verbs:
OPTIONS goo://app/firefox  → Allow: GOO, OPEN-NEW-WINDOW, OPEN-PRIVATE, QUIT, COPY, REVEAL
GET goo://calc/2%2B2*3     → 200 "8"                                     # instant answer (safe, cacheable)
```

**fabric** (pattern-as-verb; piping; output routing = `To:`)
```
SUMMARIZE goo://clip/                   Using: goo://channel/fabric                     # wl-paste | fabric -p summarize
THINK     goo://url/https://example.com Using: goo://channel/fabric?model=iq4xs  With: lang=es   # -u … -p extract_wisdom -v lang=es
SUMMARIZE goo://file/~/article.md       Using: goo://channel/fabric  To: goo://chat/new          # summarize → send result to a chat
```

**Pass-through / target-value `To:`**
```
TRANSLATE goo://text/Hello   To: spanish          # "to" fails entity resolution → literal "spanish" → handler reads as lang
ROT13     goo://text/Hello   To: goo://contact/x  # lenient: 'to' ignored | strict: 422 Goo-Unexpected: To
```

## 11. The engine/handler boundary — marshalling & buffers

The engine speaks `goo://` to itself; it speaks **bytes and paths** to the world.
The edge between is a **marshalling boundary** (an FFI/serialization seam): goo's
internal representation — references, the subject JSON, and **buffers** — is
translated into a handler's native form before it crosses, and **handlers never
bleed goo internals**.

### Buffers — the materialization primitive

A **buffer** is goo's `mktemp`: it turns *data* into a *reference*
(`goo://buffer/<id>`). It closes the references-not-data thesis — inline/produced
data (a coercion output, an LLM result, a blob) is the exception, and
materializing it to a buffer makes it a reference again, so the system stays
reference-based **end-to-end, even through data-producing stages.** Buffers are:

- **an address for the unaddressable** — give produced data a `goo://` handle;
- **the by-reference path for too-big/binary data** — pass `goo://buffer/abc`, never bytes-in-a-URL;
- **the file-path lingua franca** — tools that exchange via *file paths* (ffmpeg, duckdb, image tools) read/write a buffer's backing file;
- **coercion wires** — the intermediate stages of a `csv→json→sql` chain land in buffers.

Buffers are **typed** (`{read, write}` of a held MIME — writing tags, reading
yields a typed subject), with two lifecycles, like temp vs named files:

- **ephemeral** (engine-owned): auto-named, request-scoped, GC'd after the route; backed by a temp file (CLI) or memory (the `good` daemon).
- **named/persistent** (user-owned): `^scratch`, `goo://buffer/notes`; survives; `?mode=append` accumulates. `^` is the degenerate *unnamed* clipboard buffer.

The engine may **auto-insert** a buffer into a route (between a producer of data
and a consumer needing a reference/file) — the same way it auto-inserts a
coercion on a type gap — preferring streaming/by-reference, materializing only
when *forced* (size / a file-only consumer / no address), GC'd after, and
**visible in the planned route** (like a query plan), not silent.

### The no-leak rule

**`goo://buffer/<id>` is internal. It never crosses to a non-goo-native handler —
only its *materialized content* does.** At a foreign handler the engine:

1. **derefs in** — a buffer-valued param becomes the tool's native form: a real **temp file path**, **bytes on stdin**, or an **env var**;
2. runs the tool;
3. **re-buffers out** — the tool's stdout / the file it wrote is re-internalized into a fresh buffer with a new `goo://` ref.

So the engine *wraps* foreign tools (deref in, re-buffer out); the `goo://` string
never reaches them. The temp file handed across is **request-scoped** (reaped
after the run); the internal buffer lives on its own clock. Named buffers don't
change this — `^scratch` is *user-addressable* but still *goo-only-meaningful*; a
foreign tool reading it gets a materialized snapshot, not the handle. (Same
hygiene as never handing a raw fd or internal pointer across an FFI seam: foreign
tools can't forge/probe ids or couple to goo internals, and goo owns cleanup.)

### Param-passing conventions

*How* the engine presents params to a handler's command — and therefore how a
buffer is materialized — is a per-verb/channel **convention**, so handlers stay
goo-agnostic:

- **template** (today): the `cmd`/`prompt` `{var|filter}` form — the author references goo's shape; least goo-agnostic, fine for goo-aware plugins.
- **env**: goo sets `GOO_SUBJECT_PATH` / `GOO_WITH_MODEL` / …; the tool reads env like anything else.
- **argv**: params as flags/positionals.
- **stdin-JSON**: pipe the param map; the handler `jq`s it.

The convention *also* picks the buffer-materialization form (file-consumer → temp
path; stdin-consumer → bytes; …). **env/stdin keep the tool most goo-agnostic.**
This is the loose pass-through map (§4) flattened to the handler's surface.

### goo-native handlers opt out

A handler that **speaks `goo://`** (another `goo` process, the `good` daemon, a
plugin sharing the buffer service) declares itself **goo-native** and receives
**references** — no materialization, buffers passed by handle:

> **Default = foreign → marshal (deref buffers, flatten params). Opt-in
> `goo-native` → pass `goo://` refs as-is.**

Open: *how* goo-native is declared — per-verb, per-channel, or a property of the
wrapped tool (a plugin wrapping `fabric` is foreign; one wrapping a `good`-speaking
peer is native). Likely a flag (`speaks = "goo"`).

## 12. Presentation negotiation — `Accept:`, the inherited channel, and `From:`

The same subject renders differently depending on who's looking: an image is
bytes to a file, a GUI window on a desktop, ANSI inline in a bare terminal. This
is **content negotiation**, and it's what makes goo *not* a typed `xdg-open`
wrapper — `xdg-open` is type→handler dispatch with no notion of "what can the
caller render," so it structurally can't do "show this image as ANSI because I'm
a terminal."

### `Accept:` is the mechanism; the inherited channel is the default destination

A caller declares what representations it can take with **`Accept:`** (HTTP-exact).
The engine negotiates the handler's emitted representation against it. And the
load-bearing rule for *where the result goes*:

> **The default destination is the inherited channel** — the stdout/fd the request
> came in on. Absent an explicit `To:`, an unsolicited result is presented back
> over the channel that carried the request.

So negotiation is never left dangling at "the engine picked a representation" with
no answer to "and sent it *where*": unsolicited output goes back the way it came.
This is also where the **`-` convention** lives: **`-` means "the inherited
channel."** As a *subject* that's stdin (the established Unix idiom); as a
*destination/route* it's the inherited stdout/fd; as a **selector-adverb default**
(`default = "-"`) it's "the route that fits the inherited channel." One token,
coherent across slots.

### The common path: a bare CLI gets a default `Accept:` from a thin heuristic

A human typing `goo view photo.png` will not type `--accept text/x-ansi`. So the
**bare-CLI path is the dominant one**, and it works by *synthesizing* a default
Accept from the environment: a quick `isatty(1)` + `$WAYLAND_DISPLAY` check — tty
and no display → prefer `text/x-ansi`; a display → prefer a GUI handoff. That
heuristic is the whole of what's needed for ~90% of invocations; explicit
`Accept:` (scripts, launchers, an MCP proxy) is the *secondary* path that
overrides it.

The synthesized Accept is a **preferred presentation, not a hard constraint.** A
tty *prefers* ANSI but can still take image bytes — a file redirect, a sixel
terminal. So: **the heuristic governs only the *unsolicited default*; an explicit
`To:` — including a shell redirect to a file — displaces it.** `goo view photo.png > out.png`
has a non-tty stdout (an explicit byte sink), so the engine sends image bytes —
*by the model*, not by luck.

### Accept drives `Using:` — negotiation, not enumeration

Given the negotiated Accept and the handler's emitted representation:

- **emit ∈ Accept** → present directly;
- **gap** → the engine inserts a **`Using:` instrument** that bridges it. `chafa`
  is `image/* → text/x-ansi`; it's auto-selected exactly when the caller prefers
  ANSI but the handler emits image bytes.

This is the **output-side dual of §6's input inference**: input infers the
*subject's* type; output negotiates the *caller's* presentation. The same
weighted-candidate machinery applies (the engine ranks bridging instruments),
keyed on the Accept profile instead of a destination's `accepts`.

`chafa`-as-instrument is the tell that **this needs no bespoke mechanism**.
`view`/`play` are not env-special verbs — they're ordinary verbs that emit a
representation; *negotiation* picks the route. "Terminal vs GUI" is a `Using:`
instrument selected by Accept, overridable with an explicit `To:`/`Using:`.

### The CLI surface — `--as`, `--to`/`--on`, `--using`

The two negotiation defaults each have an explicit override, plus the instrument:

| flag | slot | overrides |
|---|---|---|
| `--as <type>` | `Accept:` | the synthesized Accept — "give it to me **as** text / ansi / json" |
| `--to` / `--on` `<resource>` | `To:` | the inherited-channel destination |
| `--using <x>` | `Using:` | (no default — names the instrument) |

**`--on` is `--to`; the difference is a capability of the *target*, not a second
slot.** A destination is a resource, and what arriving *means* is decided by that
resource's capability — keeping the whole thing type-driven rather than slot-driven:

- `--to ~/copy.png` → the file's `{write}` capability **stores** bytes;
- `--on goo://display/0:1` → the display's `{present}` capability **shows** it.

`--on` is just the preposition for when the destination is a *surface* not a
*sink* — English following the capability. **Don't add a slot for what's a
capability distinction on a resource** (cf. the `User-Agent` discipline above).

This is why a fully-specified sentence isn't "deliver bytes to a display":

```
goo open image.png --using firefox --on display:0:1
  OPEN   goo://file/image.png
  Using: goo://app/firefox
  To:    goo://display/0:1        # a {present} target, not a {write} sink
```

You aren't writing bytes *to* display 0:1 — the display's `{present}` capability
plus the named presenter (`firefox`) means the engine **wires `DISPLAY=0:1` into
firefox at the marshalling boundary (§11)**. "to a file" and "on a display" are
one slot; the *target's capability* (and any named instrument) decide what
crossing the boundary means.

### `From:` — when the caller is more than its inherited channel

Two things `Accept:` alone can't express, and only these earn the full origin
model:

1. **A return address that isn't the inherited fd.** A launcher saying "render the
   result into *me*, not the terminal that spawned me" must *name* itself — an
   address, not a content type. That's `From: goo://session/current` (or an
   explicit `To:` the caller supplies), used as the destination.
2. **Branching on origin *kind*.** "What am I called from" is a kind, and kinds are
   the existing ontology (kind-first, [addressing-and-protocol.md]): `goo://session/*`
   with an `is_a` hierarchy — `session/{real-tty, ssh, tmux, script, launcher}`,
   each `is_a session/{interactive, batch}`. This is just a **richer version of the
   thin Accept heuristic** (a `tmux`-in-cosmic-terminal has a display the immediate
   pipe hides; an `ssh` session may have none; a `script` wants raw data). Detected
   from `isatty`/`$WAYLAND_DISPLAY`/`$SSH_TTY`/`$TMUX`/`$TERM`/parentage; *sortable*
   the way a source's items are.

If results always flow back over the inherited fds, **`From:` isn't needed at
all** — `Accept:` + the inherited-channel default cover everything, and the
session-kind ontology is earned only when render-into-a-named-surface becomes
real.

> **`From:` is goo's `User-Agent` — so don't sniff it.** The web spent two decades
> learning that branching on client *identity* (User-Agent strings) is a trap:
> every client ends up lying to pass the checks, and the mechanism rots. The
> principled replacement it converged on is *capability* negotiation — `Accept`,
> Client Hints, CSS media features (`pointer: coarse`, not "is it an iPhone").
> goo is type-driven from the start for the same reason. A session **kind** is a
> **named bundle of capabilities, not an identity to special-case**:
> `session/launcher` is sugar for `{has display, wants render-into-self}`, the way
> `pointer: coarse` names a capability rather than a device. Negotiate on what the
> caller *accepts*; reach for the kind label only as shorthand for a common
> capability set, never as a thing to branch on. (This is why the kind ontology is
> a *refinement of the Accept heuristic* above, not a parallel identity mechanism.)

### Status

Designed-not-built. Negotiation proper is gated on **coercion** (§13 — `chafa` as
an auto-routed `image→ansi` channel); the named-return-channel and kind ontology
are a later refinement. The minimal *first* step that needs neither: a **selector
adverb whose default is `-`** (environment-computed → `terminal` / `gui` by the
`isatty`/`$WAYLAND_DISPLAY` heuristic) — env-aware `view`/`play` today, promotable
to full negotiation when coercion lands. This is the consumer that *motivates*
coercion on the **output** side, exactly as JSON motivated `infer_for` on the
**input** side.

## 13. Deferred / open

- **Multi-subject ("comma trick")** — multiple *direct objects*, `EMAIL a.pdf, b.pdf`
  (distinct from multi-`To:`, which is repeated *indirect* objects and is already
  in-scope via `card 1..*`). Single-subject for v1; fan-out via `;all` covers the
  common batch case; explicit multi-subject TBD.
- **Unnamed context bag** — reserved `With: ref=goo://…` (repeatable) if a
  bag-of-related-entities need emerges; no dedicated header for now.
- **OPTIONS narrowing beyond `Goo-Verb:`** — e.g. partial `With:` validation.
- **`Via:`-style multi-hop `Using:` chain** (SIP stacks `Via:`) — not needed yet.
- **`Log:`** — now a first-class slot (§2/§4): a second `{write}` destination for
  diagnostic/secondary output. `Log-Using:` (a transform channel for the log
  stream) remains deferred — see §4.
- **Type system, inference & coercion (the in-progress major arc).** The slot
  model already *accommodates* data-sink/transform endpoints — `To: goo://s3/bucket/key`,
  `Using: goo://channel/sql-import`, a custom server channel — because they're
  just `{write}`/`{process}` domains. **Built** (Rust engine): the subtype lattice
  (`is_subtype` — glob + structured-suffix + `is_a`) and **input inference**
  (`infer_for` — JSON-shape → type, weighted, re-ranked by the verb's `accepts`).
  **Still missing — coercion**: when `emits`(instrument) ≠ `accepts`(destination),
  do we hard-fail (`415`, v1) or insert an **implicit coercion channel** (json→sql
  rows, csv→json, text→bytes, *image→ansi*)? Coercion channels would be ordinary
  `{process}` domains the engine *auto-routes through* on a type gap. This unlocks
  "send this JSON to a SQL table / an S3 bucket / a custom server" cleanly — **and**
  the output-side presentation negotiation of §12 (`chafa` as an `image→ansi`
  coercion channel, auto-routed for a tty origin). Two consumers, one mechanism.
  Big; designed-not-built; the slot model is ready, the type system is partway.
  (Buffers — the materialization primitive that carries coercion intermediates
  and data-with-no-address — are now in §11.)
