//! Adverb resolution — the Rust port of `_resolve_adverbs` in `lib/verbs.sh`.
//!
//! For each adverb a verb `uses_adverbs`, compute its effective value (user
//! override → declared `default`), collect any `template_var` a selector adverb
//! injects for the chosen value, and let the *first* selector adverb that
//! supplies a command `template` for its chosen value dictate the route template
//! (e.g. `via=clipboard`). Returns the same `{selected, template_vars,
//! route_template}` shape the shell builds, as a `serde_json::Value`, so the
//! render step can spread it into the substitution context unchanged.

use serde_json::{json, Map, Value};

/// Resolve the adverbs `verb` uses against the registry and the user's choices.
///
/// `user_choices` is an object like `{"via": "clipboard"}` (may be empty/`{}`).
/// The result is `{ "selected": {name: value, …}, "template_vars": {…},
/// "route_template": "<cmd template>" | null }`.
pub fn resolve(reg: &Value, verb: &Value, user_choices: &Value) -> Value {
    let uses = verb
        .get("uses_adverbs")
        .and_then(|u| u.as_array())
        .cloned()
        .unwrap_or_default();
    let registry_adverbs = reg.get("adverbs").and_then(|a| a.as_array());

    let mut selected = Map::new();
    let mut template_vars = Map::new();
    let mut route_template: Option<String> = None;

    for aname_v in &uses {
        let aname = match aname_v.as_str() {
            Some(s) => s,
            None => continue,
        };
        // Find the adverb's definition. Missing → leave the accumulator
        // untouched (jq's `select` would yield nothing for this step).
        let adverb = match registry_adverbs
            .and_then(|arr| arr.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(aname)))
        {
            Some(a) => a,
            None => continue,
        };

        // Effective value: user override → declared default.
        let val = user_choices
            .get(aname)
            .cloned()
            .or_else(|| adverb.get("default").cloned())
            .unwrap_or(Value::Null);
        selected.insert(aname.to_string(), val.clone());

        // `values[<val>]` — values are keyed by the (string) chosen value.
        let chosen = val
            .as_str()
            .and_then(|vk| adverb.get("values").and_then(|vs| vs.get(vk)));

        // Selector adverbs may inject template_vars for the chosen value.
        let kind = adverb.get("kind").and_then(|k| k.as_str()).unwrap_or("selector");
        if kind == "selector" {
            if let Some(tv) = chosen.and_then(|c| c.get("template_var")).and_then(|t| t.as_object()) {
                for (k, v) in tv {
                    template_vars.insert(k.clone(), v.clone());
                }
            }
        }

        // First chosen value that supplies a command template wins the route.
        if route_template.is_none() {
            if let Some(t) = chosen.and_then(|c| c.get("template")).and_then(|t| t.as_str()) {
                route_template = Some(t.to_string());
            }
        }
    }

    json!({
        "selected": Value::Object(selected),
        "template_vars": Value::Object(template_vars),
        "route_template": route_template,
    })
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use crate::registry;
    use serde_json::json;

    // The verbs.bats fixture's adverb shapes: a `via` selector (clipboard
    // default, fabric/clipboard route templates) and a `depth` selector
    // (normal default, template_var injection).
    fn fixture() -> serde_json::Value {
        registry::from_fixture_toml(
            "fixture",
            r#"
name = "fixture"

[[verbs]]
name = "critique"
accepts = ["text/*"]
uses_adverbs = ["via"]
fabric_pattern = "analyze_claims"
prompt = "Review:\n{subject.text}"

[[verbs]]
name = "think"
accepts = ["text/*"]
uses_adverbs = ["via", "depth"]
prompt = "{depth_prefix}:\n{subject.text}"

[[adverbs]]
name = "via"
kind = "selector"
default = "clipboard"

[adverbs.values.fabric]
template = "cat <<< '{verb.prompt}' | fabric -p {verb.fabric_pattern}"

[adverbs.values.clipboard]
template = "cat <<< '{verb.prompt}'"

[[adverbs]]
name = "depth"
kind = "selector"
default = "normal"

[adverbs.values.normal]
template_var = { depth_prefix = "Think about" }

[adverbs.values.ultra]
template_var = { depth_prefix = "Ultrathink about" }
"#,
        )
    }

    fn verb<'a>(reg: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        reg["verbs"].as_array().unwrap().iter().find(|v| v["name"] == json!(name)).unwrap()
    }

    #[test]
    fn user_choice_overrides_default_and_picks_route() {
        let reg = fixture();
        let r = resolve(&reg, verb(&reg, "critique"), &json!({"via": "clipboard"}));
        assert_eq!(r["selected"]["via"], json!("clipboard"));
        assert_eq!(r["route_template"], json!("cat <<< '{verb.prompt}'"));
    }

    #[test]
    fn falls_back_to_default_when_unspecified() {
        let reg = fixture();
        // No adverbs given → via defaults to clipboard.
        let r = resolve(&reg, verb(&reg, "critique"), &json!({}));
        assert_eq!(r["selected"]["via"], json!("clipboard"));
        assert_eq!(r["route_template"], json!("cat <<< '{verb.prompt}'"));
    }

    #[test]
    fn fabric_choice_selects_other_route() {
        let reg = fixture();
        let r = resolve(&reg, verb(&reg, "critique"), &json!({"via": "fabric"}));
        assert_eq!(r["selected"]["via"], json!("fabric"));
        assert_eq!(
            r["route_template"],
            json!("cat <<< '{verb.prompt}' | fabric -p {verb.fabric_pattern}")
        );
    }

    #[test]
    fn selector_injects_template_var_for_chosen_value() {
        let reg = fixture();
        let r = resolve(&reg, verb(&reg, "think"), &json!({"via": "clipboard", "depth": "ultra"}));
        assert_eq!(r["template_vars"]["depth_prefix"], json!("Ultrathink about"));
        // via=clipboard still dictates the route (depth has no template).
        assert_eq!(r["route_template"], json!("cat <<< '{verb.prompt}'"));
    }

    #[test]
    fn default_depth_injects_normal_prefix() {
        let reg = fixture();
        let r = resolve(&reg, verb(&reg, "think"), &json!({"via": "clipboard"}));
        assert_eq!(r["template_vars"]["depth_prefix"], json!("Think about"));
    }

    #[test]
    fn no_adverbs_used_is_empty() {
        let reg = fixture();
        let no_adverb_verb = json!({"name": "x", "cmd": "echo hi"});
        let r = resolve(&reg, &no_adverb_verb, &json!({}));
        assert_eq!(r["selected"], json!({}));
        assert_eq!(r["template_vars"], json!({}));
        assert_eq!(r["route_template"], json!(null));
    }
}
