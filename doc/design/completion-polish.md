# Completion polish bundle — spec + check-in plan

Roadmap slices **#1 + #2 + #3** of [data-entry-ux.md §8](data-entry-ux.md#8-implementation-roadmap), built as one cohesive arc with consistency gates between them. Three small, visible wins that establish the **chip vocabulary** the rest of the data-entry UX will inherit.

---

## 1. Bundle scope

| # | Slice | Where data lives | What renders |
|---|---|---|---|
| 1a | **Confirm chip** `[!]` / `[!!]` | `OPTIONS.verbs.<n>.confirm` / `destructive` (new) | `goo describe` / `goo what` / future zsh-fish-GUI |
| 1b | **Polymorphism, visible via enumeration** | `registry::verb_contributor_counts` (engine helper for `goo describe`); `__complete verbs` deduped so the menu is clean | `goo describe` shows `×N` + contributors; bash tab-completion shows polymorphism *implicitly* via the heterogeneous subject menu at `goo <verb> <TAB>` |
| 2  | **Subjectless announcement** | `OPTIONS.verbs.<n>.needs_subject` (new, derived from `accepts`) | post-space hint line |
| 3  | **GOO-default disambiguation** | `OPTIONS.allow` (existing — reused ordering, NOT a new rank) | error message body |

**Out of scope** (deferred to follow-ups, named so they don't sneak in):
- Visual chip render in compose-GUI (slice #9 territory).
- Inference nudges (`≈` chip) — those are slice #7's UX.
- Per-contributor qualified completion (`stop (network)` as a selectable token) — needs display-vs-insert support; mechanical for zsh/fish, not for bash. Deferred.

---

## 2. Chip vocabulary (single source of truth)

Every surface that displays chips cites this table. If a future zsh/fish/GUI port invents its own glyph, that's a bug.

| Chip   | Meaning                              | Source of truth                          | Surfaces (v1) |
|--------|--------------------------------------|------------------------------------------|---------------|
| `[!]`  | verb has `confirm: true`             | `OPTIONS.verbs.<n>.confirm`              | `goo describe`, `goo what`, zsh/fish, GUI |
| `[!!]` | verb has `destructive: true`         | `OPTIONS.verbs.<n>.destructive`          | same as above |
| `×N`   | N contributors share this verb name  | `registry::verb_contributor_counts`      | `goo describe` only; **bash tab-completion conveys polymorphism via enumerated heterogeneous subjects at `goo <verb> <TAB>`** (see §3, D3) |
| `(no subject)` | verb's `accepts` is empty/wildcard | `OPTIONS.verbs.<n>.needs_subject == false` | bash hint line, `goo describe`, GUI |
| `≈`    | *(reserved — inferred subject, slice #7)* | —                                   | future |

Glyphs picked to be ASCII-clean (terminal-safe), short, and visually distinct. Chips are **purely informational** — they never alter the inserted token in shell completion (see §5).

---

## 3. Decisions to lock NOW (before code)

These three answers go in the commit messages and the relevant code comments. Don't defer.

**D1. `--no-confirm` and the `[!]` chip.** The override changes the *run-time gate*, not the verb's *nature*. → Chip ALWAYS shows when `confirm: true`, regardless of `--no-confirm` flag state. Rationale: the chip is "this verb wants confirmation"; the override is "I, the caller, am suppressing it this once." Conflating them hides what the verb's TOML actually declares.

**D2. Polymorphic ×N at subject-time.** Once a subject is in play, the registry merge has dispatched to the matching impl — ×N is no longer a meaningful signal for that subject. → Per-subject OPTIONS does NOT carry ×N. The count lives in `registry::verb_contributor_counts` (a small registry-level helper) and surfaces through `goo describe`. Subject-OPTIONS answers "what can I do with X"; the contributor count answers "what verbs exist in the language and how polymorphic are they" — two different questions, two different surfaces.

**D3. Bash polymorphism affordance — enumeration, not chips.** `compgen -W` inserts the displayed token literally, so there's no native bash way to show `firefox×3` as display-only without it getting typed. Rather than fight that constraint with strip-on-insert hacks, we lean on bash's existing double-tab affordance:

  - **Verb stage** (`goo <TAB>`): dedupe so each verb name appears once. (Today's behaviour is worse than `×N` — `stop` literally shows three times in the menu because the merged registry has three entries with that name. The dedup is an incidental UX win.)
  - **Subject stage** (`goo <verb> <TAB><TAB>`): subjects from *all* dispatching sources show in the same list — `:net:wifi0`, `:bt:device-x`, `:mpris:player` for `goo stop`. The heterogeneous menu IS the polymorphism revelation, delivered at the moment it's actionable (subject pick = dispatch disambiguation).
  - **Explicit metadata** (`goo describe <verb>`): full chip rendering for users who want it (`stop — 3 contributors: network, bluetooth, mpris` plus per-contributor accepts/plugin). Same chips render in `goo what <addr>` for the subject-applicable list.
  - **Confirm/destructive chips** still ride OPTIONS — they're rendered in `goo describe`, `goo what`, and (later) zsh's `_describe` / fish's `complete -d` / GUI. Same data, surface-appropriate render.

  Net: bash tab-completion returns clean tokens (no fragility); the chip *data* ships in OPTIONS + registry-summary; chip *render* in v1 lives in dedicated listing surfaces (`goo describe` / `goo what`) plus mechanical-port-readiness for zsh/fish.

---

## 4. Check-in cadence — four gates

Lightweight, concrete. Each gate has an artifact and one question to answer; if the artifact doesn't exist or the answer isn't documented, the slice isn't done.

**Status:** Gate 1 ✓ (this doc commits the chip vocabulary). Bundle starts at Gate 2.

### Gate 1 — Before any code: chip vocabulary committed ✓
**Artifact:** §2 of this doc (above), referenced from data-entry-ux.md.
**Question:** Are the four chips' meanings and source-of-truth columns stable?
**Why:** Future zsh/fish/GUI ports must cite, not invent.

### Gate 2 — OPTIONS gets new fields (end of slice 1a, slice 2)
**Artifact:** `SCHEMA_VERSION` bumped (`0.1` → `0.2` — still `stable: false`, this is just dev hygiene so consumers can gate); `projection_never_leaks_internal_verb_fields` test in `crates/goo-engine/src/options.rs` updated to explicitly allow the new keys (`confirm`, `destructive`, `needs_subject`) — never just relaxed; new positive test that asserts each new field surfaces on every verb (presence, default-false bool form — see field-shape decision in §6 slice 1).
**Question:** Did every new field get an explicit allow-listing in the leak test AND a positive presence-on-every-verb assertion?
**Why:** The leak test is the contract the daemon-as-transport will wrap. Implicit relaxation is the path to drift. The presence assertion locks the "consumers can rely on it being there" property.

### Gate 3 — Registry helper stays out of OPTIONS (slice 1b)
**Artifact:** `registry::verb_contributor_counts(reg) -> Map<String, usize>` lives in `crates/goo-engine/src/registry.rs` with its own test; `goo describe` consumes it directly; it is NOT folded into the per-subject `OPTIONS` projection.
**Question:** Is the polymorphism count addressed by a separate helper, kept out of per-subject OPTIONS?
**Why:** Per-subject OPTIONS answers "what can I do with X"; contributor count answers "what verbs exist in the language." Keeping them apart keeps each shape easy to reason about as the language grows.

### Gate 4 — Behavioural consistency (end of bundle)
**Artifact:** A new test file `tests/completion_polish.bats` (or extension of existing) that asserts the cross-consumer SSOT property — *two different code paths reading the same projection prove identical results*:
  - **Triple-equality test (the SSOT proof):** slice 3's GOO-default disambiguation error lists its top-5 verbs in identical order to the first-5 of `goo what <addr>` output, in identical order to the first-5 of `OPTIONS.allow` for the same subject. Three surfaces, one source — divergence is a bug in the projection, not a preference.
  - **Shell-safety:** `__complete` never writes to stderr, never errors on garbage input (inherits the existing degrade-to-empty pattern from `options-allow`).
  - **Chip presence:** confirm/destructive chips appear in `goo describe <verb>` output if and only if the verb's TOML declares the flag — and the chip rendering matches §2's vocabulary glyph-for-glyph.

**Question:** Do all chip-consuming surfaces (CLI fall-through, `goo what`, future compose-GUI, future zsh/fish) read from the same OPTIONS / registry-summary projection? The triple-equality test is the *proof*; it would catch a future code path that re-implements ranking locally and silently diverges.
**Why:** This is what "single source of truth" actually means at the wire. Closes the bundle.

---

## 5. The shell-safety invariant (carried forward)

`__complete` MUST: never crash the shell, never write to stderr, never block on the network. New stages inherit the existing pattern from `options-allow` (lines 1416-1426 of `crates/goo/src/main.rs`): degrade to "no candidates" on resolve / parse / IO failure.

One-line check in each new stage; explicit comment so future contributors don't accidentally remove it.

---

## 6. Per-slice tasks

### Slice 1 — confirm/destructive chips + polymorphism affordance (task #19)

**Field-shape decision (lock now):** `confirm` and `destructive` are emitted as `bool` always-present with default `false` (NOT `Option<bool>`). Consumers can rely on presence; no branching on missing-vs-set. Documented in a comment next to the field in `options.rs`. Leak test asserts presence on every verb. `destructive` is a NEW field plugin TOMLs can adopt — verified absent from current plugins via grep; ships as default-false everywhere for now, follow-up walks plugins to tag genuinely-destructive verbs.

**Engine** (`crates/goo-engine/`):
  - `options.rs`: add `confirm: bool` and `destructive: bool` to `verb_options`'s output map (both default-false). Bump `SCHEMA_VERSION` → `0.2`. Update leak test (explicit allow-list), add positive presence-on-every-verb test.
  - `registry.rs`: new `verb_contributor_counts(reg) -> Map<String, usize>` helper. Test.

**Bin** (`crates/goo/src/main.rs`):
  - Dedupe `__complete verbs` (each verb name once) — incidental UX win; this is what makes the "double-tab to see polymorphism via subject menu" affordance clean.
  - New `goo describe <verb>` subcommand → prints verb name + chips + accepts + per-contributor breakdown. Reads OPTIONS + `verb_contributor_counts`.
  - **Registration:** add `describe` to the `__complete subcommands` listing (~line 1304) AND to the main `match` dispatch (~line 1110) — easy to forget; this is the surface that makes it discoverable.

**Expected `goo describe` output** (lock the format now so chip render doesn't drift):
```
$ goo describe stop                        # ×N + per-contributor (polymorphic)
stop  ×3
  network    [stop a network connection]  accepts: vnd.cosmic-goo.network/*   cmd: nmcli ...
  bluetooth  [disconnect a BT device]     accepts: vnd.cosmic-goo.bluetooth/* cmd: bluetoothctl ...
  mpris      [pause media playback]       accepts: vnd.cosmic-goo.mpris/*     cmd: playerctl ...

$ goo describe poweroff                    # [!] confirm chip + single contributor
poweroff  [!]
  power      [systemctl poweroff]         accepts: */*                        cmd: systemctl poweroff
```

**Doc:** chip vocabulary section (§2 above) merged ✓.

**Done when:** Gates 2 (partial) + 3 pass; `goo describe stop` matches the format above with `×3`; `goo describe poweroff` shows `[!]`; `goo <TAB>` no longer shows duplicate verb names; OPTIONS for a confirm-flagged verb carries `confirm: true`.

### Slice 2 — subjectless verb announcement (task #20)
**Engine:**
  - `options.rs`: add `needs_subject: bool` (default-true; false when verb's `accepts` is empty or wildcard-only) to `verb_options`. Update leak test, positive test.
**Bin:**
  - New `__complete verb-needs-subject <verb>` stage → returns `yes|no` by reading the verb's `accepts` directly from the registry (NOT via `options::options_for` — that would filter to subject-applicable verbs and may exclude the verb being asked about). The verb's `needs_subject` is a property of its TOML declaration, not subject-dependent. Shell-safe.
**Shell** (`completions/goo.bash`):
  - After a verb name has been completed and a space follows, if `verb-needs-subject` returns `no`, surface a one-line hint indicating the verb takes no subject.
  - **Hazard to verify with real bash, not assumption:** entries starting with `#` are typed literally if selected — bash does NOT treat them as comments at completion time. The spike needs to check whichever variant doesn't have that hazard. Two candidates: `compopt -o nosort` + a description-style sentinel entry that uses `compopt -o nospace`; or `_init_completion`-style message line (varies by bash-completion version). Pick the one that works on bash 4.4+ without surprising literals.
**Done when:** Gate 2 covers the new field; bash shell-test confirms the new stage works on real bash; a subjectless verb (`apps`, `plugins`, `help`) shows the hint after `<verb>` + space, and selecting the hint does not type its visible text.

### Slice 3 — GOO-default disambiguation (task #21)
**Bin** (`crates/goo/src/main.rs`):
  - In the GOO dispatch path: when no `default_for` for the resolved type, build an error message using `OPTIONS.allow` (top 5) — re-use the existing `options::options_for` call, do not invent a new ranking.
  - New `goo what <addr>` subcommand (informational) that prints the same `allow` list with chips; the error message points to it.
  - **Registration:** add `what` to the `__complete subcommands` listing (~line 1304) AND to the main `match` dispatch (~line 1110).

**Expected output formats** (lock now to prevent drift):
```
$ goo :file/some.md                        # dispatch fails, no default for text/markdown
goo: no default verb for type text/markdown
  top 5 applicable verbs:
    critique     critique the markdown via fabric
    summarize    summarize via fabric
    view         render markdown to terminal
    edit         open in $EDITOR
    open  [!]    xdg-open
  full list:  goo what :file/some.md

$ goo what :file/some.md                   # informational, full applicable-verbs list
applicable verbs for :file/some.md  (type: text/markdown)
  critique         critique via fabric
  summarize        summarize via fabric
  view             render markdown
  edit             open in $EDITOR
  open  [!]        xdg-open
  ...
```

**Test:** the bats file from Gate 4 — specifically the **triple-equality** SSOT test: slice 3 error's top-5 == `goo what` first-5 == `OPTIONS.allow` first-5 for the same subject. If those three diverge, the projection has a bug.

**Done when:** Gate 4 passes; the triple-equality test is green; `goo :file/some.md` (assuming text/markdown has no `default_for`) prints the format above; `goo what :file/some.md` works as an informational query.

---

## 7. What the bundle does NOT change

- The protocol document (`goo-protocol.md` §7) gets a small update naming the new per-verb OPTIONS fields. We're still developing goo — these are field additions to a `stable: false` shape, not a protocol "extension."
- No new verbs, sources, types, or plugins.
- No engine behaviour change beyond projection — the run path is untouched.
- Bash tab-completion's user-visible behaviour: dedup of `__complete verbs` (incidental cleanup) + subjectless hint after slice 2. Chips themselves render in `goo describe` / `goo what` and (later) zsh/fish — bash users get the data, surfaced where it doesn't fight the shell.

---

## 8. Cross-references

- [data-entry-ux.md](data-entry-ux.md) — the parent design; §5.2, §5.5, §5.6 are the wins this bundle delivers.
- [goo-protocol.md §7](goo-protocol.md#7-options--discovery-and-completion-oracle) — OPTIONS as the completion oracle.
- [control-center.html](../control-center.html) — future GUI surface that will consume the same projections.
- Tasks: #19 (slice 1), #20 (slice 2), #21 (slice 3).
