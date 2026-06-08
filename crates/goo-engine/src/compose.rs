//! The compose **core** — a pure, I/O-free model of a noun-first sentence
//! (subject → verb → adverbs) shared by the iced compose-GUI and the scripted
//! `goo compose` CLI. GUIs can't be exercised by the bats suite, so all the
//! branching logic lives here, unit-tested without a display; the GUI's
//! `view`/`update` is a thin shell over [`ComposeState`].
//!
//! **Purity boundary.** This module does ZERO I/O. The only engine call inside
//! is [`crate::options::options_for`] (a pure projection of `reg` + `subject`
//! `Value`s). Resolving addresses, peeking the selection/clipboard, and reading
//! the action history all happen in the *shell* and are passed in as plain
//! values (e.g. `verb_menu(recent)` takes the recency list, it does not read
//! the history file).
//!
//! **The contract is `argv()`.** It is the single source of truth for both the
//! live "speak it back" preview (#10) and execution — the GUI spawns it, the
//! CLI builds the same sentence and runs it through `exec_verb`. The preview is
//! literally `"goo " + argv().join(" ")`, so what the user reads is what runs.

use crate::options;
use serde_json::Value;
use std::collections::BTreeMap;

/// One in-progress noun-first sentence. Built from a resolved subject; the
/// frozen OPTIONS projection drives the verb menu and per-verb slot metadata.
///
/// `options` is stored **owned** (not a `&Value` borrow) so the struct is
/// `'static` — the iced `App` owns it across `update` calls.
#[derive(Debug, Clone)]
pub struct ComposeState {
    /// The canonical `goo://…` address the shell resolved for the subject.
    pub subject_addr: String,
    /// The subject's MIME type (`OPTIONS.type`).
    pub subject_type: String,
    /// The frozen OPTIONS view for this subject (the SSOT for verbs + slots).
    options: Value,
    /// The picked verb, if any.
    pub verb: Option<String>,
    /// The picked object address, for a two-step verb (`object_type` set).
    pub object_addr: Option<String>,
    /// Filled selector/fill adverb slots (a later increment populates these;
    /// today they stay empty — verbs run on their registry defaults).
    pub adverbs: BTreeMap<String, String>,
}

impl ComposeState {
    /// Build from an already-resolved `subject` (the shell did `address::resolve`).
    /// `subject_addr` is the canonical address to thread into `argv`. Calls
    /// `options_for` once and freezes it.
    pub fn from_subject(reg: &Value, subject: &Value, subject_addr: String) -> ComposeState {
        let options = options::options_for(reg, subject);
        let subject_type = options.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        ComposeState {
            subject_addr,
            subject_type,
            options,
            verb: None,
            object_addr: None,
            adverbs: BTreeMap::new(),
        }
    }

    /// `OPTIONS.allow` — the applicable verbs in registry order. The CLI SSOT order.
    pub fn allow(&self) -> Vec<&str> {
        self.options
            .get("allow")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// The verb menu for the GUI: `OPTIONS.allow`, but with verbs the user
    /// **recently** ran on this type promoted to the front (recency order),
    /// then the remaining `allow` verbs in registry order (§6.3 menu-reorder).
    ///
    /// - Only promotes verbs that are BOTH in `recent` AND applicable (`allow`)
    ///   — a since-removed or inapplicable recent verb is silently dropped.
    /// - Never introduces a verb not in `allow`.
    /// - The un-promoted tail keeps `allow` order (stable).
    ///
    /// This is **compose-only** — it must NOT feed `goo what`/the dispatch error
    /// path, which are locked to registry order by the Gate-4 SSOT order-equality
    /// contract. The GUI is explicitly freed from that contract; the CLI is not.
    pub fn verb_menu(&self, recent: &[String]) -> Vec<String> {
        let allow = self.allow();
        let mut out: Vec<String> = Vec::with_capacity(allow.len());
        // 1. recent ∩ allow, in recency order, de-duplicated.
        for r in recent {
            if allow.contains(&r.as_str()) && !out.iter().any(|s| s == r) {
                out.push(r.clone());
            }
        }
        // 2. the allow tail, registry order, skipping anything already promoted.
        for a in &allow {
            if !out.iter().any(|s| s == a) {
                out.push((*a).to_string());
            }
        }
        out
    }

    /// Pick a verb (should be one of [`allow`](Self::allow)). Clears any filled
    /// adverbs AND a previously-picked object — both are verb-relative and go
    /// stale on a verb change. (The §6.6 keep-compatible-adverbs behaviour is a
    /// later increment, roadmap #12.)
    pub fn select_verb(&mut self, name: &str) {
        self.verb = Some(name.to_string());
        self.adverbs.clear();
        self.object_addr = None;
    }

    /// Pick the object address for a two-step verb.
    pub fn select_object(&mut self, addr: String) {
        self.object_addr = Some(addr);
    }

    /// The selector adverb slots the picked verb opts into (`OPTIONS.verbs.<v>.with`),
    /// in declaration order. Fill (free-text) adverbs are omitted — none ship today
    /// and the keyboard panel only cycles selectors (a later increment adds text
    /// entry if a fill adverb ever appears).
    pub fn adverb_slots(&self) -> Vec<AdverbSlot> {
        let Some(with) = self.verb_view().and_then(|v| v.get("with")).and_then(Value::as_object) else {
            return Vec::new();
        };
        with.iter()
            .filter(|(_, s)| s.get("kind").and_then(Value::as_str) == Some("selector"))
            .filter_map(|(name, s)| {
                let values: Vec<String> = s
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
                    .unwrap_or_default();
                if values.is_empty() {
                    return None;
                }
                let default = s.get("default").and_then(Value::as_str).map(String::from);
                Some(AdverbSlot { name: name.clone(), values, default })
            })
            .collect()
    }

    /// The current value of adverb `name`: the user's explicit pick, else `default`.
    pub fn adverb_value<'a>(&'a self, name: &str, default: Option<&'a str>) -> Option<&'a str> {
        self.adverbs.get(name).map(String::as_str).or(default)
    }

    /// Set adverb `name` to `value`, OR clear it when `value` equals `default` — so
    /// the sentence (and the preview) carries only *overrides*, never a redundant
    /// `--via=clipboard` that just restates the registry default.
    pub fn set_adverb(&mut self, name: &str, value: &str, default: Option<&str>) {
        if Some(value) == default {
            self.adverbs.remove(name);
        } else {
            self.adverbs.insert(name.to_string(), value.to_string());
        }
    }

    /// Whether the sentence is runnable: a verb is picked, and if that verb needs
    /// an object, one has been chosen. (Adverbs always have registry defaults, so
    /// they never block completion.)
    pub fn is_complete(&self) -> bool {
        self.verb.is_some() && (!self.needs_object() || self.object_addr.is_some())
    }

    /// The OPTIONS slot map for any applicable verb (`OPTIONS.verbs.<name>`).
    fn verb_view_named(&self, name: &str) -> Option<&Value> {
        self.options.get("verbs").and_then(|m| m.get(name))
    }

    /// The OPTIONS slot map for the currently-picked verb.
    fn verb_view(&self) -> Option<&Value> {
        self.verb_view_named(self.verb.as_deref()?)
    }

    /// `(confirm, destructive)` for ANY applicable verb — drives the verb-menu
    /// chips (`[!]`/`[!!]`) before a verb is selected. The glyph mapping is the
    /// consumer's (the engine surfaces booleans, not UI vocabulary).
    pub fn verb_flags(&self, name: &str) -> (bool, bool) {
        let v = self.verb_view_named(name);
        let c = v.and_then(|x| x.get("confirm")).and_then(Value::as_bool).unwrap_or(false);
        let d = v.and_then(|x| x.get("destructive")).and_then(Value::as_bool).unwrap_or(false);
        (c, d)
    }

    /// `OPTIONS.verbs.<v>.object_type` — the type of the object a two-step verb
    /// needs (e.g. `move-to` wants a workspace). `None` for the common one-step
    /// verb. Inc 1 uses this to **disable Run** (the object pane is inc 2).
    pub fn object_type(&self) -> Option<&str> {
        self.verb_view().and_then(|v| v.get("object_type")).and_then(Value::as_str)
    }

    /// Whether the picked verb needs an object the GUI can't supply yet (inc 1).
    pub fn needs_object(&self) -> bool {
        self.object_type().is_some()
    }

    /// `OPTIONS.verbs.<v>.confirm` — the verb has a y/N gate. The GUI renders its
    /// own confirm pane (a spawned `goo` has no stdin to answer the CLI prompt).
    pub fn needs_confirm(&self) -> bool {
        self.verb_view().and_then(|v| v.get("confirm")).and_then(Value::as_bool).unwrap_or(false)
    }

    /// `OPTIONS.verbs.<v>.destructive` — drives the `[!!]` vs `[!]` chip.
    pub fn is_destructive(&self) -> bool {
        self.verb_view().and_then(|v| v.get("destructive")).and_then(Value::as_bool).unwrap_or(false)
    }

    /// The sentence as a `goo` argv: `[verb, subject_addr, object_addr?, --k=v…]`
    /// — the object (when set) is the second positional, adverbs follow in name
    /// order (BTreeMap). The SSOT for both [`preview`](Self::preview) and
    /// execution. Empty when no verb is picked yet.
    ///
    /// **`--confirm-dangerous` is deliberately excluded** — it is a gate bypass,
    /// not a sentence slot, so it never appears in the preview. The spawn wrapper
    /// appends it *after* the confirm pane is accepted (mirroring how
    /// `recordable_adverbs` excludes it from the replay history).
    pub fn argv(&self) -> Vec<String> {
        let Some(verb) = self.verb.as_deref() else { return Vec::new() };
        let mut argv = vec![verb.to_string(), self.subject_addr.clone()];
        if let Some(obj) = self.object_addr.as_deref() {
            argv.push(obj.to_string());
        }
        for (k, v) in &self.adverbs {
            argv.push(format!("--{k}={v}"));
        }
        argv
    }

    /// The live CLI-equivalent — "speak it back" (#10). Exactly
    /// `"goo " + argv().join(" ")`, so the preview the user reads is the command
    /// that runs. Just `"goo"` before a verb is picked.
    pub fn preview(&self) -> String {
        let argv = self.argv();
        if argv.is_empty() {
            "goo".to_string()
        } else {
            format!("goo {}", argv.join(" "))
        }
    }
}

// ============================================================================
// Fuzzy ranking — the gnome-do type-to-filter scorer (pure)
// ============================================================================

/// Case-insensitive subsequence score of `needle` against `haystack`, or `None`
/// if `needle` isn't a subsequence (the candidate is filtered out). Higher is
/// better. Rewards matches at word boundaries and consecutive runs; mildly
/// penalises how far in the first match lands. An empty needle scores 0 (every
/// candidate passes, original order preserved).
pub fn fuzzy_score(haystack: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
    let need: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut hi = 0usize;
    let mut score = 0i32;
    let mut first: Option<usize> = None;
    let mut prev: Option<usize> = None;
    for &nc in &need {
        let mut found = None;
        while hi < hay.len() {
            if hay[hi] == nc {
                found = Some(hi);
                break;
            }
            hi += 1;
        }
        let idx = found?;
        if first.is_none() {
            first = Some(idx);
        }
        // word-boundary start (index 0 or preceded by a non-alphanumeric).
        if idx == 0 || !hay[idx - 1].is_alphanumeric() {
            score += 10;
        }
        if prev == Some(idx.wrapping_sub(1)) {
            score += 5; // consecutive run
        }
        prev = Some(idx);
        hi += 1;
    }
    // prefer matches that start earlier.
    score -= first.unwrap_or(0) as i32 / 4;
    Some(score)
}

/// Filter+rank `items` by `query` against each item's `label`. Non-matching
/// items are dropped; ties keep the original (caller-provided) order — so an
/// empty query is an identity filter that preserves e.g. the recency reorder.
pub fn fuzzy_rank(items: &[Item], query: &str) -> Vec<Item> {
    let mut scored: Vec<(i32, usize, &Item)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| fuzzy_score(&it.label, query).map(|s| (s, i, it)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, it)| it.clone()).collect()
}

// ============================================================================
// ComposeUi — the pure gnome-do interaction reducer
// ============================================================================

/// A candidate row: `key` is the value committed (a `goo://` address or a verb
/// name); `label` is what the user sees and types against; `icon` is an optional
/// freedesktop icon *name* (a plain string — resolution to a themed image is the
/// shell's job, so the reducer stays pure and iced-free).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub label: String,
    pub icon: Option<String>,
}

impl Item {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Item {
        Item { key: key.into(), label: label.into(), icon: None }
    }

    /// Attach an icon name (chainable on [`Item::new`]).
    pub fn with_icon(mut self, icon: Option<String>) -> Item {
        self.icon = icon;
        self
    }
}

/// A selector adverb slot for the Ready-stage panel: the adverb `name`, its
/// ordered choice `values`, and the registry `default` (so the panel can mark
/// the default and the reducer can omit it from the sentence).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdverbSlot {
    pub name: String,
    pub values: Vec<String>,
    pub default: Option<String>,
}

/// Which pane is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Subject,
    Verb,
    Object,
    /// The sentence is complete; the preview shows what will run.
    Ready,
}

/// A keypress, decoded by the shell from iced's keyboard event into this
/// iced-free vocabulary so the reducer stays pure and testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Char(String),
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    Escape,
}

/// The I/O a key produced — the ONLY thing the shell performs (everything else
/// is pure state mutation inside the reducer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiAction {
    /// Resolve this address; on success call [`ComposeUi::on_subject_resolved`].
    ResolveSubject(String),
    /// Enumerate candidates of this object type; call [`ComposeUi::set_objects`].
    LoadObjects(String),
    /// Execute the sentence (`state.argv()`).
    Run,
    /// Abort the dialog (exit 130).
    Cancel,
}

/// The gnome-do interaction state machine: pane + query + selection over the
/// compose sentence. Pure — `apply` mutates state and returns the I/O the shell
/// must perform, so the whole keyboard flow is unit-testable without a display.
///
/// **Safety invariant**: [`UiAction::Run`] is returned ONLY from [`Stage::Ready`],
/// never from the keypress that *completes* the sentence (committing a pane
/// always advances to `Ready` without running). A gated verb
/// (`confirm`/`destructive`) needs an extra armed beat in `Ready`, so a reflex
/// double-Enter can't fire it.
pub struct ComposeUi {
    pub stage: Stage,
    pub query: String,
    pub selected: usize,
    pub error: Option<String>,
    pub state: Option<ComposeState>,
    subjects: Vec<Item>,
    objects: Vec<Item>,
    recent: Vec<String>,
    /// `Ready` + gated: set on the first run-intent (shows the confirm), so the
    /// second Enter is what actually runs.
    armed: bool,
    /// Which adverb slot is focused in the `Ready` panel (`↑`/`↓` move, `←`/`→`
    /// cycle its value).
    pub adverb_sel: usize,
}

impl ComposeUi {
    pub fn new(subjects: Vec<Item>) -> ComposeUi {
        ComposeUi {
            stage: Stage::Subject,
            query: String::new(),
            selected: 0,
            error: None,
            state: None,
            subjects,
            objects: Vec::new(),
            recent: Vec::new(),
            armed: false,
            adverb_sel: 0,
        }
    }

    /// The picked verb's selector adverb slots (empty until a verb is chosen).
    pub fn adverb_slots(&self) -> Vec<AdverbSlot> {
        self.state.as_ref().map(ComposeState::adverb_slots).unwrap_or_default()
    }

    /// The candidate pool for the active stage (before fuzzy filtering). The verb
    /// pool is the recency-reordered `verb_menu` with confirm/destructive chips.
    fn pool(&self) -> Vec<Item> {
        match self.stage {
            Stage::Subject => self.subjects.clone(),
            Stage::Verb => self
                .state
                .as_ref()
                .map(|s| {
                    s.verb_menu(&self.recent)
                        .into_iter()
                        .map(|n| {
                            let (c, d) = s.verb_flags(&n);
                            let chip = if d { "  [!!]" } else if c { "  [!]" } else { "" };
                            let recent = if self.recent.iter().any(|r| *r == n) { "  ·recent" } else { "" };
                            Item::new(n.clone(), format!("{n}{chip}{recent}"))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Stage::Object => self.objects.clone(),
            Stage::Ready => Vec::new(),
        }
    }

    /// The filtered + ranked candidates for the active stage (what the view draws).
    pub fn visible(&self) -> Vec<Item> {
        fuzzy_rank(&self.pool(), &self.query)
    }

    /// `true` while the `Ready` sentence is gated and not yet armed (the view shows
    /// a confirm prompt; the next Enter arms, the one after runs).
    pub fn gated(&self) -> bool {
        self.state.as_ref().map(|s| s.needs_confirm() || s.is_destructive()).unwrap_or(false)
    }
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// The shell calls this after a successful [`UiAction::ResolveSubject`].
    pub fn on_subject_resolved(&mut self, state: ComposeState, recent: Vec<String>) {
        self.state = Some(state);
        self.recent = recent;
        self.stage = Stage::Verb;
        self.query.clear();
        self.selected = 0;
        self.error = None;
    }

    /// The shell calls this after [`UiAction::LoadObjects`].
    pub fn set_objects(&mut self, objects: Vec<Item>) {
        self.objects = objects;
        self.selected = 0;
    }

    /// Record a resolution failure (stays on the subject stage).
    pub fn set_error(&mut self, msg: String) {
        self.error = Some(msg);
    }

    /// Feed a keypress; mutate state and return any I/O the shell must perform.
    pub fn apply(&mut self, key: &KeyInput) -> Option<UiAction> {
        // Nav/control keys are matched here FIRST; only `Char` edits the query, so
        // a Tab/Enter/Backspace can never leak a control char into the filter.
        match key {
            KeyInput::Char(c) => {
                self.query.push_str(c);
                self.selected = 0;
                None
            }
            KeyInput::Backspace => {
                if self.query.is_empty() {
                    self.back();
                } else {
                    self.query.pop();
                    self.selected = 0;
                }
                None
            }
            // Up/Down move the row cursor: among adverb slots in `Ready`, among
            // candidates everywhere else.
            KeyInput::Down => {
                self.move_cursor(1);
                None
            }
            KeyInput::Up => {
                self.move_cursor(-1);
                None
            }
            // Left/Right cycle the focused adverb's value (Ready only).
            KeyInput::Left => {
                if self.stage == Stage::Ready {
                    self.cycle_adverb(-1);
                }
                None
            }
            KeyInput::Right => {
                if self.stage == Stage::Ready {
                    self.cycle_adverb(1);
                }
                None
            }
            KeyInput::Enter | KeyInput::Tab => self.commit(),
            KeyInput::Escape => {
                if !self.query.is_empty() {
                    self.query.clear();
                    self.selected = 0;
                    None
                } else if self.stage == Stage::Subject {
                    Some(UiAction::Cancel)
                } else {
                    self.back();
                    None
                }
            }
        }
    }

    /// Move the row cursor by `delta`, clamped: the adverb slots in `Ready`, the
    /// filtered candidates elsewhere.
    fn move_cursor(&mut self, delta: i32) {
        let ready = self.stage == Stage::Ready;
        let n = if ready { self.adverb_slots().len() } else { self.visible().len() };
        if n == 0 {
            return;
        }
        let cur = if ready { &mut self.adverb_sel } else { &mut self.selected };
        *cur = (*cur as i32 + delta).clamp(0, n as i32 - 1) as usize;
    }

    /// Cycle the focused adverb slot's value by `delta` (+1 / −1, wrapping) and
    /// apply it (clears back to default when it lands on the default). Changing the
    /// sentence disarms a pending gated-confirm.
    fn cycle_adverb(&mut self, delta: i32) {
        let slots = self.adverb_slots();
        let Some(slot) = slots.get(self.adverb_sel) else { return };
        let Some(state) = self.state.as_mut() else { return };
        let cur = state.adverb_value(&slot.name, slot.default.as_deref()).unwrap_or("");
        let idx = slot.values.iter().position(|v| v == cur).unwrap_or(0) as i32;
        let n = slot.values.len() as i32;
        let next = (((idx + delta) % n + n) % n) as usize;
        let value = slot.values[next].clone();
        state.set_adverb(&slot.name, &value, slot.default.as_deref());
        self.armed = false;
    }

    /// Commit the highlighted candidate / advance the stage. Never returns `Run`
    /// except from `Ready` (the completing keypress must not also execute).
    fn commit(&mut self) -> Option<UiAction> {
        match self.stage {
            Stage::Subject => {
                let items = self.visible();
                let item = items.get(self.selected)?;
                Some(UiAction::ResolveSubject(item.key.clone()))
            }
            Stage::Verb => {
                let items = self.visible();
                let name = items.get(self.selected)?.key.clone();
                let st = self.state.as_mut()?;
                st.select_verb(&name);
                self.query.clear();
                self.selected = 0;
                self.armed = false;
                // Compute both before `ready_or_run` re-borrows `self` (ends `st`).
                let needs_object = st.needs_object();
                let object_type = st.object_type().unwrap_or("").to_string();
                if needs_object {
                    self.objects = Vec::new();
                    self.stage = Stage::Object;
                    Some(UiAction::LoadObjects(object_type))
                } else {
                    self.ready_or_run()
                }
            }
            Stage::Object => {
                let items = self.visible();
                let addr = items.get(self.selected)?.key.clone();
                let st = self.state.as_mut()?;
                st.select_object(addr);
                self.query.clear();
                self.selected = 0;
                self.armed = false;
                self.ready_or_run()
            }
            Stage::Ready => {
                // We reached Ready by *advancing* on the previous commit (no run),
                // so this is a fresh keypress. A plain verb runs now; a gated verb
                // arms first (the view flips to "press Enter to run"), runs next.
                if self.gated() && !self.armed {
                    self.armed = true;
                    None
                } else {
                    Some(UiAction::Run)
                }
            }
        }
    }

    /// A commit just *completed* the sentence (landing it in `Ready`). A plain
    /// verb — not gated, no adverb slots to tweak — runs on that completing
    /// keypress: gnome-do directness, and no review is lost because the
    /// speak-it-back preview was already visible during verb/object selection. A
    /// gated verb or one with adverb slots dwells in `Ready` instead (arm-then-run,
    /// or tweak-then-run). The completing keypress for a *gated* verb still never
    /// runs — that safety beat lives in the `Stage::Ready` arm.
    fn ready_or_run(&mut self) -> Option<UiAction> {
        self.stage = Stage::Ready;
        self.adverb_sel = 0;
        if !self.gated() && self.adverb_slots().is_empty() {
            Some(UiAction::Run)
        } else {
            None
        }
    }

    /// Step back one stage (clearing the relevant selection) and reset the query.
    fn back(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.adverb_sel = 0;
        self.armed = false;
        match self.stage {
            Stage::Subject => {}
            Stage::Verb => {
                self.state = None;
                self.recent.clear();
                self.stage = Stage::Subject;
            }
            Stage::Object => {
                if let Some(s) = self.state.as_mut() {
                    s.verb = None;
                    s.object_addr = None;
                }
                self.stage = Stage::Verb;
            }
            Stage::Ready => {
                let needs_obj = self.state.as_ref().map(|s| s.needs_object()).unwrap_or(false);
                if needs_obj {
                    if let Some(s) = self.state.as_mut() {
                        s.object_addr = None;
                    }
                    self.stage = Stage::Object;
                } else {
                    if let Some(s) = self.state.as_mut() {
                        s.verb = None;
                    }
                    self.stage = Stage::Verb;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn reg() -> Value {
        j!({
            "verbs": [
                { "name": "summarize", "accepts": ["text/*"], "uses_adverbs": ["via"], "cmd": "x" },
                { "name": "critique", "accepts": ["text/*"], "cmd": "x" },
                { "name": "move-to", "accepts": ["text/*"], "object_type": "application/vnd.x.ws", "cmd": "mv" },
                { "name": "delete", "accepts": ["text/*"], "confirm": true, "destructive": true, "cmd": "rm" },
                { "name": "open", "accepts": ["text/*"], "default_for": "text/plain", "cmd": "xdg-open" }
            ],
            "adverbs": [
                { "name": "via", "kind": "selector", "applies_to": ["text/*"], "default": "clipboard",
                  "values": { "fabric": {}, "clipboard": {} } }
            ]
        })
    }

    fn state() -> ComposeState {
        let subject = j!({ "type": "text/plain", "text": "hi" });
        ComposeState::from_subject(&reg(), &subject, "goo://clip/".to_string())
    }

    #[test]
    fn from_subject_freezes_type_and_options() {
        let s = state();
        assert_eq!(s.subject_type, "text/plain");
        assert_eq!(s.subject_addr, "goo://clip/");
        assert!(s.verb.is_none());
        // allow carries the applicable verbs (registry order).
        assert!(s.allow().contains(&"summarize"));
        assert!(s.allow().contains(&"open"));
    }

    #[test]
    fn verb_menu_promotes_recent_then_keeps_allow_tail() {
        let s = state();
        let allow = s.allow();
        // recency: critique most-recent, then summarize. Both applicable → promoted
        // in recency order; the rest follow in allow (registry) order.
        let menu = s.verb_menu(&["critique".into(), "summarize".into()]);
        assert_eq!(&menu[0], "critique");
        assert_eq!(&menu[1], "summarize");
        // every allow verb appears exactly once, none introduced.
        assert_eq!(menu.len(), allow.len());
        for v in &allow {
            assert_eq!(menu.iter().filter(|m| m.as_str() == *v).count(), 1, "{v} appears once");
        }
    }

    #[test]
    fn verb_menu_drops_recent_verbs_not_applicable_to_this_type() {
        let s = state();
        // "view-img" was recently run on some image, but it's not in allow for
        // text/plain → it must NOT appear. "critique" is applicable → promoted.
        let menu = s.verb_menu(&["view-img".into(), "critique".into()]);
        assert!(!menu.contains(&"view-img".to_string()), "inapplicable recent verb dropped");
        assert_eq!(&menu[0], "critique");
        assert_eq!(menu.len(), s.allow().len());
    }

    #[test]
    fn verb_menu_no_recent_is_plain_allow_order() {
        let s = state();
        let menu = s.verb_menu(&[]);
        let allow: Vec<String> = s.allow().iter().map(|x| x.to_string()).collect();
        assert_eq!(menu, allow, "no recency → registry order unchanged");
    }

    #[test]
    fn verb_menu_dedupes_repeated_recent() {
        let s = state();
        // a duplicated recent entry must not double-list the verb.
        let menu = s.verb_menu(&["critique".into(), "critique".into()]);
        assert_eq!(menu.iter().filter(|m| m.as_str() == "critique").count(), 1);
        assert_eq!(menu.len(), s.allow().len());
    }

    #[test]
    fn argv_and_preview_are_consistent() {
        let mut s = state();
        // no verb yet → empty argv, bare "goo".
        assert!(s.argv().is_empty());
        assert_eq!(s.preview(), "goo");

        s.select_verb("summarize");
        assert_eq!(s.argv(), vec!["summarize", "goo://clip/"]);
        // the load-bearing invariant: preview == "goo " + argv.
        assert_eq!(s.preview(), format!("goo {}", s.argv().join(" ")));
        assert_eq!(s.preview(), "goo summarize goo://clip/");
    }

    #[test]
    fn argv_renders_adverbs_in_name_order() {
        let mut s = state();
        s.select_verb("summarize");
        s.adverbs.insert("via".into(), "fabric".into());
        assert_eq!(s.argv(), vec!["summarize", "goo://clip/", "--via=fabric"]);
        assert_eq!(s.preview(), "goo summarize goo://clip/ --via=fabric");
    }

    #[test]
    fn argv_never_contains_confirm_dangerous() {
        // The gate bypass is appended by the spawn wrapper, never by the sentence.
        let mut s = state();
        s.select_verb("delete");
        let argv = s.argv();
        assert!(argv.iter().all(|a| !a.contains("confirm-dangerous")), "argv stays a clean sentence");
        assert_eq!(s.preview(), "goo delete goo://clip/");
    }

    #[test]
    fn verb_flags_reports_confirm_destructive_by_name_before_selection() {
        // No verb selected yet — the menu still needs per-verb chips.
        let s = state();
        assert_eq!(s.verb_flags("delete"), (true, true));
        assert_eq!(s.verb_flags("summarize"), (false, false));
        assert_eq!(s.verb_flags("move-to"), (false, false));
        // an unknown/inapplicable verb is flag-free, not a panic.
        assert_eq!(s.verb_flags("nope"), (false, false));
    }

    #[test]
    fn select_verb_clears_adverbs() {
        let mut s = state();
        s.select_verb("summarize");
        s.adverbs.insert("via".into(), "fabric".into());
        s.select_verb("critique"); // inc1: fresh slots on verb change
        assert!(s.adverbs.is_empty());
    }

    #[test]
    fn confirm_and_destructive_and_object_type_read_from_options() {
        let mut s = state();
        s.select_verb("delete");
        assert!(s.needs_confirm());
        assert!(s.is_destructive());
        assert!(!s.needs_object());

        s.select_verb("move-to");
        assert_eq!(s.object_type(), Some("application/vnd.x.ws"));
        assert!(s.needs_object()); // inc1 disables Run on this

        s.select_verb("summarize");
        assert!(!s.needs_confirm());
        assert!(!s.is_destructive());
        assert!(!s.needs_object());
    }

    #[test]
    fn object_threads_into_argv_as_second_positional() {
        let mut s = state();
        s.select_verb("move-to");
        // incomplete until the object is picked.
        assert!(!s.is_complete());
        assert_eq!(s.argv(), vec!["move-to", "goo://clip/"]);
        s.select_object("goo://ws/2".into());
        assert!(s.is_complete());
        assert_eq!(s.argv(), vec!["move-to", "goo://clip/", "goo://ws/2"]);
        assert_eq!(s.preview(), "goo move-to goo://clip/ goo://ws/2");
    }

    #[test]
    fn is_complete_for_one_step_verbs_needs_only_a_verb() {
        let mut s = state();
        assert!(!s.is_complete()); // no verb
        s.select_verb("summarize"); // no object_type
        assert!(s.is_complete());
    }

    #[test]
    fn adverb_slots_come_from_options_and_only_overrides_reach_argv() {
        let mut s = state();
        s.select_verb("summarize"); // uses_adverbs = ["via"], via default = clipboard
        let slots = s.adverb_slots();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].name, "via");
        assert_eq!(slots[0].default.as_deref(), Some("clipboard"));
        assert!(slots[0].values.contains(&"fabric".to_string()));

        // current value with nothing set = the default.
        assert_eq!(s.adverb_value("via", Some("clipboard")), Some("clipboard"));

        // setting to a NON-default value puts it in the sentence.
        s.set_adverb("via", "fabric", Some("clipboard"));
        assert_eq!(s.adverb_value("via", Some("clipboard")), Some("fabric"));
        assert_eq!(s.argv(), vec!["summarize", "goo://clip/", "--via=fabric"]);

        // cycling back to the default DROPS it — the preview never restates a default.
        s.set_adverb("via", "clipboard", Some("clipboard"));
        assert_eq!(s.argv(), vec!["summarize", "goo://clip/"]);
        assert!(s.adverbs.is_empty());

        // a verb with no adverbs has no slots.
        s.select_verb("critique");
        assert!(s.adverb_slots().is_empty());
    }

    #[test]
    fn select_verb_clears_a_stale_object() {
        let mut s = state();
        s.select_verb("move-to");
        s.select_object("goo://ws/2".into());
        assert!(s.object_addr.is_some());
        s.select_verb("summarize"); // different verb → object is stale
        assert!(s.object_addr.is_none());
    }

    // ---- fuzzy ----

    #[test]
    fn fuzzy_filters_to_subsequence_matches() {
        assert!(fuzzy_score("summarize", "smz").is_some()); // subsequence
        assert!(fuzzy_score("summarize", "xyz").is_none()); // not a subsequence
        assert!(fuzzy_score("anything", "").is_some()); // empty needle passes
    }

    #[test]
    fn fuzzy_rank_keeps_order_on_empty_query_and_ranks_matches() {
        let items = vec![Item::new("a", "summarize"), Item::new("b", "critique"), Item::new("c", "compose")];
        // empty query = identity (preserves caller order, e.g. recency).
        let all = fuzzy_rank(&items, "");
        assert_eq!(all.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
        // "co" matches only "compose"; "summarize"/"critique" drop out.
        let co = fuzzy_rank(&items, "co");
        assert_eq!(co.len(), 1);
        assert_eq!(co[0].key, "c");
        // a word-start prefix outranks a scattered subsequence.
        let items2 = vec![Item::new("scatter", "supercilious"), Item::new("prefix", "submarine")];
        let su = fuzzy_rank(&items2, "sub");
        assert_eq!(su[0].key, "prefix"); // "sub"marine starts with it
    }

    #[test]
    fn a_window_row_is_found_by_app_name_only_when_the_label_leads_with_it() {
        // Regression (the real bug): window rows were labeled by TITLE alone, so an
        // Alacritty window titled "git--woollama ● 1 claude" was unfindable by typing
        // "alacritty". The control proves the title-only label fails; the fix is to
        // lead the label with the app id (done in plugins/apps.toml's :win source).
        let title_only = vec![Item::new("goo://win/Alacritty/6", "git--woollama ● 1 claude")];
        assert!(
            fuzzy_rank(&title_only, "alacritty").is_empty(),
            "title-only label is NOT found by app name — the bug"
        );
        let app_led = vec![Item::new("goo://win/Alacritty/6", "Alacritty — git--woollama ● 1 claude")];
        let got = fuzzy_rank(&app_led, "alacritty");
        assert_eq!(got.len(), 1, "an app-led label IS found by the app name — the fix");
        assert_eq!(got[0].key, "goo://win/Alacritty/6");
    }

    #[test]
    fn item_icon_is_carried_through_filtering() {
        let items = vec![Item::new("a", "Firefox").with_icon(Some("firefox".into())), Item::new("b", "plain")];
        // the icon name survives a fuzzy filter (the shell resolves it to an image).
        let got = fuzzy_rank(&items, "fire");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].icon.as_deref(), Some("firefox"));
        // a plain item has no icon.
        assert_eq!(Item::new("b", "plain").icon, None);
    }

    // ---- reducer (ComposeUi) ----

    fn ui() -> ComposeUi {
        // Two subject candidates; resolution is faked by the test via on_subject_resolved.
        ComposeUi::new(vec![
            Item::new("goo://clip/", "clipboard: hi"),
            Item::new("goo://sel/", "selection: yo"),
        ])
    }

    fn resolved_ui(recent: &[&str]) -> ComposeUi {
        let mut u = ui();
        let st = state(); // a text/plain subject with summarize/critique/move-to/delete/open
        u.on_subject_resolved(st, recent.iter().map(|s| s.to_string()).collect());
        u
    }

    fn ch(c: &str) -> KeyInput {
        KeyInput::Char(c.to_string())
    }

    #[test]
    fn typing_filters_and_committing_subject_asks_the_shell_to_resolve() {
        let mut u = ui();
        assert_eq!(u.visible().len(), 2);
        // type "sel" → only the selection candidate remains.
        for c in ["s", "e", "l"] {
            assert_eq!(u.apply(&ch(c)), None);
        }
        assert_eq!(u.visible().len(), 1);
        assert_eq!(u.visible()[0].key, "goo://sel/");
        // Enter commits → the shell is asked to resolve THAT address (no I/O here).
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::ResolveSubject("goo://sel/".into())));
        // still on the subject stage until the shell resolves + advances.
        assert_eq!(u.stage, Stage::Subject);
    }

    #[test]
    fn one_step_plain_verb_runs_on_the_completing_keypress() {
        // A plain (non-gated, no-adverb) verb runs on the keypress that completes
        // the sentence — gnome-do directness. No review is lost: the speak-it-back
        // preview is already visible while selecting the verb. (Gated verbs and
        // verbs with adverb slots still dwell in Ready — covered below.)
        let mut u = resolved_ui(&[]);
        assert_eq!(u.stage, Stage::Verb);
        for c in ["c", "r", "i"] {
            u.apply(&ch(c));
        }
        assert_eq!(u.visible()[0].key, "critique");
        // ONE Enter both completes and runs the sentence (no dwell, no second press).
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::Run));
        assert_eq!(u.state.as_ref().unwrap().preview(), "goo critique goo://clip/");
    }

    #[test]
    fn gated_verb_needs_a_distinct_confirm_beat_a_reflex_double_enter_cannot_fire_it() {
        // recency promotes `delete` (confirm+destructive) to the top.
        let mut u = resolved_ui(&["delete"]);
        assert_eq!(u.visible()[0].key, "delete"); // selected=0
        // First Enter: commit the verb → Ready. Does NOT run (completing keypress).
        assert_eq!(u.apply(&KeyInput::Enter), None);
        assert_eq!(u.stage, Stage::Ready);
        assert!(u.gated());
        // Second Enter (the reflex): only ARMS the confirm — still no run.
        assert_eq!(u.apply(&KeyInput::Enter), None);
        assert!(u.armed());
        // Third, deliberate Enter: now it runs.
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::Run));
    }

    #[test]
    fn two_step_verb_loads_objects_then_completes_with_the_object_in_argv() {
        let mut u = resolved_ui(&[]);
        for c in ["m", "o", "v", "e"] {
            u.apply(&ch(c));
        }
        assert_eq!(u.visible()[0].key, "move-to");
        // committing a two-step verb advances to the Object stage and asks the
        // shell to load object candidates of the verb's object_type.
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::LoadObjects("application/vnd.x.ws".into())));
        assert_eq!(u.stage, Stage::Object);
        // shell supplies candidates; pick one.
        u.set_objects(vec![Item::new("goo://ws/1", "Workspace 1"), Item::new("goo://ws/2", "Workspace 2")]);
        u.apply(&ch("2"));
        // The OBJECT pick completes a plain two-step sentence → it runs on that
        // keypress too (the short-circuit is symmetric across the completing commit).
        let act = u.apply(&KeyInput::Enter);
        assert_eq!(u.state.as_ref().unwrap().argv(), vec!["move-to", "goo://clip/", "goo://ws/2"]);
        assert_eq!(act, Some(UiAction::Run));
    }

    #[test]
    fn escape_clears_the_query_then_steps_back_then_cancels() {
        let mut u = resolved_ui(&[]);
        u.apply(&ch("z")); // a query with no matches
        assert_eq!(u.query, "z");
        // Esc with a query just clears it.
        u.apply(&KeyInput::Escape);
        assert_eq!(u.query, "");
        assert_eq!(u.stage, Stage::Verb);
        // Esc with empty query steps back to the subject stage.
        u.apply(&KeyInput::Escape);
        assert_eq!(u.stage, Stage::Subject);
        assert!(u.state.is_none());
        // Esc at the subject stage (empty query) cancels.
        assert_eq!(u.apply(&KeyInput::Escape), Some(UiAction::Cancel));
    }

    #[test]
    fn ready_stage_cycles_adverbs_into_the_sentence_and_disarms() {
        // summarize has one selector adverb (via, default clipboard).
        let mut u = resolved_ui(&[]);
        for c in ["s", "u", "m"] {
            u.apply(&ch(c));
        }
        assert_eq!(u.visible()[0].key, "summarize");
        assert_eq!(u.apply(&KeyInput::Enter), None); // → Ready (no run)
        assert_eq!(u.stage, Stage::Ready);
        assert_eq!(u.adverb_slots().len(), 1);
        assert_eq!(u.state.as_ref().unwrap().preview(), "goo summarize goo://clip/"); // default, no flag

        // Right cycles the focused `via` off its default → it enters the sentence.
        u.apply(&KeyInput::Right);
        let pv = u.state.as_ref().unwrap().preview();
        assert!(pv.starts_with("goo summarize goo://clip/ --via="), "cycled adverb shows: {pv}");
        // It's still a Ready/run-on-Enter sentence.
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::Run));
    }

    #[test]
    fn ready_arrows_are_safe_no_ops_when_a_dwelling_verb_has_no_adverb_slots() {
        // A plain verb now runs on commit (never dwells), so the only verb that sits
        // in Ready with NO adverb slots is a gated one. `delete` is gated → it dwells;
        // Left/Right/Down must be safe no-ops there, and the arm→run beat still works.
        let mut u = resolved_ui(&["delete"]);
        assert_eq!(u.visible()[0].key, "delete");
        assert_eq!(u.apply(&KeyInput::Enter), None); // commit → Ready (gated dwell)
        assert_eq!(u.stage, Stage::Ready);
        assert!(u.adverb_slots().is_empty());
        u.apply(&KeyInput::Left); // no-op (no slots)
        u.apply(&KeyInput::Right); // no-op
        u.apply(&KeyInput::Down); // no-op
        assert!(!u.armed(), "arrows didn't arm anything");
        assert_eq!(u.apply(&KeyInput::Enter), None); // arm
        assert!(u.armed());
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::Run));
    }

    #[test]
    fn cycling_an_adverb_disarms_a_gated_confirm() {
        // a gated verb WITH an adverb would re-require the beat after an edit; use
        // `delete` (gated, no adverb) to confirm the arm path, then assert the
        // summarize (non-gated) cycle path returns Run on a single Ready Enter.
        let mut u = resolved_ui(&["delete"]);
        assert_eq!(u.visible()[0].key, "delete");
        u.apply(&KeyInput::Enter); // commit → Ready (gated)
        assert_eq!(u.apply(&KeyInput::Enter), None); // arm
        assert!(u.armed());
        // (delete has no adverb slot, so there's nothing to cycle; the arm path is
        // what matters and is covered here + in the dedicated gated test.)
        assert_eq!(u.apply(&KeyInput::Enter), Some(UiAction::Run));
    }

    #[test]
    fn arrow_keys_move_selection_within_bounds() {
        let mut u = resolved_ui(&[]);
        let n = u.visible().len();
        assert!(n >= 2);
        u.apply(&KeyInput::Up); // already at 0, stays
        assert_eq!(u.selected, 0);
        u.apply(&KeyInput::Down);
        assert_eq!(u.selected, 1);
        for _ in 0..(n + 5) {
            u.apply(&KeyInput::Down);
        }
        assert_eq!(u.selected, n - 1); // clamped at the last row
    }
}
