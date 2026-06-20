# The `:contact` domain — a worked spec for per-instance facets

> **Status: built** as [`doc/examples/contacts.toml`](https://github.com/teaguesterling/cosmic-goo/blob/main/doc/examples/contacts.toml)
> (`-c`-loadable, vCard-dir backend via `$GOO_CONTACTS_DIR`). The contact is goo's
> **multi-facet exemplar** — the candidate that most stress-tests the membership model
> the file-vs-data work shipped, and building it surfaced the last gap in that model
> (see *What building it taught us* below). The per-instance, data-driven facet mechanism
> it exercises generalizes far beyond contacts.
>
> **What building it taught us.** (1) The capability-facet *action* verbs (`email`/`call`)
> initially didn't dispatch — `needs_coercion` (the exec router) checked only the primary
> `type`, not membership, so a facet-accepted verb was wrongly sent through the
> coercion/presentation pipeline (which threads the subject as bytes and discarded its
> fields). Making `needs_coercion` membership-aware — the one acceptance site the original
> file-vs-data work missed (masked because files have a `metadata.path`) — fixed it; the
> facet design now works end-to-end. (2) The display verb is `card`, not `show`: `show`
> already collides across git/clipboard, and verb-first dispatch of a name with multiple
> impls used to resolve to the first-registered, 415-ing on the others — a separate
> pre-existing gap this surfaced, since **fixed** (`verbs::lookup_subject` re-selects the
> impl by the resolved subject). The name stays `card` on the merits: a contact card is
> its own thing, not something you `git show`.

## Why a contact is the interesting case

A file's two natures are static: every regular file is both `inode/file` and its content
type, so the engine mints the `inode/file` facet unconditionally in `resolve_file`. A
**contact is different**: it is emailable *only if it has an email*, callable *only if it
has a phone*, messageable *only if it has an SMS/IM handle*. Its capabilities are a
property of **its data**, not its kind — so the membership must be minted **per instance**,
and it must be minted by whoever reads the data: the **source**.

This is the design question from `expansion-candidates.md` made concrete:

> *What accept-patterns does this entity claim — and does that inherit exactly the verbs
> that make sense?* — except here the answer is **different for each contact**.

**Verified:** the engine already supports this. `address::resolve_source` builds a subject
by cloning the source item and tagging its `type` (`tagged`, address.rs), so any `_facets`
a source's `list_cmd` emits **survive into the subject** and are honoured by
`verbs::subject_types` at accept-matching — exactly like the engine-minted file facet.
Files mint facets in Rust; contacts mint them in a `list_cmd`. Same mechanism, two
sources of provenance.

## The model

A contact's primary type is an opaque handle; its **capabilities are facets**, each a tiny
type that exactly one verb-family accepts. The source decides, per contact, which it
claims.

```toml
# contact.toml  (tier = productivity — needs a CardDAV/vCard backend)

[[types]]
name = "application/vnd.goo.contact"
display = "contact"
kind = "handle"            # a person/org reference; NOT text, NOT a file (see the guard below)

# Capability facets — each is the accept-pattern of one verb. A contact claims the
# ones its data supports. These are membership types, not the contact's `type`.
[[types]]
name = "application/vnd.goo.emailable"
[[types]]
name = "application/vnd.goo.callable"
[[types]]
name = "application/vnd.goo.messageable"
```

```toml
[[sources]]
name = "contacts"
prefix = "contact"          # :contact:alice  (fuzzy)   :contact/<uid>  (exact)
emits = "application/vnd.goo.contact"
# khard (CardDAV via vdirsyncer) is the reference backend; a ~/.contacts/*.vcf parse is
# the dep-light fallback. Per contact, emit the capability facets its fields support, and
# stash the raw field values in metadata for the verbs to read.
list_cmd = '''
  khard ls --parsable 2>/dev/null | jq -Rsc '
    split("\n") | map(select(length>0) | split("\t") | {
      id:        .[0],                      # stable uid (the addressable locator)
      title:     .[1],                      # display name (Tainted at the surface)
      metadata:  { email: .[2], phone: .[3] },
      _facets: ( []
        + (if (.[2] // "") != "" then ["application/vnd.goo.emailable"]    else [] end)
        + (if (.[3] // "") != "" then ["application/vnd.goo.callable",
                                        "application/vnd.goo.messageable"] else [] end) )
    })'
'''
```

Now the verb list **adapts to the contact**: `goo what :contact:alice` lists `email`/`call`/
`message` for a fully-populated contact, but only `email` (and `show`) for one with no
phone — because the `callable`/`messageable` facets aren't claimed, so those verbs don't
match. That adaptivity is the whole point, and it falls out of the membership model for
free.

## The verbs — mostly thin, mostly coercions to URI schemes

The key realization (design principle #4 from `expansion-candidates.md`): a contact's
actions are **URI-scheme builders that ride the `text/x-uri` bus into `open`**, not
bespoke command-runners. `email` → `mailto:`, `call` → `tel:`, `message` → `sms:`. `open`
already accepts `text/x-uri`, so the heavy lifting is a **coercion channel**, not a verb
that re-implements launching.

Two ways to wire it, both valid:

- **(a) Verb + scheme, riding open.** `email` accepts `emailable`, builds
  `mailto:{metadata.email|uri}`, and the result coerces to `text/x-uri` → `open` hands it
  to the MUA. The verb is three lines; the URI-encoding (`|uri`, which already exists) is
  the safety. `call`/`message` mirror it with `tel:`/`sms:`.
- **(b) Pure coercion.** Declare `emailable → mailto: text/x-uri` as a `[[channels]]`
  converter; then *no `email` verb is needed* — `goo open :contact:alice` negotiates
  through it. But losing the verb name costs discoverability (`goo email …` reads better
  than `goo open …` for a person), so **(a) is preferred** and (b) is the fallback for
  composition.

Genuinely-new (non-coercion) verbs, small in number:

| Verb | Accepts | Action | Notes |
|---|---|---|---|
| `email` | `emailable` | `xdg-email {metadata.email\|q}` (or `mailto:` → open) | the bus path is preferred |
| `call` | `callable` | `tel:{metadata.phone\|uri}` → open | dialer / KDE-Connect handler |
| `message` | `messageable` | `sms:`/IM scheme → open | |
| `show` / `vcard` | `application/vnd.goo.contact` | render the card | `show` is **polymorphic** — add a contact impl that joins the existing family |
| `email` *(two-step)* | `inode/file` / `text/*` | `xdg-email --attach {subject…} --to {object…}` | "email **this file** to Alice": `object_type = application/vnd.goo.contact`, `object_source = contacts`. This is the path that unblocks the deferred `email --to alice` design in `goo-protocol.md` §10 |

So the contact domain is roughly **1 source + 3 facet types + ~4 verbs (2 of them thin
scheme-builders) + 1–3 coercion channels** — Bucket B, real but bounded.

## Provenance guard — what a contact must NOT claim

Run the same check that keeps clipboard-CSV from gaining file verbs:

- ✅ Claims `emailable`/`callable`/`messageable` **per field** — a phone-less contact
  isn't callable, so `call` correctly doesn't appear.
- ❌ Must **not** claim `text/plain`/`text/*` — a person is not prose; `upper`/`sha256`/
  `wc`/`summarize` on a contact is nonsense. (The *email string* is text, but that's a
  field the verbs read via `metadata`, not the contact's identity.)
- ❌ Must **not** claim `inode/*` — a contact is not a file; no `open`/`reveal` on the
  person. (The `mailto:` it coerces to is `text/x-uri`, a different thing that *does* ride
  `open` — that's the coercion, deliberately, not a membership.)

The narrowness is the design: claim only the capability facets, let scheme-coercion reach
`open`, and the wrong verbs never appear.

## Security

Contact data is **untrusted external input** (synced from CardDAV/a shared server), so the
same disciplines the codebase already has apply:

- **Display** — names, emails, org strings flow as `Tainted` and render via `.sanitized()`
  at every surface (the picker, `what`, the confirm prompt). The wrapper already exists.
- **Shell** — any field reaching `bash -c` (`xdg-email {metadata.email}`) is `|q`-quoted;
  a field reaching a URI is `|uri`-encoded (a contact "email" of `a@b?cc=evil` must not
  inject mailto headers). Both filters exist.
- **Source-emitted facets — the trust boundary.** A source's `_facets` are the *first*
  channel by which `list_cmd` output (often untrusted external data) can change which verbs
  match — `type` itself is locked to the static `emits` and can't be injected. A source that
  let external data claim a **bus** facet (`inode/file`, `text/*`) would grant inappropriate
  verbs, and a forged membership reaches whatever verbs accept that type — not all of which
  are guaranteed injection-clean (a real `read`/`preview` command-injection was found and
  fixed while analyzing this — `f3450fd`). **Resolved → a per-source declared facet
  allowlist** ([facet-trust-boundary.md](facet-trust-boundary.md)): a source declares
  `facets = […]` and the engine drops any emitted facet outside it. This contact source
  declares its three capability facets; `emailable`/`callable`/`messageable` are safe and
  enumerated, `inode/file` can never be claimed.

## Decisions / open questions

1. **Multiple emails/phones.** A contact with `work` + `home` email: pick the preferred
   (vCard `PREF`) for the one-shot, or make `email` two-step with `object_source` = the
   contact's own addresses. Recommend: preferred-by-default, `--field=work` adverb to
   override.
2. **The default verb.** `goo :contact:alice` (bare) → `default_for`. A contact's default
   is ambiguous (email? call? show?). Recommend **`show`** (non-destructive, always
   applicable) as `default_for application/vnd.goo.contact`.
3. **Backend dep.** khard (+ vdirsyncer) is the reference; a `~/.contacts/*.vcf` parser is
   the fallback so the domain isn't hard-gated on a sync stack. Both are productivity-tier;
   neither belongs in core.
4. **Org vs person.** Orgs lack a phone often → naturally fewer facets; the model handles
   it with no special case.

## Why this spec matters beyond contacts

The per-instance facet pattern proven here is the template for every **capability-shaped**
entity: a `:pass` entry is *copyable* + *otp-capable* (only if it has an `otp` field); a
`:disk` is *mountable* xor *unmountable* by its current state; a `:repo` is *pushable* only
if it has a remote. Each is "claim the facet iff the data supports it, in the `list_cmd`."
Contacts are simply the case where it's most visible — which is why they were worth
speccing first.
