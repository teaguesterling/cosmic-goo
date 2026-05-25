//! Content-dispatch rule table — the Rust port of `lib/dispatch.sh`.
//!
//! A `[[dispatch]]` rule classifies raw text by a `matches` regex and routes it
//! to a verb, optionally rewriting the subject via the regex captures. Rules are
//! tried in load order; the first whose pattern matches wins; matching is
//! single-shot (the rewritten subject is not re-fed through the table — that's
//! the caller's contract). `${N}` interpolates capture N (0 = whole match) into
//! any `set`/`adverbs` value (nested too).
//!
//! Regex: the bash engine uses bash ERE with POSIX classes (`[[:space:]]`,
//! `[[:digit:]]`). The `regex` crate supports POSIX classes, so the fixture
//! patterns port verbatim (verified against the bash `=~` oracle).

use regex::Regex;
use serde_json::{json, Value};
use std::sync::OnceLock;

/// The descriptor a matched rule renders into: `{ verb, type, adverbs, fields }`
/// — the same shape `_dispatch_render` builds. `fields` is the rule's `set` map
/// with captures interpolated; the caller turns it into the subject. Returns
/// `None` if no rule matches.
pub fn dispatch_match(reg: &Value, text: &str) -> Option<Value> {
    let rules = reg.get("dispatch")?.as_array()?;
    for rule in rules {
        let pattern = match rule.get("matches").and_then(|m| m.as_str()) {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };
        let re = match Regex::new(pattern) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Some(caps) = re.captures(text) {
            let caps_vec: Vec<String> = (0..caps.len())
                .map(|i| caps.get(i).map(|m| m.as_str().to_string()).unwrap_or_default())
                .collect();
            return Some(render_rule(rule, &caps_vec));
        }
    }
    None
}

fn render_rule(rule: &Value, caps: &[String]) -> Value {
    json!({
        "verb": rule.get("verb").cloned().unwrap_or(Value::Null),
        "type": rule.get("type").cloned().unwrap_or(Value::Null),
        "adverbs": deep_interp(rule.get("adverbs").cloned().unwrap_or_else(|| json!({})), caps),
        "fields": deep_interp(rule.get("set").cloned().unwrap_or_else(|| json!({})), caps),
    })
}

/// Recursively interpolate `${N}` in every string value of `v`.
fn deep_interp(v: Value, caps: &[String]) -> Value {
    match v {
        Value::String(s) => Value::String(interp(&s, caps)),
        Value::Array(a) => Value::Array(a.into_iter().map(|x| deep_interp(x, caps)).collect()),
        Value::Object(m) => {
            Value::Object(m.into_iter().map(|(k, x)| (k, deep_interp(x, caps))).collect())
        }
        other => other,
    }
}

/// Replace each `${N}` with capture N (out-of-range → empty), mirroring the jq
/// `gsub("\\$\\{(?<n>[0-9]+)\\}"; ($caps[(.n|tonumber)] // ""))`.
fn interp(s: &str, caps: &[String]) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\$\{([0-9]+)\}").unwrap());
    re.replace_all(s, |c: &regex::Captures| {
        let n: usize = c[1].parse().unwrap_or(usize::MAX);
        caps.get(n).cloned().unwrap_or_default()
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::dispatch_match;
    use crate::registry;
    use serde_json::{json, Value};

    // The cli.bats `test-dispatch.toml` fixture.
    fn fixture() -> Value {
        registry::from_fixture_toml(
            "test-dispatch",
            r#"
name = "test-dispatch"

[[dispatch]]
matches = 'RFC:?[[:space:]]*([0-9]+)'
type = "text/plain"
set = { text = "rfc-${1}" }
verb = "echo-back"

[[dispatch]]
matches = '([a-z]+)=([0-9]+)'
type = "text/plain"
set = { text = "${1}:${2}" }
verb = "echo-back"

[[dispatch]]
matches = 'hello'
verb = "echo-back"

[[dispatch]]
matches = '^wrapme:(.*)$'
set = { text = "${1}" }
adverbs = { via = "dump" }
verb = "wrap"

[[dispatch]]
matches = '^route ([a-z]+):(.*)$'
set = { text = "${2}" }
adverbs = { via = "${1}" }
verb = "wrap"

[[dispatch]]
matches = '^recurse$'
set = { text = "RFC 999" }
verb = "echo-back"

[[dispatch]]
matches = 'ZZZ'
set = { text = "first" }
verb = "echo-back"

[[dispatch]]
matches = 'ZZZ'
set = { text = "second" }
verb = "echo-back"
"#,
        )
    }

    #[test]
    fn capture_rewrites_subject_text() {
        let d = dispatch_match(&fixture(), "RFC 2616").unwrap();
        assert_eq!(d["verb"], json!("echo-back"));
        assert_eq!(d["type"], json!("text/plain"));
        assert_eq!(d["fields"]["text"], json!("rfc-2616"));
    }

    #[test]
    fn matches_substring_within_input() {
        let d = dispatch_match(&fixture(), "please read RFC 822 today").unwrap();
        assert_eq!(d["fields"]["text"], json!("rfc-822"));
    }

    #[test]
    fn interpolates_multiple_captures() {
        let d = dispatch_match(&fixture(), "port=8080").unwrap();
        assert_eq!(d["fields"]["text"], json!("port:8080"));
    }

    #[test]
    fn no_set_means_no_fields_to_rewrite() {
        let d = dispatch_match(&fixture(), "say hello there").unwrap();
        assert_eq!(d["verb"], json!("echo-back"));
        assert_eq!(d["fields"], json!({})); // no `set` → empty fields
        assert_eq!(d["type"], json!(null)); // no `type`
    }

    #[test]
    fn adverbs_reach_the_descriptor() {
        let d = dispatch_match(&fixture(), "wrapme:payload").unwrap();
        assert_eq!(d["verb"], json!("wrap"));
        assert_eq!(d["adverbs"]["via"], json!("dump"));
        assert_eq!(d["fields"]["text"], json!("payload"));
    }

    #[test]
    fn captures_interpolate_into_adverb_values() {
        let d = dispatch_match(&fixture(), "route dump:via-capture").unwrap();
        assert_eq!(d["adverbs"]["via"], json!("dump"));
        assert_eq!(d["fields"]["text"], json!("via-capture"));
    }

    #[test]
    fn single_shot_does_not_recurse() {
        // The matched rule rewrites to "RFC 999"; dispatch_match returns that as
        // the fields — it does NOT re-run the table.
        let d = dispatch_match(&fixture(), "recurse").unwrap();
        assert_eq!(d["fields"]["text"], json!("RFC 999"));
    }

    #[test]
    fn first_matching_rule_wins() {
        let d = dispatch_match(&fixture(), "ZZZ").unwrap();
        assert_eq!(d["fields"]["text"], json!("first"));
    }

    #[test]
    fn no_rule_matches_is_none() {
        assert!(dispatch_match(&fixture(), "plain words").is_none());
    }
}
