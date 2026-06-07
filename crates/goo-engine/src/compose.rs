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
    /// Filled selector/fill adverb slots (inc 2 populates these; inc 1 leaves
    /// it empty — verbs run on their registry defaults).
    pub adverbs: BTreeMap<String, String>,
}

impl ComposeState {
    /// Build from an already-resolved `subject` (the shell did `address::resolve`).
    /// `subject_addr` is the canonical address to thread into `argv`. Calls
    /// `options_for` once and freezes it.
    pub fn from_subject(reg: &Value, subject: &Value, subject_addr: String) -> ComposeState {
        let options = options::options_for(reg, subject);
        let subject_type = options.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        ComposeState { subject_addr, subject_type, options, verb: None, adverbs: BTreeMap::new() }
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
    /// adverbs — inc 1 starts the slots fresh on each verb change; the §6.6
    /// keep-compatible-adverbs behaviour is a later increment (roadmap #12).
    pub fn select_verb(&mut self, name: &str) {
        self.verb = Some(name.to_string());
        self.adverbs.clear();
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

    /// The sentence as a `goo` argv: `[verb, subject_addr, --k=v…]`, adverbs in
    /// name order (BTreeMap). The SSOT for both [`preview`](Self::preview) and
    /// execution. Empty when no verb is picked yet.
    ///
    /// **`--confirm-dangerous` is deliberately excluded** — it is a gate bypass,
    /// not a sentence slot, so it never appears in the preview. The spawn wrapper
    /// appends it *after* the confirm pane is accepted (mirroring how
    /// `recordable_adverbs` excludes it from the replay history).
    pub fn argv(&self) -> Vec<String> {
        let Some(verb) = self.verb.as_deref() else { return Vec::new() };
        let mut argv = vec![verb.to_string(), self.subject_addr.clone()];
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
}
