//! MIME type matching — the Rust port of `mime_matches` in `lib/types.sh`.
//!
//! The bash uses a `case "$mime" in $pattern)` glob. We mirror that: `*` matches
//! any run of characters (including `/`), so `text/*` matches
//! `text/plain;charset=utf-8`, `*/json` matches `application/json`, and
//! `application/vnd.x.*` matches a vendor subtype. An empty pattern or empty
//! type never matches. Other bash-glob metacharacters (`?`, `[...]`) are not
//! used by any shipped pattern, so they are treated literally here; if a plugin
//! ever needs them, extend `glob_match` and add a parity test.
//!
//! `mime_detect_path` / `mime_detect_content` (which shell out to `file` and
//! touch the filesystem) land with the address slice, since detection feeds
//! canonicalization.

/// Returns true iff `mime` matches the glob `pattern`, mirroring
/// `lib/types.sh::mime_matches`. Empty `pattern` or empty `mime` never match.
pub fn mime_matches(pattern: &str, mime: &str) -> bool {
    if pattern.is_empty() || mime.is_empty() {
        return false;
    }
    glob_match(pattern, mime)
}

/// Minimal glob where only `*` is special (matches any sequence, including an
/// empty one). Implemented by splitting the pattern on `*` and matching the
/// literal segments left-to-right: the first must be a prefix, the last a
/// suffix, and any middle segment must occur in order.
fn glob_match(pattern: &str, text: &str) -> bool {
    let segments: Vec<&str> = pattern.split('*').collect();
    if segments.len() == 1 {
        // No `*` — exact match.
        return pattern == text;
    }
    let last = segments.len() - 1;
    let mut pos = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        if i == 0 {
            // Leading literal must be a prefix.
            if !text[pos..].starts_with(seg) {
                return false;
            }
            pos += seg.len();
        } else if i == last {
            // Trailing literal must be a suffix of the remainder.
            return text[pos..].ends_with(seg);
        } else {
            // Middle literal must appear at or after the current position.
            match text[pos..].find(seg) {
                Some(idx) => pos += idx + seg.len(),
                None => return false,
            }
        }
    }
    // Pattern ended on a `*` (empty trailing segment): the rest matches.
    true
}

use serde_json::Value;
use std::collections::HashSet;

/// True iff `sub` is a **subtype** of `sup` — a *superset* of `mime_matches`
/// (the type-system lattice). `mime_matches` stays the pure glob primitive; this
/// adds:
///   - equality, and glob where `sup` is the accept-pattern (`text/*`, `*/json`);
///   - **structured-syntax suffix**, same top-level (RFC 6839):
///     `application/vnd.api+json` <: `application/json`;
///   - **declared transitive `is_a`** edges from the registry's `[[types]]`
///     (a DAG — `text/csv is_a = ["text/plain", …]`).
///
/// Additive: with no `+suffix` and no declared `is_a`, this is exactly
/// `mime_matches(sup, sub)` — so wiring it into accept-matching never *removes*
/// a match.
pub fn is_subtype(sub: &str, sup: &str, reg: &Value) -> bool {
    let mut seen = HashSet::new();
    is_subtype_rec(sub, sup, reg, &mut seen)
}

fn is_subtype_rec(sub: &str, sup: &str, reg: &Value, seen: &mut HashSet<String>) -> bool {
    if sub == sup {
        return true;
    }
    // Glob: `sup` is the accept-pattern, `sub` the concrete type.
    if mime_matches(sup, sub) {
        return true;
    }
    // Guard cycles / re-exploration (is_a is a DAG; the visited set keeps it linear).
    if !seen.insert(sub.to_string()) {
        return false;
    }
    // Structured-syntax suffix (same top-level): `T/x+suf` <: `T/suf`.
    if let Some(parent) = suffix_supertype(sub) {
        if is_subtype_rec(&parent, sup, reg, seen) {
            return true;
        }
    }
    // Declared `is_a` supertypes.
    for parent in declared_supertypes(sub, reg) {
        if is_subtype_rec(&parent, sup, reg, seen) {
            return true;
        }
    }
    false
}

/// `T/<name>+<suffix>` → `T/<suffix>` (RFC 6839, **same top-level only**; a
/// cross-top-level supertype like `application/xml` for `image/svg+xml` must be
/// declared explicitly via `is_a`). Params after `;` are dropped. `None` if the
/// subtype carries no `+suffix`.
fn suffix_supertype(mime: &str) -> Option<String> {
    let (top, rest) = mime.split_once('/')?;
    let rest = rest.split(';').next().unwrap_or(rest);
    let suffix = rest.rsplit_once('+')?.1;
    if suffix.is_empty() {
        None
    } else {
        Some(format!("{top}/{suffix}"))
    }
}

/// The supertypes a type declares via `[[types]] is_a = [...]` in the registry.
fn declared_supertypes(sub: &str, reg: &Value) -> Vec<String> {
    reg.get("types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|t| t.get("name").and_then(Value::as_str) == Some(sub))
                .filter_map(|t| t.get("is_a").and_then(Value::as_array))
                .flatten()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// MIME type of a file on disk via libmagic (`file --mime-type -b`). Err if the
/// path doesn't exist — mirrors `mime_detect_path`.
pub fn detect_path(path: &str) -> Result<String, String> {
    if !Path::new(path).exists() {
        return Err(format!("mime_detect_path: not found: {path}"));
    }
    let out = Command::new("file")
        .args(["--mime-type", "-b", "--", path])
        .output()
        .map_err(|e| format!("mime_detect_path: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// MIME type of an arbitrary string — port of `mime_detect_content`. In order:
/// URI scheme → `text/x-uri`; an existing single-line path → its file type;
/// libmagic on the content; else `text/plain`.
pub fn detect_content(content: &str) -> String {
    if looks_like_uri(content) {
        return "text/x-uri".to_string();
    }
    if !content.contains('\n') && Path::new(content).exists() {
        if let Ok(m) = detect_path(content) {
            return m;
        }
    }
    if let Some(detected) = file_on_stdin(content) {
        if !detected.is_empty() && detected != "application/octet-stream" {
            return detected;
        }
    }
    "text/plain".to_string()
}

/// `^[A-Za-z][A-Za-z0-9+.-]*://` followed by a non-space — the RFC-3986 scheme
/// shape the shell uses to spot a URL.
fn looks_like_uri(s: &str) -> bool {
    let Some(idx) = s.find("://") else { return false };
    if idx == 0 {
        return false;
    }
    let mut scheme = s[..idx].chars();
    if !scheme.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if !scheme.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return false;
    }
    s[idx + 3..].chars().next().is_some_and(|c| !c.is_whitespace())
}

fn file_on_stdin(content: &str) -> Option<String> {
    let mut child = Command::new("file")
        .args(["--mime-type", "-b", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(content.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True if `content` (trimmed) is a complete JSON object or array — a *positive*
/// structural signal. Unlike libmagic (`file_on_stdin`), which is unreliable on
/// short strings, this actually parses, so `{"k":1}` is recognized.
pub fn looks_like_json(content: &str) -> bool {
    let t = content.trim();
    (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<Value>(t).is_ok()
}

/// Run the registry's declared `[[checkers]]` against `content`: each checker that
/// *verifies* yields its `(target, weight)` candidate, *additive* to
/// `detect_content`'s no-context default. Replaces the old hardwired JSON shape —
/// the json check now ships as a declared checker (`core.toml`, `builtin = "json"`).
/// Empty for unstructured content (the common case), so it never perturbs
/// `detect_content`. Weight = the checker's tier (default `strong` = 2.0,
/// preserving the prior json weight). See doc/design/detection.md.
pub fn infer_candidates(content: &str, reg: &Value) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let Some(checkers) = reg.get("checkers").and_then(Value::as_array) else { return out };
    for c in checkers {
        let Some(target) = c.get("target").and_then(Value::as_str) else { continue };
        if run_checker(c, content) {
            out.push((target.to_string(), tier_weight(c.get("tier").and_then(Value::as_str))));
        }
    }
    out
}

/// Execute a checker's verdict on `content`. v1 implements the `builtin` impls
/// inline; `cmd` checkers run via the cmd runner (slice 2b) — until then a declared
/// `cmd` checker validates but is inert (skipped here).
fn run_checker(c: &Value, content: &str) -> bool {
    match c.get("builtin").and_then(Value::as_str) {
        Some("json") => looks_like_json(content),
        Some(_) => false, // unknown builtin (load-time validator rejects these)
        None => false,    // cmd checker — slice 2b
    }
}

/// Detection-tier → ranking weight. Absent = `strong` = 2.0, preserving the prior
/// hardwired json weight (see [`DETECTION_TIERS`]).
fn tier_weight(tier: Option<&str>) -> f64 {
    match tier.unwrap_or("strong") {
        "certain" => 3.0,
        "strong" => 2.0,
        "medium" => 1.0,
        "weak" => 0.5,
        _ => 2.0,
    }
}

/// Context-sensitive inference: of the structural candidates `content` yields,
/// return the highest-weighted one the verb *accepts* (subtype-aware), else
/// `None`. `None` means "no positive signal this verb wants" — the caller falls
/// back to `detect_content`, so today's behavior is preserved exactly. A type
/// verb only sees an inferred type when both (a) the content positively looks
/// like it and (b) the verb's `accepts` admits it.
pub fn infer_for(content: &str, verb: &Value, reg: &Value) -> Option<String> {
    let accepts = verb.get("accepts").and_then(|a| a.as_array())?;
    let mut best: Option<(String, f64)> = None;
    for (mime, w) in infer_candidates(content, reg) {
        // A structural candidate earns its seat only when the verb is asking for
        // the structured representation *specifically* — an accept pattern that
        // matches the candidate but would NOT also accept plain text. This is the
        // gating rule: a `text/*` verb sees plain text (the detect_content
        // fallback), not the candidate, because `text/plain` already satisfies
        // `text/*`; only a verb that accepts e.g. `application/json` (which doesn't
        // subsume `text/plain`) gets the structured type. Without it, a structural
        // signal would hijack every generic text verb (e.g. `goo upper '{"k":1}'`).
        let earns_seat = accepts.iter().filter_map(|p| p.as_str()).any(|pat| {
            is_subtype(&mime, pat, reg) && !is_subtype("text/plain", pat, reg)
        });
        if earns_seat && best.as_ref().is_none_or(|(_, bw)| w > *bw) {
            best = Some((mime, w));
        }
    }
    best.map(|(m, _)| m)
}

// ---- declared detectors / checkers (see doc/design/detection.md) ----
//
// A **detector** classifies content → a type; a **checker** verifies content
// against a `target` type → yes/no. Both are registry entries (`[[detectors]]` /
// `[[checkers]]`), declared not hardwired, implemented by `cmd` (primary) or a
// named native `builtin`. These validators are the registry-load contract
// (mirroring `negotiation::validate_channels`); the runner that *executes* them
// lands in a later slice — until then these collections carry data with no
// consumer, so the validators just keep malformed declarations out.

/// Detection-confidence tiers — a signal's *nature* (does a yes mean yes?), not
/// its impl, and distinct from converter **cost** tiers. Defaulted per kind
/// (checker = `strong`, detector = `medium`).
pub const DETECTION_TIERS: &[&str] = &["certain", "strong", "medium", "weak"];

/// Validate `[[detectors]]` declarations. No-op until a plugin ships one.
pub fn validate_detectors(reg: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let Some(arr) = reg.get("detectors").and_then(Value::as_array) else { return errs };
    for d in arr {
        let name = d.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
        check_impl(d, "detector", name, &mut errs);
        check_tier(d, "detector", name, &mut errs);
    }
    errs
}

/// Validate `[[checkers]]` declarations. A checker additionally needs a `target`
/// type to verify against. No-op until a plugin ships one.
pub fn validate_checkers(reg: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let Some(arr) = reg.get("checkers").and_then(Value::as_array) else { return errs };
    for c in arr {
        let name = c.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
        if c.get("target").and_then(Value::as_str).filter(|s| !s.is_empty()).is_none() {
            errs.push(format!("checker \"{name}\" needs a target type to verify"));
        }
        check_impl(c, "checker", name, &mut errs);
        check_tier(c, "checker", name, &mut errs);
    }
    errs
}

/// Exactly one of `cmd` (shell) or `builtin` (named native primitive).
fn check_impl(it: &Value, kind: &str, name: &str, errs: &mut Vec<String>) {
    let has = |k: &str| it.get(k).and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    match (has("cmd"), has("builtin")) {
        (false, false) => errs.push(format!("{kind} \"{name}\" needs a cmd or builtin impl")),
        (true, true) => errs.push(format!("{kind} \"{name}\" has both cmd and builtin — pick one")),
        _ => {}
    }
}

fn check_tier(it: &Value, kind: &str, name: &str, errs: &mut Vec<String>) {
    if let Some(t) = it.get("tier").and_then(Value::as_str) {
        if !DETECTION_TIERS.contains(&t) {
            errs.push(format!(
                "{kind} \"{name}\" has unknown tier \"{t}\" (certain|strong|medium|weak)"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mime_matches;

    // These mirror, one-to-one, the `mime_matches` cases in tests/types.bats.
    #[test]
    fn exact_match() {
        assert!(mime_matches("text/plain", "text/plain"));
    }
    #[test]
    fn exact_non_match() {
        assert!(!mime_matches("text/plain", "text/markdown"));
    }
    #[test]
    fn suffix_wildcard_matches_markdown() {
        assert!(mime_matches("text/*", "text/markdown"));
    }
    #[test]
    fn suffix_wildcard_matches_plain() {
        assert!(mime_matches("text/*", "text/plain"));
    }
    #[test]
    fn suffix_wildcard_does_not_cross_supertype() {
        assert!(!mime_matches("text/*", "application/json"));
    }
    #[test]
    fn prefix_wildcard_matches() {
        assert!(mime_matches("*/json", "application/json"));
    }
    #[test]
    fn prefix_wildcard_non_match() {
        assert!(!mime_matches("*/json", "application/xml"));
    }
    #[test]
    fn vendor_wildcard_matches_subtype() {
        assert!(mime_matches(
            "application/vnd.tmux-use.*",
            "application/vnd.tmux-use.session"
        ));
    }
    #[test]
    fn vendor_wildcard_different_vendor() {
        assert!(!mime_matches(
            "application/vnd.tmux-use.*",
            "application/vnd.cos-cli.app"
        ));
    }
    #[test]
    fn text_star_matches_charset_parameter() {
        assert!(mime_matches("text/*", "text/plain;charset=utf-8"));
    }
    #[test]
    fn empty_pattern_no_match() {
        assert!(!mime_matches("", "text/plain"));
    }
    #[test]
    fn empty_mime_no_match() {
        assert!(!mime_matches("text/*", ""));
    }

    // A couple beyond the bats set, pinning glob edge cases the port relies on.
    #[test]
    fn bare_star_matches_anything_nonempty() {
        assert!(mime_matches("*", "anything/at-all"));
        assert!(!mime_matches("*", "")); // empty mime still never matches
    }
    #[test]
    fn exact_with_no_wildcard_is_strict() {
        assert!(!mime_matches("text/pla", "text/plain"));
    }

    // ---- is_subtype (the type lattice; a superset of mime_matches) ----
    use super::is_subtype;
    use serde_json::json;

    #[test]
    fn subtype_exact_and_glob() {
        let r = json!({});
        assert!(is_subtype("text/plain", "text/plain", &r));
        assert!(is_subtype("text/csv", "text/*", &r));
        assert!(is_subtype("application/json", "*/json", &r));
        assert!(!is_subtype("application/json", "text/*", &r));
        assert!(!is_subtype("", "text/plain", &r));
    }

    #[test]
    fn subtype_structured_suffix_same_top_level() {
        let r = json!({});
        assert!(is_subtype("application/vnd.api+json", "application/json", &r));
        assert!(is_subtype("application/vnd.git+json;charset=utf-8", "application/json", &r));
        // cross-top-level is NOT implied — must be declared explicitly.
        assert!(!is_subtype("image/svg+xml", "application/xml", &r));
        // the structured supertype is not itself a glob pattern.
        assert!(!is_subtype("application/json", "application/*+json", &r));
    }

    #[test]
    fn subtype_declared_is_a_transitive_dag() {
        let r = json!({ "types": [
            { "name": "application/vnd.git.repo", "is_a": ["inode/directory"] },
            { "name": "text/csv", "is_a": ["text/plain", "application/vnd.tabular"] },
            { "name": "text/tsv", "is_a": ["text/csv"] }
        ]});
        assert!(is_subtype("application/vnd.git.repo", "inode/directory", &r)); // direct edge
        assert!(is_subtype("application/vnd.git.repo", "inode/*", &r));         // edge then glob
        assert!(is_subtype("text/csv", "text/plain", &r));
        assert!(is_subtype("text/csv", "application/vnd.tabular", &r));         // second parent (DAG)
        assert!(is_subtype("text/tsv", "text/plain", &r));                     // transitive tsv→csv→plain
        assert!(!is_subtype("text/csv", "application/json", &r));               // no path
    }

    #[test]
    fn subtype_cycle_guard_terminates() {
        let r = json!({ "types": [
            { "name": "a/x", "is_a": ["a/y"] },
            { "name": "a/y", "is_a": ["a/x"] }
        ]});
        assert!(!is_subtype("a/x", "a/z", &r)); // must terminate, not match
        assert!(is_subtype("a/x", "a/y", &r));  // the real edge still resolves
    }

    // ---- detection (mirror tests/types.bats mime_detect_*) ----
    use super::{detect_content, detect_path};

    #[test]
    fn detect_https_url() {
        assert_eq!(detect_content("https://example.com"), "text/x-uri");
    }
    #[test]
    fn detect_http_url_with_query() {
        assert_eq!(detect_content("http://example.com/path?q=1"), "text/x-uri");
    }
    #[test]
    fn detect_custom_scheme_url() {
        assert_eq!(detect_content("claude://claude.ai/new?q=hi"), "text/x-uri");
    }
    #[test]
    fn detect_plain_text() {
        assert!(detect_content("just some words here").starts_with("text/"));
    }
    #[test]
    fn detect_multiline_is_not_url_or_path() {
        assert!(detect_content("line one\nline two").starts_with("text/"));
    }
    #[test]
    fn detect_existing_path_is_its_file_type() {
        let dir = std::env::temp_dir().join(format!("goo-mime-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sample.txt");
        std::fs::write(&f, "hello\n").unwrap();
        let p = f.to_str().unwrap();
        assert!(detect_content(p).starts_with("text/"));
        assert!(detect_path(p).unwrap().starts_with("text/"));
        assert!(detect_path(&format!("{p}.nope")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- structural inference (infer_candidates / infer_for) ----
    use super::{infer_candidates, infer_for, looks_like_json};
    use serde_json::json as j;

    // A registry carrying just the core `json` checker — what `load_all()` seeds.
    fn json_reg() -> serde_json::Value {
        j!({ "checkers": [{ "name": "json", "target": "application/json", "builtin": "json" }] })
    }

    #[test]
    fn json_shape_detects_objects_and_arrays() {
        assert!(looks_like_json(r#"{"k":1}"#));
        assert!(looks_like_json("  [1,2,3] "));
        assert!(!looks_like_json("just words"));
        assert!(!looks_like_json("{not json"));
        assert!(!looks_like_json("42")); // a bare scalar isn't an object/array
    }

    #[test]
    fn infer_candidates_is_empty_for_plain_text() {
        // The common case: no structural signal, so nothing perturbs detect_content.
        assert!(infer_candidates("hello world", &json_reg()).is_empty());
    }

    #[test]
    fn infer_candidates_offers_json_above_baseline() {
        let c = infer_candidates(r#"{"k":1}"#, &json_reg());
        assert_eq!(c, vec![("application/json".to_string(), 2.0)]);
    }

    #[test]
    fn infer_candidates_empty_without_a_checker() {
        // Registry-driven: no json checker declared ⇒ no candidate, even for json.
        assert!(infer_candidates(r#"{"k":1}"#, &j!({})).is_empty());
    }

    #[test]
    fn infer_for_picks_json_when_verb_accepts_it() {
        let reg = json_reg();
        let verb = j!({ "accepts": ["application/json"] });
        assert_eq!(infer_for(r#"{"k":1}"#, &verb, &reg).as_deref(), Some("application/json"));
    }

    #[test]
    fn infer_for_declines_json_for_a_text_only_verb() {
        // Parity direction: a text-only verb never gets json from inference.
        let reg = json_reg();
        let verb = j!({ "accepts": ["text/*"] });
        assert_eq!(infer_for(r#"{"k":1}"#, &verb, &reg), None);
    }

    #[test]
    fn infer_for_declines_plain_text() {
        let reg = json_reg();
        let verb = j!({ "accepts": ["application/json"] });
        assert_eq!(infer_for("just words", &verb, &reg), None);
    }

    // The gating rule, exercised where it matters: a registry where
    // `application/json is_a text/plain` (as shipped). A text/* verb must NOT get
    // the json candidate (text/plain already satisfies text/*), or it would
    // hijack every generic text verb on structured-looking input.
    #[test]
    fn infer_for_gating_text_star_verb_gets_no_structured_candidate() {
        let reg = j!({
            "types": [{ "name": "application/json", "is_a": ["text/plain"] }],
            "checkers": [{ "name": "json", "target": "application/json", "builtin": "json" }],
        });
        // sanity: json IS a subtype of text/* in this reg…
        assert!(is_subtype("application/json", "text/*", &reg));
        // …yet a text/* verb still declines the json candidate (gating).
        let text_verb = j!({ "accepts": ["text/*"] });
        assert_eq!(infer_for(r#"{"k":1}"#, &text_verb, &reg), None);
        // A verb specifically accepting json still gets it.
        let json_verb = j!({ "accepts": ["application/json"] });
        assert_eq!(infer_for(r#"{"k":1}"#, &json_verb, &reg).as_deref(), Some("application/json"));
    }

    // ---- detector / checker validators ----
    use super::{validate_checkers, validate_detectors};

    #[test]
    fn validators_pass_on_empty_or_absent() {
        assert!(validate_detectors(&j!({})).is_empty());
        assert!(validate_checkers(&j!({})).is_empty());
        assert!(validate_detectors(&j!({ "detectors": [] })).is_empty());
    }

    #[test]
    fn well_formed_detector_and_checker_validate() {
        let reg = j!({
            "detectors": [{ "name": "libmagic", "cmd": "file --mime-type -b" }],
            "checkers":  [{ "name": "json", "target": "application/json", "cmd": "jq -e ." }],
        });
        assert!(validate_detectors(&reg).is_empty());
        assert!(validate_checkers(&reg).is_empty());
    }

    #[test]
    fn checker_without_target_is_flagged() {
        let reg = j!({ "checkers": [{ "name": "json", "cmd": "jq -e ." }] });
        let errs = validate_checkers(&reg);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("needs a target"), "{}", errs[0]);
    }

    #[test]
    fn missing_or_double_impl_is_flagged() {
        let no_impl = j!({ "detectors": [{ "name": "x" }] });
        assert!(validate_detectors(&no_impl)[0].contains("cmd or builtin"));
        let both = j!({ "checkers": [{ "name": "json", "target": "application/json",
                                       "cmd": "jq -e .", "builtin": "serde-json" }] });
        assert!(validate_checkers(&both)[0].contains("both cmd and builtin"));
    }

    #[test]
    fn unknown_tier_is_flagged() {
        let reg = j!({ "detectors": [{ "name": "x", "cmd": "c", "tier": "bogus" }] });
        assert!(validate_detectors(&reg)[0].contains("unknown tier"));
        // a valid tier is accepted
        let ok = j!({ "detectors": [{ "name": "x", "cmd": "c", "tier": "medium" }] });
        assert!(validate_detectors(&ok).is_empty());
    }
}
