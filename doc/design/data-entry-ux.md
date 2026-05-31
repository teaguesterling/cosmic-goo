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

### 3.2 Entity-name inference — how it ranks

For a bare token `t`:

```
candidates = []
for each enumerable source in registry (or all sources if expensive lookup OK):
    items = cached_list_cmd(source)         // §3.3 caching
    for each item in items:
        score = 0
        if item.id == t:                    score = ∞ (exact)
        elif item.id contains t (whole):    score += 1000
        elif item.title == t:                score += 800
        elif item.title contains t:          score += 100 * (matchlen / titlelen)
        elif item.id contains t:             score += 50
        // Source-priority weight (user-configurable; defaults below)
        score *= source.weight (default 1.0; :app ~ 1.2, :hist ~ 0.6, :ps ~ 0.7)
        // Recency: bonus if source items are sorted by recency and idx is small
        if source.recency_ordered and item.idx < 10:  score += 20 - 2*item.idx
        candidates.push({source, item, score})

candidates.sort_by_score_desc
if top.score < THRESHOLD:                                → fall through to text/plain
elif top.score - second.score < MARGIN:                  → ambiguous: picker / "did you mean: …"
else:                                                    → resolve as top.source/top.item
```

Threshold + margin are tunable knobs (CLI: `goo --config inference.threshold=500`). Sensible defaults:
- `THRESHOLD = 80` (substring on a title, anything weaker is text content).
- `MARGIN = 300` (one clear winner vs. multiple plausible).

### 3.3 Performance & caching

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

When the bare token follows a verb (`goo connect fox`), the inference
*biases toward sources whose emits matches the verb's accepts*:

- `connect fox` → verb `connect` accepts `vnd.ssh.host`/`vnd.bluez.device`/`vnd.nm.connection`; inference only considers `:ssh`, `:bt`, `:net` sources. "fox" matches no ssh host but might match a BT device "Foxconn-headset" → resolved.
- `open fox` → verb `open` accepts `inode/*` and `text/x-uri`; inference considers `:file`, `:recent`, `:mnt` (vendor inode-subtypes), and falls back to URL shapes. "fox" matches recent file `fox-recipe.md`.
- `summarize fox` → verb `summarize` accepts `text/*`; inference considers `:recent` (text files), `:clip`, `:sel`; matches a recent text file containing "fox" or current clipboard if it starts with "fox".

This dramatically narrows the candidate pool and improves both relevance
and performance.

### 3.5 Ambiguity handling

When the top match's lead over the second isn't decisive (`MARGIN` not
met):

- **CLI**: don't silently pick. Print: `firefox is ambiguous — pick one:`
  followed by a numbered list. User can repeat the command with the
  picked address. Or pass `--fuzzy` to accept the top.
- **Compose-GUI**: surface the picker UI directly; user picks; flow
  continues.
- **Inline launcher**: ranked list as suggestions; user picks with arrow keys.

Threshold-not-met (no real candidate): silently fall through to text/plain.

### 3.6 Privacy & determinism

- **Privacy**: by-default inferable sources are those a user expects to
  query interactively. Sensitive sources (clipboard-history, ssh,
  containers) are opt-in. Surfacing what's queried at completion time is
  honest; the `inferable` flag is documented.
- **Determinism for automation**: scripts should still use explicit
  sigils. The CLI prints a *one-shot deprecation-style nudge* when an
  inference fires non-interactively: `inferred firefox → :app/firefox
  (use :app/firefox in scripts for stability)`. Suppressible with
  `--no-inference-warning` or env var.

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
2. **Verb-pick after a noun (CLI)**. Today after `goo :app/firefox <space>` the parser expects a second positional (object). The grammar's ambiguous here. Possible forms:
   - `goo :app/firefox close` → ambiguous (is `close` the verb or a token to pass?). Could disambiguate by: if the second positional is a known verb AND the first positional is an address, reinterpret as `goo close :app/firefox` (reorder). Subtle but workable. Optional `--reorder` flag for explicit opt-in.
   - `goo --on :app/firefox close` → explicit "noun-first" marker. Unambiguous. Adds a flag.
   - `goo do <addr> [verb]` → new subcommand that pops verb-pick if verb omitted. Cleanest CLI surface.
3. **Inline-launcher noun-first** — the pop-launcher meta-plugin (planned, not built). Type → entity matches → Enter for default verb, → opens actions menu. Spotlight-style.
4. **Compose-GUI noun-first** — the GUI builds noun-first by default: subject candidates surface first (PRIMARY/clip/recent + fuzzy source items), then verb-pick from OPTIONS.allow.

### 4.4 The grammar split, concretely

| Surface | Default grammar | When to break it |
|---|---|---|
| CLI (`goo`) | **verb-first** | `goo <addr>` no-verb → GOO default. Optional `goo do <addr>` for noun-first verb-pick. |
| Shell completion | follows CLI | augmented with entity inference (§3) at the verb-position |
| Compose-GUI | **noun-first** | Subject first, then verb. CLI-equivalent preview shown live. |
| Inline launcher | **noun-first** | Spotlight pattern; one keystroke discovers entities. |
| `goo dispatch <raw>` | content-classifier (separate pattern) | regex → route |

---

## 5. The improvements (cross-cutting wins)

These came out of the input walk. Each is small-to-medium scope; most
are independent.

### 5.1 Subject-shape-aware listing (§B-8, B-9, C-15)
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

### 6.8 Conversion suggestions on 415
Today the teaching 415 (negotiation §4.1) suggests `--hops N` or
`--force`. Extend: also suggest *alternative verbs* that would reach the
goal from the subject. "View won't render this as ANSI; try `cat` or
`json-pretty`." Powered by OPTIONS.allow filtered to verbs that accept
the subject's type AND emit something close to Accept.

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
| Noun-first verb-pick | not supported | `goo do <addr> [verb]` (new subcommand) OR `goo <addr> <verb>` with `--reorder` | additive |
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
| 7 | **Entity-name inference v1** (§3.2 with caching) | substantial | new `address::infer_entity` + cache layer |
| 8 | **Verb-aware bias** (§3.4) | medium-substantial | layered on top of #7 |
| 9 | **Compose-GUI v2 noun-first flow** | substantial | the GUI payoff slice |
| 10 | **"Speak it back" live preview** in compose-GUI | small once #9 lands | |
| 11 | **JSON Schema for plugin TOML** (§6.12) | small | static file + docs |
| 12 | **Late-binding / error recovery** in compose-GUI | medium | UX correctness work |
| 13 | **"Again" / recent-actions** (§6.1, §6.3) | medium | persistent history layer |
| 14 | **Conversion suggestions on 415** (§6.8) | medium | extends teaching 415 |
| 15 | **`goo do <addr>`** noun-first subcommand | small | bin |

**Suggested first slice**: #1 + #2 + #3 together as "completion polish"
(all small, all visible). Then #4 as the first inference taste. Then
either #7 (the keystone — entity inference) or #9 (compose-GUI v2) as
the next big arc, depending on appetite.

---

## 9. Open questions

1. **Inference threshold/margin defaults** — I proposed 80/300, but the
   right numbers come from instrumenting real use. Start with these,
   adjust based on "ambiguous picker fires too often / not often
   enough."
2. **`inferable` per-source default policy** — listed defaults in §3.3;
   verify by walking each shipped source.
3. **Cache invalidation** for the entity cache — per-source signals
   listed in §3.3; some sources (network, bluetooth) might need dbus
   listeners to be tight, vs the lazier TTL approach.
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
