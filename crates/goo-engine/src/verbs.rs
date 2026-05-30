//! Verb resolution and application — the Rust port of `lib/verbs.sh`.
//!
//! Built up across slice 3. First piece: `valid_when` evaluation.
//!
//! **`valid_when` (and `object_valid_when`) decision:** evaluate the
//! user-authored jq predicate by shelling the same `jq` binary the bash engine
//! uses — guaranteed byte-for-byte parity, and `valid_when` is not hot in the
//! one-shot path (once per verb×subject). `jaq` (no subprocess) is a daemon-era
//! optimization (#31), not needed for the conformance port. This is the spike's
//! conclusion (task #42).

use crate::{adverbs, address, mime, template};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

/// True if `subject` satisfies the jq boolean `predicate`. An empty/absent
/// predicate is always true. Mirrors `verb_valid_for`: `jq -e <expr>` over the
/// subject JSON, where exit 0 = truthy (output neither null nor false).
pub fn satisfies(predicate: &str, subject: &Value) -> bool {
    if predicate.trim().is_empty() {
        return true;
    }
    jq_truthy(predicate, &subject.to_string())
}

/// A verb's `valid_when` predicate evaluated against `subject` (absent → true).
fn valid_for(verb: &Value, subject: &Value) -> bool {
    let expr = verb.get("valid_when").and_then(|v| v.as_str()).unwrap_or("");
    satisfies(expr, subject)
}

/// True if any of `verb.accepts` glob-matches `mime`. A verb with no `accepts`
/// never matches a (non-empty) type — mirrors `jq -r '.accepts[]?'` yielding
/// nothing in the shell.
fn accepts_type(verb: &Value, mime: &str, reg: &Value) -> bool {
    verb.get("accepts")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .any(|pat| mime::is_subtype(mime, pat, reg))
        })
        .unwrap_or(false)
}

/// JSON for the verb named `name`, optionally filtered to those whose `accepts`
/// matches `type_filter`. With multiple verbs of the same name (cross-plugin
/// polymorphism, supported by [`crate::registry::merge`]), the lookup picks the
/// **most-specific** impl for the type — exact > lattice/subtype > glob, ties
/// broken by registry order (later-registered = user-override wins).
/// Without `type_filter`, returns the first verb by name.
pub fn lookup(reg: &Value, name: &str, type_filter: Option<&str>) -> Option<Value> {
    let candidates: Vec<&Value> = reg
        .get("verbs")?
        .as_array()?
        .iter()
        .filter(|v| v.get("name").and_then(|n| n.as_str()) == Some(name))
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let Some(t) = type_filter else {
        return Some(candidates[0].clone());
    };
    // Most-specific match wins; ties take the LATER-registered (>= so iterating
    // forward yields the last-good tie-breaker).
    let mut best: Option<(i32, &Value)> = None;
    for v in &candidates {
        if let Some(s) = verb_specificity(v, t, reg) {
            if best.is_none_or(|(b, _)| s >= b) {
                best = Some((s, v));
            }
        }
    }
    best.map(|(_, v)| v.clone())
}

/// How well `pattern` matches `t`: `None` = no match; higher = more specific.
/// Exact (`pattern == t`) > lattice/subtype match (declared `is_a`, structured
/// suffix) > glob (`text/*`, scored by prefix length so `text/markdown` beats
/// `text/*` and `image/*` beats `*/*`).
fn pattern_specificity(t: &str, pattern: &str, reg: &Value) -> Option<i32> {
    if t == pattern {
        return Some(i32::MAX);
    }
    if !mime::is_subtype(t, pattern, reg) {
        return None;
    }
    // Glob: longer prefix = more specific.
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return Some(prefix.len() as i32);
    }
    if pattern == "*/*" {
        return Some(0);
    }
    // Lattice / structured-suffix match (non-glob, non-exact). Scored above any
    // glob: a verb saying "I want application/json specifically" beats one
    // saying "anything text/*" for a json subject.
    Some(1_000_000)
}

/// A verb's specificity for type `t` = the max across its `accepts` patterns.
fn verb_specificity(verb: &Value, t: &str, reg: &Value) -> Option<i32> {
    let accepts = verb.get("accepts").and_then(Value::as_array)?;
    let mut best: Option<i32> = None;
    for p in accepts {
        if let Some(ps) = p.as_str() {
            if let Some(s) = pattern_specificity(t, ps, reg) {
                if best.is_none_or(|b| s > b) {
                    best = Some(s);
                }
            }
        }
    }
    best
}

/// The verb whose `default_for` matches `type` — a single type string or an
/// array of types (a polymorphic default like `open`). First match wins.
/// Mirrors `verb_default_for`.
pub fn default_for(reg: &Value, type_: &str) -> Option<Value> {
    reg.get("verbs")?
        .as_array()?
        .iter()
        .find(|v| match v.get("default_for") {
            Some(Value::Array(arr)) => arr.iter().any(|d| d.as_str() == Some(type_)),
            Some(Value::String(s)) => s == type_,
            _ => false,
        })
        .cloned()
}

/// Every verb applicable to `subject` — type-accepted *and* passing its
/// `valid_when`. With cross-plugin polymorphism (multiple verbs of the same name
/// with different `accepts`), this returns **one verb per name** — the
/// most-specific impl for the subject type. Order: registry order of the kept
/// verbs, so the picker sees the natural sequence.
pub fn for_subject(reg: &Value, subject: &Value) -> Vec<Value> {
    let stype = match subject.get("type").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    let verbs = match reg.get("verbs").and_then(|v| v.as_array()) {
        Some(v) => v,
        None => return Vec::new(),
    };
    // Walk in registry order; for each name, keep the best-specificity impl seen
    // (>= so later-registered wins ties, matching `lookup`'s tie-break).
    use std::collections::HashMap;
    let mut best: HashMap<String, (i32, usize)> = HashMap::new();
    for (i, v) in verbs.iter().enumerate() {
        if !valid_for(v, subject) {
            continue;
        }
        let Some(name) = v.get("name").and_then(Value::as_str) else { continue };
        if let Some(s) = verb_specificity(v, stype, reg) {
            best.entry(name.to_string())
                .and_modify(|e| {
                    if s >= e.0 {
                        *e = (s, i);
                    }
                })
                .or_insert((s, i));
        }
    }
    // Re-emit in original registry order so consumers see a stable picker list.
    let mut kept: Vec<(usize, Value)> = best.values().map(|(_, i)| (*i, verbs[*i].clone())).collect();
    kept.sort_by_key(|(i, _)| *i);
    kept.into_iter().map(|(_, v)| v).collect()
}

/// A fully-rendered command, ready for `bash -c`. `confirm` mirrors the verb's
/// `confirm` flag — the caller (the `goo` bin) prompts and may decline (130).
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub cmd: String,
    pub confirm: bool,
}

/// Resolve adverbs, build the substitution context, and render the verb's
/// command — the pure (no-exec) core of `verb_apply`. The bin executes
/// `Rendered.cmd` via `bash -c` (honouring `confirm`). Errors mirror the shell's
/// stderr messages.
///
/// `object` is `Value::Null` for one-step verbs; `user_adverbs` is an object
/// (possibly `{}`).
pub fn render(
    reg: &Value,
    verb: &Value,
    subject: &Value,
    object: &Value,
    user_adverbs: &Value,
) -> Result<Rendered, String> {
    // 1. Subject type must match accepts (when the subject is typed).
    let stype = subject.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if !stype.is_empty() && !accepts_type(verb, stype, reg) {
        return Err(format!(
            "subject type '{stype}' does not match verb accepts"
        ));
    }

    // 1b. Honour valid_when against the subject.
    if !valid_for(verb, subject) {
        return Err("subject does not satisfy this verb's valid_when predicate".into());
    }

    // 2. Validate object type if the verb declares object_type.
    let expected_obj = verb.get("object_type").and_then(|t| t.as_str()).unwrap_or("");
    if !expected_obj.is_empty() {
        let got = object.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if got.is_empty() {
            return Err(format!("verb requires object of type '{expected_obj}'"));
        }
        if !mime::is_subtype(got, expected_obj, reg) {
            return Err(format!(
                "object type '{got}' does not match '{expected_obj}'"
            ));
        }
    }

    // 3-5. Build the substitution context (adverbs, template_vars, prompt).
    let (context, resolved) = build_context(reg, verb, subject, object, user_adverbs);

    // 6. Pick the template: adverb route → verb cmd → error.
    let template_str = match resolved["route_template"].as_str().filter(|s| !s.is_empty()) {
        Some(r) => r.to_string(),
        None => match verb.get("cmd").and_then(|c| c.as_str()).filter(|s| !s.is_empty()) {
            Some(c) => c.to_string(),
            None => {
                return Err("verb has neither cmd nor an adverb-routed template".into())
            }
        },
    };

    Ok(Rendered { cmd: template::substitute(&template_str, &context), confirm: confirm_of(verb) })
}

/// Steps 3-5 of `render`: resolve adverbs and build the `{subject, object, verb,
/// adverbs, cwd, …template_vars}` substitution context, with `verb.prompt`
/// pre-rendered and re-injected. Returns `(context, resolved-adverbs)`.
fn build_context(reg: &Value, verb: &Value, subject: &Value, object: &Value, user_adverbs: &Value) -> (Value, Value) {
    let resolved = adverbs::resolve(reg, verb, user_adverbs);
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let mut context = json!({
        "subject": subject,
        "object": object,
        "verb": verb,
        "adverbs": resolved["selected"],
        "cwd": cwd,
    });
    if let Some(tv) = resolved["template_vars"].as_object() {
        let obj = context.as_object_mut().unwrap();
        for (k, v) in tv {
            obj.insert(k.clone(), v.clone());
        }
    }
    let raw_prompt = verb.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
    if !raw_prompt.is_empty() {
        context["verb"]["prompt"] = json!(template::substitute(raw_prompt, &context));
    }
    (context, resolved)
}

fn confirm_of(verb: &Value) -> bool {
    verb.get("confirm").and_then(|c| c.as_bool()).unwrap_or(false)
}

/// Render an arbitrary `template` in the verb's full context — the negotiation
/// executor's entry point for running a verb's chosen `usage` channel `cmd`
/// (slice 2b). Steps 2-6 *minus validation*: it skips the subject-`accepts` and
/// `valid_when` checks because the caller is mid-pipeline and the planner already
/// cleared them at plan time (the subject here is the post-coercion buffer, which
/// may lack the original metadata `valid_when` was written against). The `cmd`
/// sees `{subject.*}` / `{verb.*}` / `{verb.prompt}`, exactly like `verb.cmd`.
pub fn render_template_in_context(
    reg: &Value,
    verb: &Value,
    subject: &Value,
    object: &Value,
    user_adverbs: &Value,
    template: &str,
) -> Rendered {
    let (context, _) = build_context(reg, verb, subject, object, user_adverbs);
    Rendered { cmd: template::substitute(template, &context), confirm: confirm_of(verb) }
}

/// Resolve the indirect OBJECT of a two-step verb against its `object_type`.
/// Port of `resolve_object` in `bin/goo`. Returns `Value::Null` for verbs with
/// no `object_type`; otherwise the matched candidate tagged with `{type}`.
///
/// Candidate pool (in order): an explicit address arg → resolve directly; else
/// `object_list_cmd` (subject-substituted, `bash -c`) → `object_source` (named
/// source `list_cmd`) → any source whose `emits` matches `object_type`. The
/// pool is then filtered by `object_valid_when` (subject-substituted jq), and
/// matched against `object_arg` by id/title substring (empty arg → first).
pub fn resolve_object(
    reg: &Value,
    verb: &Value,
    object_arg: &str,
    subject: &Value,
) -> Result<Value, String> {
    let otype = verb.get("object_type").and_then(|t| t.as_str()).unwrap_or("");
    if otype.is_empty() {
        return Ok(Value::Null);
    }

    // Explicit address form → resolve directly (bypasses the candidate pool).
    if !object_arg.is_empty() && address::is_explicit(object_arg, reg) {
        return address::resolve(object_arg, reg, Some(verb));
    }

    // Gather candidate items (a JSON array as text).
    let items_text = gather_object_items(reg, verb, subject, otype)?;
    let mut items: Vec<Value> = serde_json::from_str(&items_text)
        .ok()
        .and_then(|v: Value| v.as_array().cloned())
        .unwrap_or_default();

    // object_valid_when: subject-substituted jq predicate over each candidate.
    if let Some(ovw) = verb.get("object_valid_when").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let octx = json!({ "subject": subject });
        let pred = template::substitute(ovw, &octx);
        let prog = format!("(try (. // []) catch []) | map(select({pred}))");
        let filtered = jq_filter(&prog, &Value::Array(items.clone()));
        items = filtered
            .as_ref()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        if items.is_empty() {
            return Err(format!(
                "no object of type '{otype}' satisfies this verb's object_valid_when"
            ));
        }
    }

    // Match object_arg against id/title (case-insensitive); empty → first.
    let tag = |item: &Value| {
        let mut o = item.clone();
        if let Some(m) = o.as_object_mut() {
            m.insert("type".into(), json!(otype));
        }
        o
    };
    let chosen = if object_arg.is_empty() {
        // bash: `.[0] | select(. != null)` — the *first* item, iff non-null.
        items.into_iter().next().filter(|i| !i.is_null()).map(|i| tag(&i))
    } else {
        let q = object_arg.to_lowercase();
        items
            .iter()
            .find(|i| {
                let field = |k: &str| {
                    i.get(k)
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_lowercase()
                };
                field("id").contains(&q) || field("title").contains(&q)
            })
            .map(tag)
    };

    chosen.ok_or_else(|| {
        if object_arg.is_empty() {
            format!("could not list objects of type '{otype}' for this verb")
        } else {
            format!("no object matching '{object_arg}' of type '{otype}'")
        }
    })
}

/// Run the candidate-gathering branch of `resolve_object`: `object_list_cmd`
/// (subject-substituted) → `object_source` → emits-matching source. Returns the
/// raw stdout (expected to be a JSON array); errors if nothing produced output.
fn gather_object_items(
    reg: &Value,
    verb: &Value,
    subject: &Value,
    otype: &str,
) -> Result<String, String> {
    let err = || format!("could not list objects of type '{otype}' for this verb");

    if let Some(olist) = verb.get("object_list_cmd").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let ctx = json!({ "subject": subject });
        let rendered = template::substitute(olist, &ctx);
        let out = bash_stdout(&rendered);
        return if out.trim().is_empty() { Err(err()) } else { Ok(out) };
    }

    let sources = reg.get("sources").and_then(|s| s.as_array());
    if let Some(osrc) = verb.get("object_source").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        let lc = sources.and_then(|arr| {
            arr.iter()
                .find(|s| {
                    s.get("name").and_then(|n| n.as_str()) == Some(osrc)
                        || s.get("prefix").and_then(|p| p.as_str()) == Some(osrc)
                })
                .and_then(|s| s.get("list_cmd"))
                .and_then(|c| c.as_str())
        });
        if let Some(lc) = lc {
            let out = bash_stdout(lc);
            if !out.trim().is_empty() {
                return Ok(out);
            }
        }
        return Err(err());
    }

    // Else: first source whose emits matches object_type and produces output.
    if let Some(arr) = sources {
        for source in arr {
            let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
            if emits.is_empty() || !mime::is_subtype(emits, otype, reg) {
                continue;
            }
            if let Some(lc) = source.get("list_cmd").and_then(|c| c.as_str()) {
                let out = bash_stdout(lc);
                if !out.trim().is_empty() {
                    return Ok(out);
                }
            }
        }
    }
    Err(err())
}

/// Run `bash -c <cmd>` and capture stdout (stderr discarded), mirroring the
/// shell's `bash -c "$cmd" 2>/dev/null`.
fn bash_stdout(cmd: &str) -> String {
    Command::new("bash")
        .args(["-c", cmd])
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Apply the jq `program` to `input`, returning the parsed JSON output (or
/// `None` on error). Used for the user-authored `object_valid_when` filter.
fn jq_filter(program: &str, input: &Value) -> Option<Value> {
    let mut child = Command::new("jq")
        .args(["-c", program])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.to_string().as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Convenience for the unit tests / bin: render then run via `bash -c`,
/// returning captured stdout. (Real interactive use inherits stdio; the bin
/// owns that path.)
#[cfg(test)]
fn render_and_run(
    reg: &Value,
    verb: &Value,
    subject: &Value,
    object: &Value,
    user_adverbs: &Value,
) -> Result<String, String> {
    let r = render(reg, verb, subject, object, user_adverbs)?;
    Ok(bash_stdout(&r.cmd))
}

fn jq_truthy(expr: &str, input: &str) -> bool {
    let mut child = match Command::new("jq")
        .args(["-e", expr])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;
    use serde_json::json;

    // The `tests/verbs.bats` `fixture.toml`, loaded through the real registry
    // path so the verb/adverb shapes are exactly what the shell sees.
    fn fixture() -> Value {
        registry::from_fixture_toml(
            "fixture",
            r#"
name = "fixture"

[[types]]
name = "application/vnd.fixture.thing"
display = "fixture thing"
kind = "handle"

[[verbs]]
name = "echo-text"
accepts = ["text/*"]
default_for = "text/plain"
cmd = "echo {subject.text}"

[[verbs]]
name = "echo-id"
accepts = ["application/vnd.fixture.thing"]
cmd = "echo {subject.id}"

[[verbs]]
name = "open-poly"
accepts = ["inode/*", "text/x-uri"]
default_for = ["inode/file", "text/x-uri"]
cmd = "xdg-open {subject.id|q}"

[[verbs]]
name = "destructive"
accepts = ["application/vnd.fixture.thing"]
cmd = "echo would-delete {subject.id}"
confirm = true

[[verbs]]
name = "two-step"
accepts = ["application/vnd.fixture.thing"]
object_type = "application/vnd.fixture.thing"
cmd = "echo move {subject.id} to {object.id}"

[[verbs]]
name = "only-zip"
accepts = ["text/*"]
valid_when = ".text | endswith(\".zip\")"
cmd = "echo zipping {subject.text}"

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

    // ---- lookup ----

    #[test]
    fn lookup_returns_known_verb() {
        let reg = fixture();
        assert_eq!(lookup(&reg, "echo-text", None).unwrap()["name"], json!("echo-text"));
    }

    #[test]
    fn lookup_unknown_is_none() {
        assert!(lookup(&fixture(), "does-not-exist", None).is_none());
    }

    #[test]
    fn lookup_type_filter_accepts_and_rejects() {
        let reg = fixture();
        assert!(lookup(&reg, "echo-text", Some("text/plain")).is_some());
        assert!(lookup(&reg, "echo-text", Some("application/vnd.fixture.thing")).is_none());
    }

    // ---- default_for ----

    #[test]
    fn default_for_string_match() {
        assert_eq!(default_for(&fixture(), "text/plain").unwrap()["name"], json!("echo-text"));
    }

    #[test]
    fn default_for_no_default_is_none() {
        assert!(default_for(&fixture(), "image/png").is_none());
    }

    #[test]
    fn default_for_array_matches_each_listed_type() {
        let reg = fixture();
        assert_eq!(default_for(&reg, "inode/file").unwrap()["name"], json!("open-poly"));
        assert_eq!(default_for(&reg, "text/x-uri").unwrap()["name"], json!("open-poly"));
    }

    // ---- for_subject ----

    fn names(verbs: &[Value]) -> Vec<String> {
        verbs.iter().map(|v| v["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn for_subject_text_plain() {
        let reg = fixture();
        let n = names(&for_subject(&reg, &json!({"type":"text/plain","text":"hi"})));
        for want in ["echo-text", "critique", "think"] {
            assert!(n.contains(&want.to_string()), "missing {want} in {n:?}");
        }
        for unwanted in ["echo-id", "destructive", "two-step"] {
            assert!(!n.contains(&unwanted.to_string()), "unexpected {unwanted}");
        }
    }

    #[test]
    fn for_subject_vendor_type() {
        let reg = fixture();
        let n = names(&for_subject(&reg, &json!({"type":"application/vnd.fixture.thing","id":"x"})));
        for want in ["echo-id", "destructive", "two-step"] {
            assert!(n.contains(&want.to_string()), "missing {want}");
        }
        for unwanted in ["echo-text", "critique"] {
            assert!(!n.contains(&unwanted.to_string()), "unexpected {unwanted}");
        }
    }

    #[test]
    fn for_subject_filters_on_valid_when() {
        let reg = fixture();
        let zip = names(&for_subject(&reg, &json!({"type":"text/plain","text":"a.zip"})));
        let txt = names(&for_subject(&reg, &json!({"type":"text/plain","text":"a.txt"})));
        assert!(zip.contains(&"only-zip".to_string()));
        assert!(!txt.contains(&"only-zip".to_string()));
        assert!(zip.contains(&"echo-text".to_string()));
        assert!(txt.contains(&"echo-text".to_string()));
    }

    // ---- cross-plugin polymorphism (multi-verb-per-name) ----

    // The fixture: TWO verbs named `connect` with different `accepts`. Today's
    // engine accumulates them (registry::merge_verbs); lookup picks by
    // specificity for the subject type; for_subject returns the right one per name.
    fn poly_reg() -> Value {
        json!({
            "types": [],
            "verbs": [
                { "name": "connect", "accepts": ["application/vnd.ssh.host"], "cmd": "ssh {subject.id|q}" },
                { "name": "connect", "accepts": ["application/vnd.bluez.device"], "cmd": "bluetoothctl connect {subject.id|q}" },
                { "name": "open", "accepts": ["*/*"], "cmd": "open-anything" },
                { "name": "open", "accepts": ["text/plain"], "cmd": "open-text" },
            ]
        })
    }

    #[test]
    fn lookup_picks_most_specific_match_for_type() {
        let reg = poly_reg();
        // Two `connect` verbs; ssh subject → the ssh-accepting one (exact-pattern match).
        let v = lookup(&reg, "connect", Some("application/vnd.ssh.host")).unwrap();
        assert_eq!(v["cmd"], "ssh {subject.id|q}");
        // BT subject → the bluez-accepting one.
        let v = lookup(&reg, "connect", Some("application/vnd.bluez.device")).unwrap();
        assert_eq!(v["cmd"], "bluetoothctl connect {subject.id|q}");
        // A subject neither accepts → None.
        assert!(lookup(&reg, "connect", Some("text/plain")).is_none());
    }

    #[test]
    fn lookup_exact_pattern_beats_glob() {
        let reg = poly_reg();
        // Two `open` verbs: `*/*` (most permissive) and `text/plain` (exact for text/plain).
        // For a text/plain subject, the exact-match `open` must win over `*/*`.
        let v = lookup(&reg, "open", Some("text/plain")).unwrap();
        assert_eq!(v["cmd"], "open-text");
        // For an image, only `*/*` matches.
        let v = lookup(&reg, "open", Some("image/png")).unwrap();
        assert_eq!(v["cmd"], "open-anything");
    }

    #[test]
    fn lookup_no_type_returns_first_by_name() {
        let reg = poly_reg();
        // Without a type filter, lookup returns the first verb named `connect` in registry order.
        let v = lookup(&reg, "connect", None).unwrap();
        assert_eq!(v["accepts"][0], "application/vnd.ssh.host");
    }

    #[test]
    fn for_subject_dedups_by_name_keeping_most_specific() {
        let reg = poly_reg();
        // Two `open` verbs both match text/plain; for_subject must return ONE
        // entry for "open" (the most-specific impl), not two.
        let verbs = for_subject(&reg, &json!({"type": "text/plain"}));
        let opens: Vec<&Value> = verbs.iter().filter(|v| v["name"] == "open").collect();
        assert_eq!(opens.len(), 1, "for_subject leaked two `open` impls");
        assert_eq!(opens[0]["cmd"], "open-text", "kept the wrong impl");
    }

    // ---- render / apply ----

    fn verb(reg: &Value, name: &str) -> Value {
        lookup(reg, name, None).unwrap()
    }

    #[test]
    fn render_executes_direct_cmd_with_subject_substitution() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "echo-text"),
            &json!({"type":"text/plain","text":"hello world"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap();
        assert_eq!(out.trim_end(), "hello world");
    }

    #[test]
    fn render_handle_verb_substitutes_id() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "echo-id"),
            &json!({"type":"application/vnd.fixture.thing","id":"abc-123"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap();
        assert_eq!(out.trim_end(), "abc-123");
    }

    #[test]
    fn render_rejects_mismatched_subject_type() {
        let reg = fixture();
        let err = render(
            &reg,
            &verb(&reg, "echo-id"),
            &json!({"type":"text/plain","text":"oops"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap_err();
        assert!(err.contains("does not match verb accepts"), "{err}");
    }

    #[test]
    fn render_two_step_substitutes_object() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "two-step"),
            &json!({"type":"application/vnd.fixture.thing","id":"src"}),
            &json!({"type":"application/vnd.fixture.thing","id":"dst"}),
            &json!({}),
        )
        .unwrap();
        assert_eq!(out.trim_end(), "move src to dst");
    }

    #[test]
    fn render_two_step_fails_without_object() {
        let reg = fixture();
        let err = render(
            &reg,
            &verb(&reg, "two-step"),
            &json!({"type":"application/vnd.fixture.thing","id":"src"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap_err();
        assert!(err.contains("requires object"), "{err}");
    }

    #[test]
    fn render_rejects_subject_failing_valid_when() {
        let reg = fixture();
        let err = render(
            &reg,
            &verb(&reg, "only-zip"),
            &json!({"type":"text/plain","text":"notes.txt"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap_err();
        assert!(err.contains("valid_when"), "{err}");
    }

    #[test]
    fn render_runs_when_valid_when_passes() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "only-zip"),
            &json!({"type":"text/plain","text":"archive.zip"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap();
        assert!(out.contains("zipping archive.zip"), "{out}");
    }

    #[test]
    fn render_confirm_flag_is_surfaced() {
        let reg = fixture();
        let r = render(
            &reg,
            &verb(&reg, "destructive"),
            &json!({"type":"application/vnd.fixture.thing","id":"q"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap();
        assert!(r.confirm);
        assert_eq!(r.cmd, "echo would-delete q");
    }

    // ---- adverb-routed render ----

    #[test]
    fn render_critique_via_clipboard_routes_through_prompt() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "critique"),
            &json!({"type":"text/plain","text":"important text"}),
            &Value::Null,
            &json!({"via":"clipboard"}),
        )
        .unwrap();
        assert!(out.contains("Review:"), "{out}");
        assert!(out.contains("important text"), "{out}");
    }

    #[test]
    fn render_critique_uses_default_adverb() {
        let reg = fixture();
        // No adverbs given → via defaults to clipboard; renders without error.
        let r = render(
            &reg,
            &verb(&reg, "critique"),
            &json!({"type":"text/plain","text":"x"}),
            &Value::Null,
            &json!({}),
        )
        .unwrap();
        assert!(r.cmd.contains("cat <<<"), "{}", r.cmd);
    }

    #[test]
    fn render_think_depth_ultra_injects_template_var() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "think"),
            &json!({"type":"text/plain","text":"the thing"}),
            &Value::Null,
            &json!({"via":"clipboard","depth":"ultra"}),
        )
        .unwrap();
        assert!(out.contains("Ultrathink about"), "{out}");
    }

    #[test]
    fn render_think_depth_normal_default_injection() {
        let reg = fixture();
        let out = render_and_run(
            &reg,
            &verb(&reg, "think"),
            &json!({"type":"text/plain","text":"x"}),
            &Value::Null,
            &json!({"via":"clipboard"}),
        )
        .unwrap();
        assert!(out.contains("Think about"), "{out}");
    }

    // ---- resolve_object ----

    #[test]
    fn resolve_object_null_for_one_step_verb() {
        let reg = fixture();
        let r = resolve_object(&reg, &verb(&reg, "echo-text"), "", &json!({"type":"text/plain"}))
            .unwrap();
        assert_eq!(r, Value::Null);
    }

    #[test]
    fn resolve_object_subject_dependent_list_first_item() {
        // object_list_cmd renders {subject.id} into a JSON-array command; no arg
        // → first candidate, tagged with object_type.
        let reg = fixture();
        let v = json!({
            "name": "twostep",
            "accepts": ["application/vnd.fixture.thing"],
            "object_type": "application/vnd.fixture.thing",
            "object_list_cmd": r#"printf '[{"id":"{subject.id}-a"},{"id":"{subject.id}-b"}]'"#,
            "cmd": "echo move {subject.id} to {object.id}"
        });
        let obj = resolve_object(&reg, &v, "", &json!({"id":"src"})).unwrap();
        assert_eq!(obj["id"], json!("src-a"));
        assert_eq!(obj["type"], json!("application/vnd.fixture.thing"));
    }

    #[test]
    fn resolve_object_matches_arg_by_id_substring() {
        let reg = fixture();
        let v = json!({
            "object_type": "application/vnd.fixture.thing",
            "object_list_cmd": r#"printf '[{"id":"alpha"},{"id":"beta"}]'"#,
        });
        let obj = resolve_object(&reg, &v, "bet", &json!(null)).unwrap();
        assert_eq!(obj["id"], json!("beta"));
    }

    #[test]
    fn resolve_object_valid_when_filters_pool() {
        let reg = fixture();
        let v = json!({
            "object_type": "application/vnd.fixture.thing",
            "object_list_cmd": r#"printf '[{"id":"keep","ok":true},{"id":"drop","ok":false}]'"#,
            "object_valid_when": ".ok == true",
        });
        // No arg → first candidate AFTER the valid_when filter.
        let obj = resolve_object(&reg, &v, "", &json!(null)).unwrap();
        assert_eq!(obj["id"], json!("keep"));
    }

    #[test]
    fn absent_predicate_is_always_true() {
        assert!(satisfies("", &json!({"text": "anything"})));
        assert!(satisfies("   ", &json!({})));
    }

    #[test]
    fn endswith_predicate() {
        // the only valid_when in the suite (verbs.bats fixture)
        assert!(satisfies(r#".text | endswith(".zip")"#, &json!({"text": "a.zip"})));
        assert!(!satisfies(r#".text | endswith(".zip")"#, &json!({"text": "a.txt"})));
    }

    // The scoping-doc parity checklist — all must work (they run through the
    // real jq, so parity is by construction).
    #[test]
    fn checklist_constructs_work() {
        let s = json!({"id": "Report.ZIP", "text": "hello", "type": "inode/file"});
        assert!(satisfies(r#".text | startswith("hel")"#, &s));
        assert!(satisfies(r#".id | ascii_downcase | endswith(".zip")"#, &s));
        assert!(satisfies(r#".id | test("(?i)\\.zip$")"#, &s));
        assert!(satisfies(r#".type | contains("inode")"#, &s));
        assert!(satisfies(r#"(.missing // "fallback") == "fallback""#, &s));
        assert!(satisfies(r#".text | select(. == "hello") | length > 0"#, &s));
    }

    #[test]
    fn false_and_null_are_not_truthy() {
        assert!(!satisfies(".nope", &json!({"text": "x"}))); // null
        assert!(!satisfies(r#".text == "other""#, &json!({"text": "x"}))); // false
    }
}
