# Decision: source-emitted facets are a declared allowlist

> **Status: decided, unimplemented.** A design-record for the trust boundary the
> per-instance-facet pattern ([contact-domain.md](contact-domain.md)) introduces. No
> source emits facets yet — only the engine does (`resolve_file`'s `inode/file`) — so
> this is settled *before* the first facet-emitting source ships, when it's free.

## The surface this opens

The file-vs-data work gave subjects a `_facets` membership list, consulted in
accept-matching (`verbs::subject_types`). For files the engine mints it (trusted). The
contact spec proposes a **source** mint it per-instance from its `list_cmd` output (a
contact is emailable iff it has an email). That output routinely includes **untrusted
external data** — a shared CardDAV store, a cloned repo, a filename.

Why this is genuinely new: today a source **cannot** influence a subject's type-
memberships. `address::resolve_source` (`tagged`) **overwrites** any `type` the item
emitted with the source's statically-declared `emits` — so `list_cmd` output can't change
which verbs match. `_facets` is **not** overwritten; it passes straight through. It would
be the **first channel by which `list_cmd` output decides verb applicability** — exactly
the kind of "untrusted input changes a trust decision" that goo otherwise closes by
construction (validated provider names, `Tainted` display).

## Why it's dangerous, with evidence

A forged `_facets: ["inode/file"]` makes a non-file subject match every `inode/*` verb. The
harm is bounded only by whether those verbs' `cmd`s are injection-clean — and **you cannot
assume they are**. While analyzing this, the `read`/`preview` verbs were found to wrap the
path in *manual* single quotes (`cat '{subject.metadata.path}'`) without `|q`: a file named
`x';touch PWNED;'.txt` achieved **command injection** on `goo read` (fixed in `f3450fd`).
Even now that those are `|q`-clean, the principle stands: a forged membership lets untrusted
data reach *whatever verbs accept that type*, including verbs not yet written. The
membership layer must not be the weak link.

## The decision — **B: a per-source declared facet allowlist**

A `[[sources]]` **declares** the facets it may emit; the engine **intersects** the
`_facets` an item emits with that declaration and **drops the rest**. So even if untrusted
external data flows into `_facets`, a source can only ever claim facets from its own
author-declared set — never a bus type like `inode/file`.

```toml
[[sources]]
name = "contacts"
emits = "application/vnd.goo.contact"
facets = [                                   # ← the allowlist; the only claims this source can make
  "application/vnd.goo.emailable",
  "application/vnd.goo.callable",
  "application/vnd.goo.messageable",
]
list_cmd = "…"   # may emit any subset of `facets` per item; anything else is silently dropped
```

This is the same shape as `emits` (declare your output type) and the verb-name rule
(declare/validate the safe set), and it makes a source's claimable memberships **auditable
without reading its `list_cmd`**. The contact design is unaffected — the source just lists
its three capability facets.

### Rejected alternatives

- **A — author discretion** (`_facets` passes through; "don't pipe untrusted fields into
  it"). Zero engine change, maximum flexibility — but *by-convention*, a silent footgun
  (no test or lint catches a careless source), and it re-opens the exact class goo closes
  by construction everywhere else. Defensible for a single-user launcher where the
  practical risk is low; rejected because goo's "point at any directory, including a
  hostile clone" pitch already made the opposite commitment, and consistency matters more
  than the one saved declaration.
- **C — engine denylist of bus types** (refuse `inode/*`/`text/*`/`image/*` from sources).
  Simpler for authors, but a *denylist* is fragile — a newly-added bus that's forgotten is
  a hole. An allowlist fails closed; a denylist fails open.

## Implementation shape (for when the first facet-source lands — not now)

- `[[sources]]` gains an optional `facets: [String]`.
- In `address::resolve_source::tagged`, after cloning the item, **retain only** the
  `_facets` entries present in the source's declared `facets` (drop the rest); engine-minted
  facets (`resolve_file`) are unaffected — they don't come through a source.
- `goo validate`: declared `facets` must be declared `[[types]]` (catches typos and
  stray bus-type claims at load).
- Tests mirror the file-membership unit tests: a source emitting an *undeclared* facet
  (e.g. `inode/file`) does **not** grant the corresponding verbs; a declared one does.

## Related hardening (separable)

- **Manual-quote injection (fixed, `f3450fd`):** every untrusted subject/object field
  reaching `bash -c` must use `|q`, never hand-wrapped `'…'`. Swept and fixed across
  files/tmux verbs; regression in `cmd-injection.bats`.
- **Trusted-source numerics (noted, not changed):** `apps`/`workspaces` verbs interpolate
  `{subject.metadata.index}` unquoted — integers from `cos-cli` (the compositor), outside
  the hostile-data threat model. Defense-in-depth would `|q` them; left as-is to avoid
  regressing working verbs for no real-threat gain.
- **Example files model the safe pattern:** `doc/examples/*.toml` channels use `{in.path}`
  unquoted; since examples are copy-paste templates, they should demonstrate `|q` even
  though they're not shipped/loaded by default.
