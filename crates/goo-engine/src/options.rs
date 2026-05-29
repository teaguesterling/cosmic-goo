//! The OPTIONS surface (goo-protocol §7) — a pure projection of the registry into
//! one composable discovery shape: the verbs applicable to a subject and, per
//! verb, the slots a caller can fill. The compose-gui's verb-pick, completion, and
//! (later) the `good` daemon all consume this *one* function, so the daemon becomes
//! a thin transport over a proven surface rather than new semantics.
//!
//! **The JSON shape is UNSTABLE through v1** — `schema_version` + `stable:false`
//! let consumers gate on it while it settles.
//!
//! Scope (v1): `allow` + per-verb `using` / `with` / `object_type`. The `with`
//! slots mirror the *run-path* adverb gate (`uses_adverbs`, per [`crate::adverbs`]),
//! not the `applies_to` offer-scope, so OPTIONS never promises a slot that wouldn't
//! actually take effect. `to:` (write-destination choices) is deferred to v2 with
//! the declared `{write}`-domain framework — file/clip are reachable today via
//! `--to`/`-o` regardless of OPTIONS.

use crate::verbs;
use serde_json::{json, Map, Value};

/// Schema version of the OPTIONS JSON — bumped on any shape change so consumers
/// (compose-gui, …) can gate. Paired with `stable:false` until the shape settles.
const SCHEMA_VERSION: &str = "0.1";

/// The OPTIONS view for `subject`: `allow` (applicable verbs in `for_subject`
/// order), the type's `default` verb, and a per-verb slot map. A pure projection —
/// it never leaks the verb's `cmd`/`prompt`/internals (see [`verb_options`]).
pub fn options_for(reg: &Value, subject: &Value) -> Value {
    let stype = subject.get("type").and_then(Value::as_str).unwrap_or("");
    let applicable = verbs::for_subject(reg, subject);

    let allow: Vec<Value> = applicable
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str).map(|n| json!(n)))
        .collect();

    let default = verbs::default_for(reg, stype)
        .and_then(|v| v.get("name").and_then(Value::as_str).map(String::from));

    let mut verbs_map = Map::new();
    for v in &applicable {
        if let Some(name) = v.get("name").and_then(Value::as_str) {
            verbs_map.insert(name.to_string(), verb_options(reg, v));
        }
    }

    json!({
        "schema_version": SCHEMA_VERSION,
        "stable": false,
        "type": stype,
        "default": default,
        "allow": allow,
        "verbs": Value::Object(verbs_map),
    })
}

/// Project ONE verb to its OPTIONS slots. This is an **explicit** projection, never
/// a pass-through of the verb's TOML — `cmd`, `prompt`, `description`, and every
/// other internal field stay out of the discovery surface.
fn verb_options(reg: &Value, verb: &Value) -> Value {
    // `Using:` — the verb's `usage` channels (the instruments the planner chooses
    // among / `--using` pins). Empty for a plain or `present` verb.
    let using: Vec<Value> = verb
        .get("usage")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(|s| json!(s)).collect())
        .unwrap_or_default();

    // `With:` — the adverbs the verb actually resolves at run time, i.e. its
    // `uses_adverbs` (the gate `adverbs::resolve` uses), NOT `applies_to` (which is
    // offer/completion scope). Mirroring the run path keeps OPTIONS honest.
    let mut with = Map::new();
    if let Some(uses) = verb.get("uses_adverbs").and_then(Value::as_array) {
        for name in uses.iter().filter_map(Value::as_str) {
            if let Some(adverb) = find_adverb(reg, name) {
                with.insert(name.to_string(), adverb_schema(adverb));
            }
        }
    }

    let object_type = verb
        .get("object_type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);

    json!({
        "using": using,
        "with": Value::Object(with),
        "object_type": object_type,
    })
}

fn find_adverb<'a>(reg: &'a Value, name: &str) -> Option<&'a Value> {
    reg.get("adverbs")
        .and_then(Value::as_array)?
        .iter()
        .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
}

/// Project an adverb to its OPTIONS schema: `kind`, `default`, and the choice
/// `values` (the keys of its `values` table — empty for a `fill` adverb).
fn adverb_schema(adverb: &Value) -> Value {
    let kind = adverb.get("kind").and_then(Value::as_str).unwrap_or("fill");
    let default = adverb.get("default").and_then(Value::as_str).map(String::from);
    let values: Vec<Value> = adverb
        .get("values")
        .and_then(Value::as_object)
        .map(|m| m.keys().map(|k| json!(k)).collect())
        .unwrap_or_default();
    json!({ "kind": kind, "default": default, "values": values })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn reg() -> Value {
        j!({
            "verbs": [
                { "name": "summarize", "accepts": ["text/*"], "uses_adverbs": ["via", "depth"],
                  "prompt": "SECRET PROMPT", "cmd": "secret-cmd" },
                { "name": "move-to", "accepts": ["text/*"], "object_type": "application/vnd.x.ws",
                  "cmd": "mv {subject.id} {object.id}" },
                { "name": "say", "accepts": ["text/*"], "usage": ["loud", "quiet"] },
                { "name": "view-img", "accepts": ["image/*"], "cmd": "x" },
                { "name": "open", "accepts": ["text/*"], "default_for": "text/plain", "cmd": "xdg-open" }
            ],
            "adverbs": [
                { "name": "via", "kind": "selector", "applies_to": ["text/*"], "default": "clipboard",
                  "values": { "fabric": {}, "clipboard": {} } },
                { "name": "depth", "kind": "selector", "applies_to_verbs": ["summarize"], "default": "normal",
                  "values": { "normal": {}, "ultra": {} } }
            ],
            "channels": [
                { "name": "loud", "accepts": ["text/*"], "emits": "text/x-said", "cmd": "x" },
                { "name": "quiet", "accepts": ["text/*"], "emits": "text/x-said", "cmd": "y" }
            ]
        })
    }

    fn subj(t: &str) -> Value {
        j!({ "type": t, "text": "hi" })
    }

    #[test]
    fn allow_lists_applicable_verbs_and_default() {
        let o = options_for(&reg(), &subj("text/plain"));
        let allow: Vec<&str> = o["allow"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(allow.contains(&"summarize"));
        assert!(allow.contains(&"say"));
        assert!(allow.contains(&"open"));
        assert!(!allow.contains(&"view-img")); // image/* verb — not applicable to text/plain
        assert_eq!(o["default"], j!("open")); // default_for text/plain
        assert_eq!(o["type"], j!("text/plain"));
        assert_eq!(o["stable"], j!(false));
        assert_eq!(o["schema_version"], j!("0.1"));
    }

    #[test]
    fn with_mirrors_uses_adverbs_with_values() {
        let o = options_for(&reg(), &subj("text/plain"));
        let with = &o["verbs"]["summarize"]["with"];
        assert_eq!(with["via"]["kind"], j!("selector"));
        assert_eq!(with["via"]["default"], j!("clipboard"));
        let via_vals: Vec<&str> = with["via"]["values"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(via_vals.contains(&"fabric") && via_vals.contains(&"clipboard"));
        assert_eq!(with["depth"]["default"], j!("normal"));
        // a verb that uses no adverbs has an empty `with`.
        assert_eq!(o["verbs"]["say"]["with"], j!({}));
    }

    #[test]
    fn using_lists_instrument_channels() {
        let o = options_for(&reg(), &subj("text/plain"));
        let using: Vec<&str> = o["verbs"]["say"]["using"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(using, vec!["loud", "quiet"]);
        // a plain verb has no instruments.
        assert_eq!(o["verbs"]["open"]["using"], j!([]));
    }

    #[test]
    fn object_type_surfaces_for_two_step_verbs() {
        let o = options_for(&reg(), &subj("text/plain"));
        assert_eq!(o["verbs"]["move-to"]["object_type"], j!("application/vnd.x.ws"));
        assert_eq!(o["verbs"]["summarize"]["object_type"], Value::Null);
    }

    // The projection guarantee: OPTIONS exposes ONLY the documented slots — never
    // the verb's cmd/prompt/description/internal fields. This is the contract the
    // daemon-as-transport will wrap, so it must hold exactly.
    #[test]
    fn projection_never_leaks_internal_verb_fields() {
        let o = options_for(&reg(), &subj("text/plain"));
        let v = &o["verbs"]["summarize"];
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 3, "exactly the documented slots: {keys:?}");
        for leaked in ["cmd", "prompt", "description", "accepts", "name"] {
            assert!(v.get(leaked).is_none(), "OPTIONS leaked verb.{leaked}");
        }
        // and the rendered JSON string contains none of the secret bodies.
        let s = serde_json::to_string(&o).unwrap();
        assert!(!s.contains("SECRET PROMPT") && !s.contains("secret-cmd"));
    }
}
