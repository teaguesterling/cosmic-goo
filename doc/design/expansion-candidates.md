# Expansion candidates — entities to add, and how they integrate

> **Status: design review only.** A read of the launcher inspiration landscape
> against goo's current surface, producing a *cost-honest* candidate list of types,
> verbs, sources, and mechanisms — organized by **how they plug in**, not by domain.
> Nothing here is implemented. The integration discipline is the contribution; the
> wishlist is the easy half.

## Why this exists

goo's inspirations — Kupfer (the trinity: noun → verb → indirect object), Plan 9
plumber (content-type dispatch), GNOME-Do/Quicksilver (the three-pane compose model),
rofi/wofi/fuzzel (fuzzy pickers), and the native cosmic-launcher (pop-launcher) — each
expose object types and actions goo could adopt. `doc/launcher-landscape.html` already
surveys *what's missing* per category. This doc answers the harder question the survey
doesn't: **for each candidate, what does it cost to integrate, and through which of
goo's existing seams?**

## Baseline (current surface)

~31 plugins · **20 source domains** (`app br bt clip ctr emo file hist mnt net ps recent
repo sel sink ssh svc tmux win ws`) · 20 declared types · ~80 verbs · 5 adverbs · 5
coercion channels. Capability gaps vs the launcher field (from `launcher-landscape.html`):
browser bookmarks/history, snippets/text-expand, notes/scratchpads, calendar/contacts/mail,
password-store/keyring, brightness/display, color picker.

## The integration model (the spine)

A goo subject holds a **type** plus, optionally, additional **memberships** (`_facets`)
— the file-vs-data work established that a subject can belong to several types at once
and **inherit a verb's vocabulary through any membership it claims** (a file is both
`inode/file` and its content type; accept-matching scores a verb over `subject_types` =
type ∪ facets). Inheritance flows through the `is_a` lattice + RFC-6839 structured-suffix
(`mime::is_subtype`); polymorphic verbs dispatch by `(name, accepts)` and pick the
most-specific impl; `emits≠accepts` is bridged by **coercion channels**; the
bare-address default verb comes from `default_for` over the membership.

So the **one question** every candidate must answer — the same provenance-guard
discipline that keeps clipboard-CSV from gaining file verbs:

> *What accept-patterns does this entity claim membership in — and does that inherit
> exactly the verbs that make sense, while NOT exposing verbs that don't?*

Four accept-patterns act as **free vocabulary buses** (verified against the shipped
`accepts`):

| Bus (accept-pattern) | Verbs a member inherits for free |
|---|---|
| `inode/*` | `open` `reveal` `copy-path` `read` `preview` `tree` `size` |
| `text/x-uri` | `open` (URL → default app) |
| `text/plain` / `text/*` | `search` + ~30 text utilities (`summarize` `critique` `think` `upper` `lower` `wc` `sha256` `md5` `qr-encode` `base64-*` `json-*` `url-*` …) |
| `image/*` | `view` `ocr-image` |

By contrast `connect` `info` `copy` `status` `logs` `stop` are **polymorphic per-type**
(each impl `accepts` one vendor type) — they are *not* a free bus; a new "connectable"
or "copyable" must ship its **own** impl of that verb name (which then composes into the
existing polymorphic family). Knowing which verbs are buses vs per-type is the difference
between "1 type, 0 verbs" and "1 type + N verbs."

---

## Bucket A — New **types** that ride existing verbs (cheapest: a `[[types]]` + `is_a` + a source)

These add an addressable domain and inherit their whole vocabulary from a bus. Cost is a
type declaration, an `is_a` edge, and a `list_cmd` source — **zero new verbs**.

| Candidate | Claims membership in | Inherits (free) | Still needs | Inspiration |
|---|---|---|---|---|
| **`:bookmark`** (GTK `~/.config/gtk-3.0/bookmarks`, Firefox `places.sqlite`) | `text/x-uri` | `open` | (opt.) `edit`/`delete` handle verbs; a fetch channel to summarize the page | Spotlight, KRunner, Raycast |
| **`:snip` / `:note`** (a notes dir, named buffers) | `text/plain` **and** `inode/file` (when file-backed) | all text utilities + `open` `read` `reveal` | (opt.) `new-note` `append` | Alfred, Raycast, Tomboy |
| **`:recent-url` / browser history** | `text/x-uri` | `open` | a history source (sqlite) | universal |
| **`:font` / `:icon`** (fontconfig, icon themes) | `inode/file` (the file) | `open` `reveal` `copy-path` `preview` | (opt.) `set-as` | — |
| **`:trash` entries** (`~/.local/share/Trash`) | `inode/file` | `open` `reveal` | `restore` `delete-permanently` (new) | KRunner |

**Discipline note (the trap):** a `:bookmark` is tempting to also declare `is_a
text/plain` so it rides `summarize`/`upper`. Don't — its *URL string* isn't prose; that
would expose `upper`/`sha256`/`wc` on a bookmark, which is noise. Claim **only**
`text/x-uri`. "Summarize this bookmark's page" is a *coercion* (`text/x-uri → fetch →
text/html → text/plain`), not a membership — it belongs to a channel, not an `is_a` edge.
This is exactly the clipboard-vs-file guard, applied to a new entity.

---

## Bucket B — New **sources** bringing genuinely new **verbs** (real surface)

Here the entity's actions don't exist yet, so the cost is a type **plus** N verbs. The
multi-facet candidates are the interesting ones — each facet routes a *different* verb.

- **`:contact`** (khard / vdirsyncer) — the canonical multi-facet entity, **drilled to a
  full spec in [contact-domain.md](contact-domain.md)**. A contact is-a *emailable* +
  *callable* + *messageable* + *addressable*. New verbs: `email`
  (two-step: object = the message/file to send), `call`, `message`, `vcard`. Integration
  choice to make explicit: model each capability as a **facet membership** (`is_a
  application/vnd.goo.emailable`, etc.) so a new `email` verb that `accepts` the emailable
  facet matches — *and* let the contact's email field **coerce** to a `mailto:`
  `text/x-uri` so it rides `open` into the MUA for free. Run the guard: a contact should
  NOT claim `text/plain` (no `upper` on a person). **Cost: 1 source + 3–4 verbs + facet
  types.** Partly unblocks the existing `email --to alice` design (goo-protocol §10).
- **`:pass` / `:totp`** (password-store) — new verbs `copy` (polymorphic impl: to clip,
  auto-clearing — reuses the destructive/`Tainted` display discipline), `otp`, `reveal`
  (guarded). Security-sensitive: secrets must flow as `Tainted`, never logged, clipboard
  auto-cleared. **Cost: 1 source + a `copy` impl + `otp`.** This is where the
  `Tainted`/redaction work pays a second dividend.
- **`:wifi`** (`nmcli device wifi`) — distinct from `:net` saved connections. New verb
  `scan`; rides the **polymorphic `connect`** family by shipping a `connect` impl
  accepting `application/vnd.nm.wifi` (composes with bt/ssh/net connect). `forget`. **Cost:
  1 source + `scan` + a `connect` impl.**
- **`:disk` / `:usb`** (udisks2) — `mount`/`unmount` (unmount exists) `eject`. Mostly a
  source + `eject`; `unmount` already accepts the mount type. **Cost: 1 source + `eject`.**
- **`:cal`** (khal) — events. New verbs `agenda`, `add-event`; a meeting-URL field coerces
  to `text/x-uri` → rides `open`. **Cost: 1 source + 2 verbs.** Bigger if write-back.
- **`:mail`** (notmuch) — threads. New verbs `reply` `archive` `read` (polymorphic) + a
  `:mail` `search`. **Bigger lift** (notmuch dep, stateful).

---

## Bucket C — New cross-cutting **verbs** on **existing** types (fill the Kupfer file-verb gap)

No new domain — just verbs the inspirations have and goo lacks, on types that already
exist.

- **`move` / `delete` / `trash` / `copy-file`** on `inode/*` — Kupfer's core file verbs.
  goo has `open/reveal/copy-path/read/rename` but not these. **Destructive ⇒ `confirm =
  true` + `Tainted` display of the path.** `move`/`copy-file` are **two-step** (`object_type
  = inode/directory`, `object_source` = a directory enumerator). `trash` → `gio trash`.
- **`set-wallpaper`** on `image/*` (`cosmic-bg` / cosmic-config) — rides the image bus to
  appear, adds one verb + a cosmic-config write. Pairs with a **brightness** verb
  (adverb-parameterized like `volume-up`).
- **`color` ops** — a `:color`/hex token is `text/plain`, but the real value is a
  `convert` verb (hex↔rgb↔hsl, via a channel) + `preview` (a swatch). New verb +
  `color/*` coercion channel; the type itself rides text verbs.
- **`extract` / `archive`** — `inode/file` (a `.zip`/`.tar`) ↔ container; model as a
  **coercion channel** (`application/zip → inode/directory`) plus an `extract` verb, so
  "list a zip's contents" becomes `tree` over the coerced directory.
- **`share` / `send-to`** — two-step (`subject` file/text → `object` `:contact`/`:device`);
  blocked on `:contact` and (cross-device) out of scope.

---

## Bucket D — New **engine mechanisms** (not domains — link, don't redesign)

These are already designed-and-deferred with homes in the design docs. Listed for
completeness; **do not re-spec here**.

- **`[[dispatch]]` content rules** (Plan 9 plumber) — declarative `match → type + verb`
  table. Designed in `prior-art-and-architecture.md` §Content-Dispatch + `detection.md`.
  The unifying home for custom content routing. *Link.*
- **Comma-trick / multi-subject batch** — `goo-protocol.md` §13 (`BATCH`); `;all`
  fan-out covers the common case today. *Link.*
- **Learned ranking feedback** — usage history → `weight` nudge; `prior-art.md` (Kupfer
  `rank_adjust`), daemon-era. *Link to #31.*
- **`valid_when` per-verb** (Kupfer `valid_for_item`) — a jq predicate gating a verb on
  subject *content*, not just type. Partially present (`valid_when` field exists); note
  the per-verb jq-eval cost. *Link.*
- **Async / late results** (Kupfer `is_async`) — needs the daemon. *Link to #31.*
- **pop-launcher meta-plugin & Kupfer bridge** — `goo-protocol.md` / `prior-art.md`,
  both daemon-blocked. *Link.*

**Net-new entity kinds worth their own small designs** (not yet specced anywhere):

- **Adverbs** — `--format` (for `convert`/`view`/`extract`), `--recipient` (contact
  resolution for `email`), `--dest` (explicit two-step target). `--model`/`--depth`/
  `--engine`/`--via` already exist.
- **Coercion channels** (`[[channels]]`) — the highest-leverage net-new entity, because
  one channel unlocks many verbs via the bus model: `markdown↔html` (pandoc),
  `html→text` (readability, unlocks "summarize a URL/bookmark"), `vcard↔json`,
  `ics↔json`, `image→thumbnail`, `color hex↔rgb`, `mailto:`-builder. Channels are
  dep-gated and compose into the negotiation planner for free.

---

## Worked example, end-to-end — `:bookmark` (the cheapest real expansion)

Mirroring the `dynamic-verb-providers.md` walkthrough, the full integration chain:

```toml
# bookmark.toml  (tier = desktop)
[[types]]
name = "application/vnd.bookmark"
display = "bookmark"
kind = "handle"
is_a = ["text/x-uri"]        # ← the load-bearing line: claims the URL bus, so it rides `open`

[[sources]]
name = "bookmarks"
prefix = "bookmark"
emits = "application/vnd.bookmark"
# id = the URL (the actionable locator); title = the human label (display only)
list_cmd = '''
  sed -n 's/^\(http[^ ]*\) \(.*\)/{"id":"\1","title":"\2"}/p' ~/.config/gtk-3.0/bookmarks \
    | jq -sc 'map(.)'
'''
```

What this buys, checked against the model:
- **Rides for free:** `goo open :bookmark:weather` works — `open` accepts `text/x-uri`,
  and `is_subtype(application/vnd.bookmark, text/x-uri)` holds via the `is_a` edge. The
  bare form `goo :bookmark:weather` also opens it (`default_for` resolves `open` through
  the membership — the same path the file-membership work added).
- **Facets, guarded:** it claims `text/x-uri` only. It deliberately does **not** claim
  `text/plain` (that would wrongly expose `upper`/`sha256`/`wc` on a bookmark). "Summarize
  the page" is a *coercion* (`text/x-uri → [fetch channel] → text/html → [readability] →
  text/plain → summarize`), gated on a `fetch` channel — not a membership.
- **Optionally adds:** `delete`/`edit` (handle verbs accepting `application/vnd.bookmark`),
  `confirm = true` on delete.
- **Cost:** 1 type + 1 source + 0 required verbs. The minimal, lattice-clean expansion —
  and the template every Bucket-A candidate follows.

---

## Prioritization (cost-honest)

| Candidate | Bucket | Cost | Blocked on | Tier |
|---|---|---|---|---|
| `:bookmark`, `:recent-url` | A | type + source | — | desktop |
| `:snip`/`:note` | A | type + source (+1–2 verbs) | — | core |
| `:trash`, `:font`/`:icon` | A | type + source (+1 verb) | — | desktop |
| file `move`/`delete`/`trash`/`copy-file` | C | 4 verbs (confirm, two-step) | — | desktop |
| `:wifi`, `:disk`/`:usb`, `set-wallpaper`, brightness | B/C | source + 1–2 verbs | — | desktop/cosmic |
| `:color` + `convert` | C | type + verb + channel | — | core |
| `:pass`/`:totp` | B | source + `copy` impl + `otp` | — (uses `Tainted`) | desktop |
| `extract`/`archive`, `markdown↔html`, `html→text` | C/D | channel (+verb) | — | desktop |
| `:contact` + `email`/`call`/`message` | B | source + 3–4 verbs + facet types | (partial) | productivity |
| `:cal` (khal), `:mail` (notmuch) | B | source + N verbs, stateful | dep weight | productivity |
| `[[dispatch]]`, batch, learned-rank, async, pop-launcher, Kupfer-bridge | D | engine | **daemon #31** | — |

## Design principles for expansion (the discipline this review encodes)

1. **Claim the narrowest membership whose verbs all make sense** — inherit the right
   vocabulary, never the wrong one (the provenance guard, generalized).
2. **Prefer riding a bus** (`inode/*`, `text/x-uri`, `text/*`, `image/*`) over adding
   verbs. A new type that fits a bus is nearly free.
3. **New verbs only for genuinely new actions** (`email`, `otp`, `set-wallpaper`); make
   cross-type actions **polymorphic** (a new `connect`/`copy` impl that joins the family).
4. **Cross-representation transforms are channels, not memberships** — "summarize a URL,"
   "list a zip," "hex→rgb" are coercions; they unlock many verbs at once and compose into
   the planner.
5. **Destructive or secret-bearing entities** → `confirm = true` + `Tainted` display +
   (secrets) auto-clearing clipboard; never log raw.
6. **Keep core dep-free** — heavy deps (notmuch, khal, firefox, pandoc) live in
   desktop/productivity tiers or arrive as `[[providers]]`; untrusted external strings
   (bookmark titles, contact names, mail subjects) are `Tainted` at every display site.
