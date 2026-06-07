# Data entry & input UX

> **Status: design draft.** Captures the input-experience improvements
> surfaced by walking 30+ real inputs through their entry stages, the
> sigil-less inference layer that makes bare typing "just work," noun-first
> vs verb-first grammar trade-offs, and the additional experiments worth
> trying. Each section ends with the implementation hook so this doc maps
> onto specific slices.

## 1. Frame — speaking the language

goo's registry is a vocabulary: verbs, types, domains (where things live),
entities (specific instances). Data entry is *speaking sentences in that
language* — at a CLI, in a compose dialog, in shell completion, in the
inline launcher (planned). The interactive surface's job is to make
speaking feel as fast as typing.

Two design principles thread through everything below:

> **(1) Sigils are an explicit feature — they're how you're precise.
> `firefox` should work just as well as `:app/firefox` or `app/firefox`.**
> Bare input is the default mode; sigils are how you DISAMBIGUATE when
> "firefox" isn't enough (e.g., when both an app *and* a contact match).
> This is the keystone shift: today goo treats anything sigil-less as
> text/plain content; the design below adds **entity inference** at the
> address layer so bare names resolve through the registry.
>
> **(2) Surfaces match grammar to muscle memory.** The CLI stays
> **verb-first** (Unix muscle memory; `goo verb subject --flag`). The
> compose-GUI / inline launcher are **noun-first by default** (Spotlight
> muscle memory; pick the thing, then the action). Both surfaces consume
> the same OPTIONS engine. The user chooses the surface based on intent;
> the grammar matches what they're already typing.

---

## 2. Inputs traced through entry stages

30+ representative inputs, walked at: **2–3 characters** (early
prediction), **TAB at that point** (completion candidates), and **what's
listed when advancing** (next-token suggestions). For each, the note
column flags what's *missing* from today's behaviour and which design
section addresses it.

### A. Subjectless actions (verb-first, pure)

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 1 | `lo` | suggest: `lock`, `logout`, `lower` | Three overlapping — needs **subjectless-first ranking** (no subject likely → prefer verbs whose accepts is empty/wildcard). §6.3 |
| 2 | `loc` | `lock` (1 match) → TAB completes | clean today |
| 3 | `lock <space>` | "no subject needed — Enter executes" inline hint | today: silent. §6.3 |
| 4 | `shu` | `shutdown` with **confirm-gated chip** | today: no visual signal. §6.5 |
| 5 | `mute-toggle` | direct execute | takes no subject; fine |
| 6 | `screenshot` | direct | takes none; fine |

### B. Subject-needed actions (verb-first; subject after)

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 7 | `op` | `open` | unique once `e` added |
| 8 | `open <space>` | listing: paths from cwd, recent items, source prefixes whose emits match `inode/*` or `text/x-uri`, `+text`, `^clip` | today: full source list, undifferentiated. **§6.1 subject-shape-aware listing** |
| 9 | `open n` | filter: `notes.md` (recent), `nginx.conf` (cwd), `:net:` (source) | mixed-bag; **sort by accepts match** — files first, `:net:` demoted (open doesn't accept network connections). §6.1 |
| 10 | `summarize` | direct + implicit-subject preview | accepts text/* |
| 11 | `summarize ` (space) | listing: clipboard `^`, PRIMARY `:sel`, recent text files; **first hint: "if you press Enter: 'the selected paragraph…'"** | today: silent fallback chain. §6.4 |
| 12 | `summarize ./paper.pdf --vi` | filter adverbs: `--via=` → TAB to `--via=`, then selector values | already subject-aware (we shipped this); ✓ |
| 13 | `summarize ./paper.pdf --via=fa` | filter values: `fabric` | works ✓ |

### C. Polymorphic verbs (dispatch by subject type)

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 14 | `co` | `connect` `×3` chip, `copy`, `copy-path`, `containers` | **polymorphic count chip** ("this name has 3 impls"). §6.2 |
| 15 | `connect <space>` | listing: `:ssh:`, `:bt:`, `:net:` (sources whose emits matches any `connect` impl's accepts) | the polymorphism payoff at the input layer. §6.1 |
| 16 | `connect :s` | `:sel:`, `:ssh:` → demote `:sel:` (text-typed, won't actually dispatch) | rank by "this would actually dispatch." §6.1 |
| 17 | `info :ps/12<TAB>` | items from `:ps` filtered "12*"; show title+subtitle | works today; polymorphic `info` just consumes |
| 18 | `info /tmp/photo.png` | direct; dispatches image/* impl via extension signal | works ✓ |
| 19 | `stop <space>` | listing: `:svc:`, `:ctr:`, AND "no subject (media player)" | three-way poly with one subjectless impl — listing should expose all paths. §6.1+6.3 |

### D. Noun-first / GOO-default / sigil-less

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 20 | `:` | source prefixes + value domains: `:app:`, `:bt:`, …, `:file/`, `:type/` | works today; the discovery moment |
| 21 | `:wi` | `:win:` (1 match) | works |
| 22 | `:win:no` | items from `:win` filtered "no" — Notes, Notion windows | works; live `list_cmd` peek |
| 23 | `:win/com.system76.CosmicEdit/2` | (Enter) → GOO default → `activate` | works ✓ — the **GOO default** is the canonical noun-first escape hatch |
| 24 | `~/notes.md` | resolves as inode/file → `open` (file's default_for) | works; bare paths handled |
| 25 | `=text/markdown` | virtual-type subject — but no GOO default for the type → error | **GOO-default disambiguation message** needed: "no default verb for type text/markdown — try [critique, summarize, …]." §6.6 |
| **26** | **`firefox`** | **today: "unknown verb"; under §3 inference: top match is `:app/firefox` → activate** | **the keystone case. §3 sigil-less inference** |
| **27** | **`fox`** | **today: unknown verb; under §3: substring match across enumerable sources — top: `:app/firefox`, second: a recent file "fox.md"** | **fuzzy / ranked. §3.2** |
| **28** | **`build1`** | **today: unknown verb; under §3: `:ssh/build1` (ssh config host)** | **§3** |
| **29** | **`app/firefox`** | **today: text/plain content; under §3.1 "prefix-shape inference": treat as `:app/firefox`** | **the `<prefix>/<rest>` shortcut. §3.1** |
| **30** | **`ssh/build1`** | **same as 28+29 combined** | works via §3.1 |

### E. Modifier-heavy / two-step / discovery

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 31 | `move-to :win:notes :ws<TAB>` | items from `:ws` source | object completion via `object_type`/`object_source`; works |
| 32 | `--explain <verb> <subj>` | read-only plan preview | chip "(no execution)"; works ✓ |
| 33 | `options <subj>` | discovery JSON | works ✓ |
| 34 | `compose` | open compose-GUI | the noun-first entry point |
| 35 | `dispatch "RFC 2616"` | regex match → route | works (one-shot pattern) |

### F. Long-form text (text-verbs)

| # | Input | Early / TAB / advance | Note |
|---|---|---|---|
| 36 | `goo critique` (no subject) | fallback chain: PRIMARY → clip → stdin → error | works; §6.4 makes the fallback visible |
| 37 | `goo critique <paste>` | text input box (compose-GUI) for multi-paragraph | compose-GUI v2 work |
| 38 | `echo "x" \| goo upper` | stdin-as-subject pipe | works ✓ |

---

## 3. Sigil-less inference — the design

The keystone. Today, sigil-less + non-path + non-URL + non-verb input
either errors ("unknown verb") or falls to text/plain content. The design
below makes **bare entity names resolve through the registry** while
keeping explicit sigils as the precision tool.

### 3.1 Resolution rules in order

The address layer's `resolve` gains a final inference stage. Order is
first-match-wins; explicit always beats inferred.

```
For a bare input that is NOT a verb name and NOT a subcommand:
  A. Native file/URL shape  (./path, ~/path, /path, scheme://…)   →  existing
  B. Sigil-prefixed         (:, +, ^, =, user-claimed @ etc.)     →  existing
  C. Alias expansion        (declared [[aliases]] name)           →  existing
  D. Prefix-shape inference (NEW):
     If input matches `<prefix>/<rest>` AND `<prefix>` is a known
     source-prefix, treat as `:<prefix>/<rest>`. Examples:
       app/firefox      →  :app/firefox        (exact value)
       ssh/build1       →  :ssh/build1
       win/cosmic-edit  →  :win/cosmic-edit    (will fuzz the rest)
     (Cheap, deterministic, no source scans.)
  E. Entity-name inference  (NEW, §3.2):
     Query enumerable sources, rank candidates, pick a top match
     subject to threshold + margin. Yields a subject and dispatches
     to GOO default (or compose-GUI for ambiguity).
  F. Fall back to text/plain content (existing default).
```

### 3.2 Entity-name inference — confidence bands (the threshold model)

The scoring algorithm produces a numeric score, but the *user-facing model
is four bands* (locked: see "threshold-design walk" elsewhere in the
session record). Bands map directly to UX response; the numeric floors are
implementation detail and can shift without renaming the bands.

#### 3.2.1 The bands

| Band | Formal rule | UX response |
|---|---|---|
| **DEFINITIVE** | exactly one candidate with `score ≥ EXACT_FLOOR` (an exact id match, an exact title match, or a canonical id-substring in a single-source-only) | resolve silently; safe even non-interactively |
| **HIGH** | top score `≥ HIGH_FLOOR` AND top `≥ 2 × second` AND result-count `≤ 3` | interactive: resolve + one-line nudge log; script: nudge-then-fallback (see §3.2.3) |
| **MEDIUM** | top score `≥ MEDIUM_FLOOR` AND (top `< 2 × second` OR result-count `> 3`) | surface a picker — inline numbered "Did you mean: 1) :a/x  2) :b/y  …" |
| **LOW** | top score `< MEDIUM_FLOOR` | fall through to text/plain content (today's default behaviour) |

Notes on the rule:
- The `2 ×` ratio for HIGH is *relative*, not absolute — addresses the
  brittleness of pure absolute thresholds (the gaps in the scoring
  distribution are what matter, not the constant numbers).
- Result-count gates (≤3 for HIGH, >3 for MEDIUM) keep "a clear winner
  amid noise" from feeling like a guess.
- DEFINITIVE is *single*-candidate by construction — there is no scenario
  where two candidates both qualify (the rule requires uniqueness).

#### 3.2.2 The numeric scoring (implementation detail)

This feeds the bands; can change without breaking the user-facing model.

```
For a bare token t, for each enumerable source's cached items:
  score = 0
  if item.id == t:                     score = 1000  (exact id)
  elif item.title == t:                score = 800   (exact title)
  elif word_boundary_match(t, item):   score = 400 * (len(t) / len(matched_word))
                                       # ratio vs the matched WORD/segment (split on
                                       # space/-/_), NOT the whole title — so a whole-word
                                       # hit scores 400 even in a long descriptive title
                                       # ("gateway" in "api-gateway (prod)" → 400 → HIGH). cb1e2fc.
  elif item.id contains t:             score = 200 * (len(t) / len(item.id))
  elif item.title contains t:          score = 100 * (len(t) / len(item.title))
  else:                                score = 0

  // Source-priority weight (multiplicative)
  score *= source.weight              // §3.2.4 defaults

  // Recency bonus
  if source.recency_ordered and item.idx < 10:
      score += max(0, 20 - 2 * item.idx)
```

The proposed floors:

| Floor | Value | Picked to clear |
|---|---|---|
| `EXACT_FLOOR` | **800** | exact title (800) or exact id (1000) — natural gap to substring-based scores |
| `HIGH_FLOOR` | **200** | id-substring (200 max) and decent title-substrings (100 max × something with high source.weight) — anything weaker is fuzzy guess territory |
| `MEDIUM_FLOOR` | **60** | weakest meaningful substring match (a third-of-a-title hit on a default-weight source ≈ 33; with a recency bonus or source-weight boost, lands above 60) |

These floors clear the natural gaps in the scoring distribution; small
changes to scoring constants (within ±50%) don't change which band any
candidate falls into.

#### 3.2.3 Context adaptation (the script/TTY/GUI split)

Same bands, but the band boundaries shift by detected context:

| Context | Detection | DEFINITIVE | HIGH | MEDIUM | LOW |
|---|---|---|---|---|---|
| **Script** | stdout is not a TTY AND `GOO_INFER_STRICTNESS != "tty"` | resolve silently | **nudge-then-fallback** (log "would have resolved firefox → :app/firefox; use the explicit form in scripts") | always fall through | fall through |
| **Interactive** | stdout is a TTY OR `--infer-strictness=tty` | resolve silently | resolve + log a brief one-line nudge | inline numbered picker | fall through |
| **GUI** | called from compose-GUI / inline launcher (entry surface tag) | autoselect with visible "change" affordance | autoselect with visible nudge | picker is the primary UI | "nothing matched" message |

**Detection mechanism:**
- Default: `isatty(stdout)` → interactive; non-TTY → script.
- `--infer-strictness=script|tty|gui` CLI flag overrides.
- `GOO_INFER_STRICTNESS=script|tty|gui` env var overrides (lower priority than the flag).
- GUI contexts pass the mode explicitly via an entry-surface tag in the request.

**Safety property**: the only context where bare entity resolution can
fire silently in a non-interactive setting is DEFINITIVE — exact ID,
unique. Scripts cannot be surprised by a fuzzy match that happens to
have the right shape today.

#### 3.2.4 Source weights (per-source priority)

`source.weight` defaults — picked to give "obvious launcher targets" a
slight boost over noisier sources, but no source can over-power an exact
match (the rule structure protects exact matches as DEFINITIVE
regardless of weight):

| Source | Weight | Why |
|---|---|---|
| `:app` | 1.3 | apps are the canonical noun-first target |
| `:win` | 1.2 | per-toplevel; specific |
| `:ssh` | 1.2 | exact host names from config |
| `:mnt` | 1.1 | mount points are distinctive |
| `:recent` | 1.1 | recently-touched > all-time |
| `:repo`, `:br` | 1.0 | dev domain, mid-priority |
| `:bt`, `:net` | 1.0 | device/connection names |
| `:svc`, `:ctr` | 1.0 | system services |
| `:emo` | 0.8 | emoji titles often substring-collide; demote |
| `:hist` | 0.6 | clipboard fragments often look like other things; demote |
| `:ps` | 0.7 | (opt-in only; if enabled, demote — process names overlap) |

Users override per-source via `[[sources]] weight = 1.4` in plugin
config.

#### 3.2.5 Walked through the test inputs

How the band model behaves on the 10 representative inputs from §3.0:

| Input | Context | Band | Resolution |
|---|---|---|---|
| `firefox` | TTY | DEFINITIVE | `:app/firefox` silent |
| `firefox` | script | DEFINITIVE | `:app/firefox` silent (exact id; safe) |
| `fox` | TTY | MEDIUM | picker: `1) :app/firefox  2) :recent/fox-recipe.md  …` |
| `chrome` | TTY | LOW | fall through to text |
| `com.system76.CosmicEdit` | any | DEFINITIVE | silent exact id |
| `notes` | TTY | MEDIUM | picker across windows/recent/clip |
| `Notes.md` | TTY | DEFINITIVE | `:recent/Notes.md` silent (exact title) |
| `build1` | TTY | DEFINITIVE | `:ssh/build1` silent (exact `Host`) |
| `nginx` | TTY | MEDIUM | picker (3 sources tie unless one source's weight breaks it) |
| long phrase | any | LOW | text content |
| `2+2` | any | LOW | text content (calc verb consumes) |

### 3.3 Performance & caching

> **Built (slice 7b, then reworked by `c673cf4`; `inference.rs`).** Per-source
> cache at `$XDG_RUNTIME_DIR/cosmic-goo/entities/<source>.json`, with the
> `inferable` opt-in field. The original TTL was **replaced by watch/mtime
> invalidation** under a *no-stale* directive (`cache_ttl` /
> `DEFAULT_CACHE_TTL_SECS` removed): a source caches **only if** it declares
> `watch = [paths]` (files whose mtime is its freshness signal), and an entry is
> valid iff `cmd` is unchanged AND every watch path's CURRENT mtime equals the
> value stat'd *before* `list_cmd` ran — so a concurrent edit yields a false-STALE
> (safe recompute), never a false-fresh. Sources **without** `watch`
> (command/dbus-backed: apps, bluetooth, …) are no longer cached on the one-shot
> CLI — they recompute every run rather than risk staleness; true warm caching
> for those is a `good`-daemon job (#31: inotify + dbus). `goo reload` /
> `clear_entity_cache()` is the manual drop for whatever watch can't see yet. The
> cache is an optimization, never a correctness gate — any miss re-runs
> `list_cmd`; empty results aren't cached. Today only the file-backed `recent`
> source participates (`watch = ["~/.local/share/recently-used.xbel"]`);
> ssh-hosts/mounts are `enumerate = false` and the rest are command/dbus and
> recompute. (The refresh-policy table below is the original design sketch; its
> per-source *TTL fallbacks* did not ship — a no-`watch` source recomputes instead.)

Scanning every source's `list_cmd` on every keystroke is unacceptable.
Strategy: **per-source list caching** with explicit invalidation hooks.

```
Cache file:  $XDG_RUNTIME_DIR/cosmic-goo/entities/<source>.json
Refresh policy per source:
  - apps / windows / focused / workspaces:    refresh on cos-cli signal (or 5s TTL)
  - ssh-hosts:                                refresh on ~/.ssh/config mtime
  - recent:                                   refresh on recently-used.xbel mtime
  - clipboard, clipboard-history:             refresh on every read (cheap)
  - mounts:                                   refresh on findmnt mtime hook (or 10s TTL)
  - processes (`:ps`):                        DO NOT cache; volatile; opt-in only
  - bluetooth, network:                        refresh on dbus signal (or 30s TTL)
```

**Opt-in per source** for inference participation. A `[[sources]] inferable = true|false` field. Defaults: `apps`, `windows`, `mounts`, `recent`, `ssh-hosts`, `workspaces`, `clipboard`, `sinks`, `bluetooth`, `services` = true. `processes` (volatile, noisy), `containers` (potentially sensitive), `branches` (CWD-dependent) = false by default; user can opt in.

### 3.4 Verb-aware bias

> **Built (slice 8).** `inference::infer_entity_for_verb` filters the scan to
> sources whose `emits` the verb `accepts` (subtype-aware), wired into the
> verb-position bare-token path (`resolve_subject`, before the text fallback)
> with the same band model as noun-first. The accepts-filter *narrows* on top
> of the §3.3 participation gate — it never widens past it, so §3.6's privacy
> guarantee holds (an `inferable = false` source never enters the scan even
> when a verb accepts its type). Sources a verb accepts but that are
> `enumerate = false` (`:bt`, `:file`) drop out of *scored* inference and are
> resolved by the ungated `handle_search` first-match fallback until they earn
> `inferable = true` — same deferral as slice 7b.

When the bare token follows a verb (`goo connect fox`), the inference
*biases toward sources whose emits matches the verb's accepts*:

- `connect fox` → verb `connect` accepts `vnd.ssh.host`/`vnd.bluez.device`/`vnd.nm.connection`; inference only considers `:ssh`, `:bt`, `:net` sources. "fox" matches no ssh host but might match a BT device "Foxconn-headset" → resolved.
- `open fox` → verb `open` accepts `inode/*` and `text/x-uri`; inference considers `:file`, `:recent`, `:mnt` (vendor inode-subtypes), and falls back to URL shapes. "fox" matches recent file `fox-recipe.md`.
- `summarize fox` → verb `summarize` accepts `text/*`; inference considers `:recent` (text files), `:clip`, `:sel`; matches a recent text file containing "fox" or current clipboard if it starts with "fox".

This dramatically narrows the candidate pool and improves both relevance
and performance.

### 3.5 Ambiguity handling — by band & context

The band model already encodes the response:

| Band | Interactive (TTY) | Script (non-TTY) | GUI |
|---|---|---|---|
| DEFINITIVE | silent | silent (safe — exact id, unique) | autoselect; "change" visible |
| HIGH | resolve + one-line nudge | nudge-then-fallback (see below) | autoselect with nudge |
| MEDIUM | inline numbered picker | always fall through | picker UI (primary mode) |
| LOW | fall through to text/plain | fall through | "nothing matched" |

**Nudge log format** (for HIGH bands):
```
goo: inferred 'firefox' → :app/firefox  (band: HIGH; use :app/firefox to suppress)
```

**Inline picker format** (for MEDIUM in TTY):
```
goo: 'fox' is ambiguous — pick one:
  1) :app/firefox            Firefox
  2) :recent/fox-recipe.md   Fox recipe
  3) :hist/14                "fox news headline"
Re-run with the explicit address, or set GOO_INFER_STRICTNESS=tty and
add --pick=N.
```

**Script nudge-then-fallback** (HIGH-band in script context):
```
goo: would have inferred 'firefox' → :app/firefox (HIGH band) — not
     auto-resolving in script context. Use :app/firefox explicitly,
     or pass --infer-strictness=tty to opt in.
[falls through to text/plain — verb sees subject as bare text]
```

### 3.6 Privacy & determinism — what the band model guarantees

- **Privacy**: only the `inferable = true` sources participate in the
  scan; sensitive sources (clipboard-history, ssh, containers) ship
  `inferable = false` by default. Listed in §3.3.
- **Determinism for automation**: scripts get one safe class only —
  DEFINITIVE (exact id, unique). Everything fuzzier degrades to a nudge
  log + fallback. No surprise resolutions in pipes / CI / cron.
- **Override**: `--infer-strictness=tty` (CLI flag) or
  `GOO_INFER_STRICTNESS=tty` (env) escalates a script context to
  interactive permissiveness, opt-in.
- **Suppression**: `--no-infer-nudge` silences the nudge log when an
  automation actively wants the inferred behaviour but doesn't want the
  noise.

### 3.7 Subject-position only (not verb-position)

Critical constraint: inference NEVER fires for the verb-position. If a
verb-name lookup fails, that's a hard "unknown verb" — not an attempt to
infer the verb from entities. (`goo firefox` could be ambiguous between
"the firefox app as the verb-position" and "noun-first short form" — but
since "firefox" isn't a verb in the registry, the verb-position lookup
fails fast, and we fall through to noun-first inference at the
verb-position. See §4 noun-first for how this looks.)

---

## 4. Noun-first analysis

### 4.1 When users want it

**Verb-first** is decisive:
- "I want to LOCK the screen, now."
- `calc 2+2`, `search rust traits`, `screenshot`.
- Pipes (`echo … | goo upper`).
- Scripts.

**Noun-first** is discovery:
- "I have this WINDOW; what can I do with it?"
- "Type my friend's name; show me actions" (Spotlight muscle memory).
- For complex types (a specific PR, a contact), the noun is in mind first.
- The user doesn't yet know what verbs are even possible.

### 4.2 What goo already supports

- `goo <addr>` with no verb → runs the type's `default_for` (the
  protocol's **GOO** default verb). Cleanest noun-first one-shot.
- `:` at the front of completion → cycles source prefixes
  (noun-discovery moment).

### 4.3 What's missing for full noun-first

1. **Bare entity names** resolving as nouns (§3 — the keystone fix).
2. **Verb-pick after a noun (CLI)** — **shipped (#15)** as `goo do <addr> [verb]`.
   Today after `goo :app/firefox <space>` the parser expects a second positional (object). The grammar's ambiguous here. The forms considered:
   - `goo :app/firefox close` → ambiguous (is `close` the verb or a token to pass?). Could disambiguate by: if the second positional is a known verb AND the first positional is an address, reinterpret as `goo close :app/firefox` (reorder). Subtle but workable. Optional `--reorder` flag for explicit opt-in.
   - `goo --on :app/firefox close` → explicit "noun-first" marker. Unambiguous. Adds a flag.
   - `goo do <addr> [verb]` → new subcommand that pops verb-pick if verb omitted. Cleanest CLI surface. **This is what shipped.** With a verb, `goo do <addr> <verb> [args]` is a pure reorder of `goo <verb> <addr> [args]` — it re-enters the verb-first path (`cmd_verb`) verbatim, so subject/object/adverb parsing, confirm-gating, negotiation, and history recording are byte-identical (an equivalence locked by a test). With no verb it's the verb-pick: it prints the applicable-verbs listing (delegates to `goo what`, the Gate-4 SSOT — no interactive stdin picker; printing the menu matches how MEDIUM-band inference and `goo what` already work). The `--reorder` flag and `--on` marker were *not* taken — a dedicated subcommand avoids overloading the positional grammar entirely.
3. **Inline-launcher noun-first** — the pop-launcher meta-plugin (planned, not built). Type → entity matches → Enter for default verb, → opens actions menu. Spotlight-style.
4. **Compose-GUI noun-first** — the GUI builds noun-first by default: subject candidates surface first (PRIMARY/clip/recent + fuzzy source items), then verb-pick from OPTIONS.allow.

### 4.4 The grammar split, concretely

| Surface | Default grammar | When to break it |
|---|---|---|
| CLI (`goo`) | **verb-first** | `goo <addr>` no-verb → GOO default (**runs**). `goo do <addr>` → noun-first verb-pick (**lists**); `goo do <addr> <verb>` → reorder. |
| Shell completion | follows CLI | augmented with entity inference (§3) at the verb-position |
| Compose-GUI | **noun-first** | Subject first, then verb. CLI-equivalent preview shown live. |
| Inline launcher | **noun-first** | Spotlight pattern; one keystroke discovers entities. |
| `goo dispatch <raw>` | content-classifier (separate pattern) | regex → route |

---

## 5. The improvements (cross-cutting wins)

These came out of the input walk. Each is small-to-medium scope; most
are independent.

### 5.1 Subject-shape-aware listing (§B-8, B-9, C-15)

> **Built (slice #5).** `__complete verb-subject-items` now unions a
> polymorphic verb's impls' `accepts` and ranks matching enumerable sources by
> `verbs::accepts_specificity` (the SSOT scoring `lookup`/`for_subject` use —
> exact > lattice > glob-by-prefix-length); ids are deduped globally
> (first-seen wins, preserving rank). Ranking is per-source. Order-preserving
> consumers (zsh `_describe`, fish, compose-GUI) can present this order;
> **bash keeps its flat menu alphabetical by design** (a 100-entry unsorted
> `apps` menu is harder to scan, and "most-specific-accept" ≠ "most-relevant").
> Note the stage emits bare ids (no source tag), so consumers get *order* but
> can't *group* by source or distinguish a same-id collision across sources —
> true grouping needs the source-qualified completion deferred in
> [completion-polish.md](completion-polish.md) §1; when that lands, the output
> format and the cross-source dedupe rule should be revisited together. The
> per-source `list_cmd` fan-out here is uncached — sharing slice 7b's entity
> cache is a tracked follow-up.

After a verb is on the line, subject suggestions should be **ranked by**:
1. Whether the source's `emits` would dispatch under the verb's
   `accepts` (specificity-aware — the OPTIONS-style projection).
2. For polymorphic verbs: the UNION of all impls' accepts.

`__complete verb-subject-items` already does step 1 partially; extend
with the specificity scoring + polymorphic-union, and have it return
*ranked* candidates the shell uses for ordering. Slice: small Rust
addition to `cmd_complete`.

### 5.2 Polymorphic-verb count chip (§C-14)
In tab-completion suggestion lists, verbs with multiple impls get a
`×N` annotation. The bash-completion script gains a "describe before
expanding" step that queries `__complete verbs-meta <name>` (new
stage) returning `name + ×count`. Cheap.

### 5.3 Subjectless verb announcement (§A-3)
After a verb's name is typed AND a space, if accepts is empty/wildcard,
the completion emits a hint line: `"no subject needed — Enter executes"`.
Shell-side: bash-completion's `_init_completion` flow can write a
preview line (or use the `COMP_PROMPT_BAR` hack). Compose-GUI shows a
caption directly. Slice: medium for shell, trivial for GUI.

### 5.4 Implicit-subject preview (§B-11)
When a text-* verb is on the line with no subject, show "if Enter: '<the
selected paragraph snippet …>'" or "(clipboard: '<first 40 chars>')". The
fallback chain (PRIMARY → clip → stdin) is made visible. Slice: shell
hint + compose-GUI caption.

### 5.5 Confirm-gated chip (§A-4)
Verbs with `confirm = true` get a visual marker in suggestion lists.
The bash-completion script adds a `[!]` suffix; the compose-GUI shows a
red dot. Trivial.

### 5.6 GOO-default disambiguation (§D-25)
When a noun-first input resolves to a type with no `default_for`, error
helpfully: `"no default verb for type text/markdown — try one of:
critique, summarize, think, …"` (top 5 verbs by `for_subject` order, or
prompt picker). Slice: small Rust bin change in the GOO dispatch path.

### 5.7 The big one — sigil-less inference (§3)
The keystone. Slice: substantial — address-layer extension + caching
layer + per-source `inferable` flag + ambiguity handling + CLI/GUI
disambiguation UX. v1 can start with prefix-shape inference (§3.1)
only — cheap, deterministic, useful. v2 adds entity-name inference
(§3.2) with caching. v3 adds verb-aware bias (§3.4).

---

## 6. Additional experiments (worth trying)

Ideas that emerged thinking about input UX, beyond the immediate
improvements.

### 6.1 "Again" — re-run last verb on a new subject
Shell-history-style: `goo again <new-subject>` repeats the last
verb+adverbs on the new subject. Or `Up`-arrow-style in compose-GUI:
recall last sentence, edit subject inline. Saves re-typing for
repetitive workflows ("summarize this, summarize that, summarize the
other").

### 6.2 Smart implicit-subject promotion by shape
If the PRIMARY selection is **JSON-shaped** (matches the `json` checker),
the default verb for a typed-but-no-subject invocation could be the
JSON-applicable one (`json-pretty`, `json-keys`) — letting `goo` (no
verb, no subject) on a JSON selection just pretty-print it. Aggressive;
could be the `goo` subcommand-less default ("do the obvious thing with
my current context").

### 6.3 Recent-action shortcuts
Track per-subject-type the user's last N verb choices. When the user
addresses a subject of that type, suggest the recent verbs first in the
listing. "Last time you opened a `:repo:`, you ran `status`."

### 6.4 Multi-modal subject
What if the user wants to apply a verb to MULTIPLE subjects?
`goo lock :app/firefox :app/cosmic-edit` — close both. Or pipe-style:
`goo apps | filter --idle | goo close`. Real launcher need (closing many
windows in a workspace). Design call: extend the address layer to a
`<multi>` form (`:app/{firefox,cosmic-edit}` brace-expansion), or use
shell-side fan-out.

### 6.5 "Speak it back" — live canonical preview
In compose-GUI, the equivalent CLI command updates live as the user
fills slots. Two effects: users learn the grammar (the GUI is a tutor
for the typed form); copy-command escapes to scripting one keystroke
away. Already in the Control Center design draft for the dispatch trace
probe; should be in compose-GUI too.

### 6.6 Late-binding state preservation
Change verb after picking subject? Adverbs that the new verb also opts
into stay set; others fade. Change subject after a verb+adverbs are set?
Sentence preserves; verb may go from green to yellow ("doesn't dispatch
for this subject — pick a polymorphic peer or change verb"). Recovering
from "I changed my mind" without total reset is what makes a launcher
feel fluid.

### 6.7 Error recovery as part of input
When `cmd` exits nonzero, compose-GUI preserves the full sentence state
and shows stderr inline with three options: **retry**, **edit any
slot**, **cancel**. CLI version: print stderr + suggest `goo --explain
<same sentence>` so user can debug interactively without re-typing.

### 6.8 Conversion suggestions on 415 — **shipped (#14)**
Today the teaching 415 (negotiation §4.1) suggests `--hops N` or
`--force`. Extend: also suggest *alternative verbs* that would reach the
goal from the subject. "View won't render this as ANSI; try `cat` or
`json-pretty`." Powered by OPTIONS.allow filtered to verbs that accept
the subject's type AND emit something close to Accept.

**Shipped**: the plain 415 in `exec_negotiated` now appends
`try a verb that accepts <type>: <verbs>` — the verbs from `OPTIONS.allow`
(the same SSOT `goo what` shows) that accept the subject's type directly,
minus the failed verb and any `destructive` verb (a safe-by-construction
alternative list; running one won't 415). One `alt_verbs_hint` on the no-route
415 `die` (not the teaching-415, which already hands `--hops`/`--force`), so
coercion-415s and present-verb-415s share it. The simpler
"accepts directly" framing (vs. the spec's "emits close to Accept") was
chosen deliberately: a direct-accepting verb is *guaranteed* not to 415,
whereas an emits-close heuristic could re-suggest a verb that itself 415s.
Tests: `tests/integration/suggest-415.bats`.

### 6.9 Smart adverb defaults that learn
If the user always uses `--via=fabric` for `critique`, learn that and
pre-fill it. Per-user, per-verb. Stored in
`~/.config/cosmic-goo/learned-defaults.toml`. Opt-in.

### 6.10 Voice / scan entry
The CLI grammar is type-friendly; voice ("activate firefox") maps to
the same sentence structure. Image-scan ("here's a QR code, run it")
maps to `goo dispatch <decoded-text>`. The grammar handles these without
new surfaces; the entry layer just needs to translate.

### 6.11 Sticky preview in shell completion
After `<TAB>` on an ambiguous prefix, show a small status line below the
prompt with the top-3 candidates' titles/subtitles for 1.5s, then fade.
Modeled on Fish's history hints. Heavy-handed but high-impact for
discoverability.

### 6.12 Schema for plugin TOML (authoring entry)
Ship a JSON Schema for the plugin TOML at
`schemas/cosmic-goo-plugin.schema.json`, registered via a `# yaml-language-server: $schema=…` line at the top of new plugins (works in
VSCode/Neovim via the YAML/TOML language servers). Auto-complete for
`accepts`, `cmd`, `default_for`, etc. Plugin authoring without leaving
the editor.

---

## 7. CLI grammar changes — minimal & backward-compatible

| Change | Today | Proposed | Compat? |
|---|---|---|---|
| Bare entity-name resolution | "unknown verb" | §3 inference: try as noun if not verb | ✓ — only fires when verb-lookup fails |
| `prefix/rest` shortcut | text/plain | §3.1: `:prefix/rest` if `prefix` is a source-prefix | ✓ — only when the prefix matches |
| Noun-first verb-pick | **shipped (#15)** | `goo do <addr> [verb]` — new subcommand (the `--reorder`/`--on` variants were not taken) | additive |
| GOO-default error | "no default verb" | helpful suggestions list | message-only |
| Polymorphic chip / confirm chip | none | completion-time annotation | additive |

No breaking changes; everything fires only when current behaviour would
have failed or been silent.

---

## 8. Implementation roadmap

Ordered by smallest-first (each is a separate slice; each is independent
unless noted).

| # | Slice | Size | Notes |
|---|---|---|---|
| 1 | **Confirm chip + polymorphic ×N chip** in `__complete verbs` output | small | shell script + bin tweak |
| 2 | **Subjectless announcement** in completion | small | shell + bin |
| 3 | **GOO-default disambiguation** message | small | bin only |
| 4 | **Prefix-shape inference** (§3.1) | small | `address::resolve` — `app/firefox` ↦ `:app/firefox` |
| 5 | **Subject-shape-aware listing** (§5.1) | medium | extend `__complete verb-subject-items` to rank by accepts-specificity |
| 6 | **Implicit-subject preview** | medium | bin + shell hint |
| 7 | **Entity-name inference v1** (§3.2 spec: scoring + bands + context adaptation + caching) | substantial | new `address::infer_entity` returning `(Subject, Band, Reason)`; `Band` drives the caller's UX response; caching layer per §3.3 |
| 8 | **Verb-aware bias** (§3.4) | medium-substantial | layered on top of #7 |
| 9 | **Compose-GUI v2 noun-first flow** — **inc 1+2 built** (gnome-do/Kupfer: side-by-side panes, type-to-filter, keyboard nav, object pane; recency reorder) | substantial | the GUI payoff slice |
| 10 | **"Speak it back" live preview** in compose-GUI — **shipped with #9 inc 1** | small once #9 lands | |
| 11 | **JSON Schema for plugin TOML** (§6.12) | small | static file + docs |
| 12 | **Late-binding / error recovery** in compose-GUI | medium | UX correctness work |
| 13 | **"Again" / recent-actions** (§6.1, §6.3) | medium | persistent history layer |
| 14 | **Conversion suggestions on 415** (§6.8) — **shipped** | medium | extends teaching 415 |
| 15 | **`goo do <addr>`** noun-first subcommand — **shipped** | small | bin |

**Suggested first slice**: #1 + #2 + #3 together as "completion polish"
(all small, all visible). Then #4 as the first inference taste. Then
either #7 (the keystone — entity inference) or #9 (compose-GUI v2) as
the next big arc, depending on appetite.

**Shipped**: #1 + #2 + #3 (completion polish — spec/check-in plan in
[completion-polish.md](completion-polish.md), which owns the chip-vocabulary
single-source-of-truth future ports must cite); #4 (prefix-shape inference,
`address::resolve`); #7 (entity-name inference — engine `inference.rs`, bin
dispatch, MEDIUM picker) **plus its 7b caching layer** (per-source cache
at `$XDG_RUNTIME_DIR/cosmic-goo/entities/<name>.json` + the `inferable` opt-in
field — the TTL was later reworked to watch/mtime invalidation for a no-stale
guarantee, `c673cf4`; see §3.3); #8 (verb-aware bias — `infer_entity_for_verb` narrows the
scan to sources the verb accepts, wired into `resolve_subject`; see §3.4); #5
(subject-shape-aware listing — `verb-subject-items` ranks by accepts-
specificity + polymorphic-union; see §5.1); #6 (implicit-subject preview —
shell surface) **plus a run-time fallback nudge**: the completion-time `if
Enter: '…'  (PRIMARY selection)` hint is a `__complete implicit-preview` stage
(PRIMARY→clipboard peek, 150ms timeout) shown via the non-destructive stderr
mechanism; the run-time nudge (`no subject given — using …`) narrates the same
fallback when a subjectless text verb actually executes. The #6 **compose-GUI
caption** is deferred to #9 (no GUI yet); #13 §6.1 (**`goo again`** — persistent
action history at `$XDG_STATE_HOME/cosmic-goo/history.jsonl`, recording
`{verb, type, selector-adverbs}` only; `goo forget` clears, `GOO_NO_HISTORY=1`
disables; on by default). #13 §6.3 (**recency hint**) — `goo what` prints a
column-0 `recently run on this type: …` line (recent ∩ applicable, most-recent-
first) from `history::recent_verbs_for_type`. **Annotate-only by necessity**: the
verbs-for-a-subject listing is locked to registry order by the Gate-4 SSOT order-
equality contract (error == `goo what` == `OPTIONS.allow`), so §6.3 can mark but
not *reorder* in the CLI; the reorder/menu home is the compose-GUI (#9), which
isn't bound by that contract and will reuse `recent_verbs_for_type` verbatim.
#11 (**plugin-TOML JSON Schema** — `schema/cosmic-goo-plugin.schema.json` +
`.taplo.toml`/`#:schema` association + `tests/schema.bats`; editor validation for
plugin authors). #14 (**conversion suggestions on 415** — when a verb 415s with
no route, the error now also names the verbs that accept the subject's type
*directly* (`try a verb that accepts <type>: …`), drawn from `OPTIONS.allow` minus
the failed verb and any `destructive` verb; one shared `alt_verbs_hint` on the
no-route 415 `die` in `exec_negotiated` (not the teaching-415), so both
coercion-415 and present-verb-415 get it — both covered in
`tests/integration/suggest-415.bats`). #15 (**`goo do <addr> [verb]`** — the CLI's
explicit noun-first surface; with a verb it's a pure reorder of `goo <verb> <addr>`
that re-enters `cmd_verb` verbatim (subject/object/adverb/confirm/history all
identical — locked by an equivalence test), with no verb it's the verb-pick that
delegates to `goo what`; new `do` subcommand reserved against alias shadowing;
`tests/integration/do.bats`). Note the deliberate asymmetry: `goo do <addr>`
*lists* (discovery) where bare `goo <addr>` *runs* the default verb (§4.4).
#9 **inc 1** (**compose-GUI v2 noun-first flow** — the GUI payoff slice, first
increment: subject pane → verb pane (`OPTIONS.allow` **recency-reordered** — the
GUI is freed from the CLI's Gate-4 order-equality, making this the §6.3
menu-reorder home) → live CLI-equivalent preview (#10 "speak it back") → confirm
pane → Run (spawns `goo argv`; confirm/destructive verbs run with
`--confirm-dangerous=<verb>` since a spawned `goo` has no stdin for the y/N gate).
The logic is the pure, unit-tested `goo_engine::compose::ComposeState`; the
scripted `goo compose` CLI drives the **same** core, so the bats suite tests it
headlessly. Stay on iced 0.14 (libcosmic is a separate cross-cutting swap). Build
with `make build-gui` / run with `make run-gui`).
#9 **inc 2 + the gnome-do/Kupfer rework** — the dialog is now a keyboard-driven
launcher: side-by-side **Subject → Verb → Object** panes, **type-to-filter**
(fuzzy, ranked) at each step, ↑/↓ to move, Enter/Tab to pick+advance, Esc to
clear/step-back/cancel; the object pane appears for two-step verbs (candidates by
`mime::is_subtype` of the verb's `object_type`); the speak-it-back preview stays
pinned. **All** interaction logic — pane state machine, query editing, selection,
commit/advance/back, the gated-confirm beat — is a pure, unit-tested reducer
(`goo_engine::compose::ComposeUi`), so the iced shell only performs the I/O the
reducer returns (`ResolveSubject`/`LoadObjects`/`Run`/`Cancel`). **Safety invariant
(tested)**: the keypress that *completes* a sentence never also runs it (it
advances to a Ready pane), and a gated verb needs an extra armed beat — a reflex
double-Enter can't fire a `[!!]` verb. **Next**: the adverb/slot panel (key-value
widgets from `OPTIONS.verbs.<v>.with`; verbs run on defaults until then),
**inc 3** (the #6 implicit-subject caption), and **#12** (§6.6 late-binding +
§6.7 error-recovery UI).

---

## 9. Open questions

1. ~~Inference threshold/margin defaults~~ — **resolved**: the threshold
   model is now bands (DEFINITIVE / HIGH / MEDIUM / LOW) with
   context-adaptive boundaries (script / TTY / GUI). Internal floors
   (`EXACT_FLOOR=800`, `HIGH_FLOOR=200`, `MEDIUM_FLOOR=60`) and the 2×
   HIGH ratio are picked to clear the natural gaps in the scoring
   distribution; per-source weights override per-source. See §3.2 for
   the full spec.
2. **`inferable` per-source default policy** — listed defaults in §3.3;
   verify by walking each shipped source.
3. ~~**Cache invalidation** for the entity cache~~ — **resolved** (`c673cf4`):
   watch/mtime invalidation with a no-stale guarantee; a source caches only with
   a `watch` list, and command/dbus sources (network, bluetooth) simply recompute
   every run on the one-shot CLI until the `good` daemon adds inotify/dbus
   listeners (#31). See §3.3.
4. **Noun-first CLI form** — `goo do <addr>`? `goo <addr> <verb>` with
   reorder? `goo --on <addr> <verb>`? The first is cleanest; the
   second is most ergonomic; the third is most explicit. Pick one.
5. **Smart-default learning** (§6.9) — privacy/opt-in story matters; off
   by default with a one-line `learned_defaults = true` opt-in seems
   right. Storage location.
6. **Multi-modal subject** (§6.4) — does it stay shell-fan-out, or does
   goo grow first-class multi-subject grammar (probably shell-fan-out
   for v1; revisit if it becomes a real pain).
7. **Inline launcher's home** — pop-launcher meta-plugin is the
   long-stated target. The data-entry UX above assumes it; building it
   is its own substantial arc gated on the daemon (`good`) or a
   different IPC.

---

## 10. Cross-references

- Polymorphism + OPTIONS surface — `doc/control-center.html`,
  `doc/launcher-landscape.html`.
- Addressing layer — `doc/design/addressing-and-protocol.md`.
- Negotiation + planner — `doc/design/negotiation.md`.
- Detection signals — `doc/design/detection.md`.
- Quickstart — `doc/quickstart.md`.
