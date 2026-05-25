//! Subject addressing — the Rust port of `lib/address.sh`.
//!
//! Turns a user-typed argument (or a programmatic URI) into a resolved subject
//! (`serde_json::Value` of shape `{type,text,id?,title?,metadata?}`).
//!
//! Canonical forms (post the goo:// rename):
//!   `goo://<source>/<input>[?params]`  — source lookup (search list_cmd output)
//!   `goo+<scheme>:<value>`             — scheme handoff (direct construction)
//!
//! Sigils (`:` source, `+` handoff, custom single-char) and native shapes
//! (`./ ~/ /` → file, `scheme://` → url, else text) rewrite into those.

use crate::{mime, selection};
use serde_json::{json, Value};
use std::io::Read;

/// Look up a custom sigil's expansion (`.sigils[].char` → `.expands`).
fn sigil_expand(ch: char, reg: &Value) -> Option<String> {
    reg.get("sigils")?.as_array()?.iter().find_map(|s| {
        let c = s.get("char")?.as_str()?;
        if c.chars().next() == Some(ch) {
            s.get("expands")?.as_str().map(str::to_string)
        } else {
            None
        }
    })
}

/// `[A-Za-z]…://…` — the loose native-URL shape the shell `case` uses.
fn has_scheme_sep(s: &str) -> bool {
    s.contains("://") && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

fn starts_core_or_native(s: &str) -> bool {
    s.starts_with(':')
        || s.starts_with('+')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with('/')
        || s.starts_with("~/")
}

/// True if RAW carries an explicit sigil / native shape / canonical URI that
/// `resolve` should handle (vs a bare word treated as text or a handle search).
pub fn is_explicit(raw: &str, reg: &Value) -> bool {
    if starts_core_or_native(raw)
        || raw.starts_with("goo://")
        || raw.starts_with("goo+")
        || has_scheme_sep(raw)
    {
        return true;
    }
    raw.chars().next().is_some_and(|c| sigil_expand(c, reg).is_some())
}

/// `<source>[:<input>][?<params>]` → `goo://<source>/<input>[?<params>]`.
fn source_uri(s: &str) -> String {
    let (body, params) = match s.split_once('?') {
        Some((b, p)) => (b, format!("?{p}")),
        None => (s, String::new()),
    };
    let (src, inp) = match body.split_once(':') {
        Some((a, b)) => (a, b),
        None => (body, ""),
    };
    format!("goo://{src}/{inp}{params}")
}

/// Reverse of `source_uri`: `source/input?params` → the legacy
/// `source:input?params` blob `resolve_source` parses.
fn source_args(r: &str) -> String {
    let (body, q) = match r.split_once('?') {
        Some((b, p)) => (b, format!("?{p}")),
        None => (r, String::new()),
    };
    let (a, p) = match body.split_once('/') {
        Some((a, p)) => (a, p),
        None => (body, ""),
    };
    if p.is_empty() {
        format!("{a}{q}")
    } else {
        format!("{a}:{p}{q}")
    }
}

/// Absolutize a path without resolving symlinks (logical, like bash's
/// `pwd`-based handling); expand a leading `~`.
fn abspath(p: &str) -> String {
    let expanded = if let Some(rest) = p.strip_prefix("~/") {
        format!("{}/{}", std::env::var("HOME").unwrap_or_default(), rest)
    } else {
        p.to_string()
    };
    let abs = if expanded.starts_with('/') {
        expanded
    } else {
        let pwd = std::env::var("PWD").unwrap_or_else(|_| ".".to_string());
        format!("{pwd}/{expanded}")
    };
    normalize(&abs)
}

/// Collapse `.` / `..` segments logically.
fn normalize(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

/// Rewrite a user-typed argument into a canonical goo URI.
pub fn canonicalize(raw: &str, reg: &Value) -> String {
    if raw.starts_with("goo://") || raw.starts_with("goo+") {
        return raw.to_string();
    }
    // Custom sigil expansion, unless a core/native/url shape we handle below.
    let mut raw = raw.to_string();
    if !(starts_core_or_native(&raw) || has_scheme_sep(&raw)) {
        if let Some(first) = raw.chars().next() {
            if let Some(exp) = sigil_expand(first, reg) {
                raw = format!("{exp}{}", &raw[first.len_utf8()..]);
            }
        }
    }

    if raw.starts_with("goo://") || raw.starts_with("goo+") {
        raw
    } else if let Some(rest) = raw.strip_prefix(':') {
        source_uri(rest)
    } else if let Some(rest) = raw.strip_prefix('+') {
        format!("goo+{rest}")
    } else if raw.starts_with("./") || raw.starts_with("../") || raw.starts_with('/') || raw.starts_with("~/") {
        format!("goo+file://{}", abspath(&raw))
    } else if has_scheme_sep(&raw) {
        format!("goo+{raw}")
    } else {
        format!("goo+text:{raw}")
    }
}

/// Resolve a canonical/sigil/native address to a subject. `verb` is currently
/// unused (reserved, like the shell) but kept for signature parity.
pub fn resolve(raw: &str, reg: &Value, _verb: Option<&Value>) -> Result<Value, String> {
    let uri = canonicalize(raw, reg);
    if let Some(rest) = uri.strip_prefix("goo://") {
        resolve_source(&source_args(rest), reg)
    } else if let Some(rest) = uri.strip_prefix("goo+") {
        let (scheme, value) = rest.split_once(':').unwrap_or((rest, ""));
        resolve_scheme(scheme, value)
    } else {
        Err(format!("address_resolve: cannot canonicalize '{raw}'"))
    }
}

fn resolve_scheme(scheme: &str, value: &str) -> Result<Value, String> {
    match scheme {
        "text" => {
            let mt = mime::detect_content(value);
            Ok(json!({ "type": mt, "text": value }))
        }
        "clip" => {
            if !value.is_empty() {
                return Err(format!("address: named clipboard buffers ('^{value}') not yet supported"));
            }
            Ok(json!({ "type": "text/plain", "text": selection::clipboard() }))
        }
        "sel" | "selection" => Ok(json!({ "type": "text/plain", "text": selection::primary() })),
        "stdin" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).ok();
            Ok(json!({ "type": "text/plain", "text": s }))
        }
        "file" => {
            let path = value.strip_prefix("//").unwrap_or(value);
            if !std::path::Path::new(path).exists() {
                return Err(format!("address: no such file: {path}"));
            }
            let mt = mime::detect_path(path)?;
            let title = path.rsplit('/').next().unwrap_or(path);
            let text = if mt.starts_with("text/") || mt == "application/json" || mt == "application/xml" {
                std::fs::read_to_string(path).unwrap_or_default()
            } else {
                String::new()
            };
            Ok(json!({
                "type": mt, "text": text, "id": path, "title": title,
                "metadata": { "path": path }
            }))
        }
        // URL schemes (http/https/ftp/claude/…) and any unknown scheme: a URI
        // reference. .id carries the locator (the addressable-entity convention).
        _ => {
            let url = format!("{scheme}:{value}");
            Ok(json!({ "type": "text/x-uri", "text": url, "id": url }))
        }
    }
}

fn resolve_source(spec: &str, reg: &Value) -> Result<Value, String> {
    // Split off ?params.
    let (spec, params) = match spec.split_once('?') {
        Some((s, p)) => (s, params_to_pairs(p)),
        None => (spec, Vec::new()),
    };
    let (source_key, input) = match spec.split_once(':') {
        Some((a, b)) => (a, b),
        None => (spec, ""),
    };
    if source_key.is_empty() {
        return Err(format!("address: empty source in '{spec}'"));
    }

    let source = reg
        .get("sources")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter().find(|s| {
                s.get("name").and_then(Value::as_str) == Some(source_key)
                    || s.get("prefix").and_then(Value::as_str) == Some(source_key)
            })
        })
        .ok_or_else(|| format!("address: no source named or prefixed '{source_key}'"))?;

    let emits = source.get("emits").and_then(Value::as_str).unwrap_or("text/plain");
    let list_cmd = source
        .get("list_cmd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("address: source '{source_key}' has no list_cmd"))?;

    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(list_cmd)
        .output()
        .map_err(|e| format!("address: source '{source_key}' failed: {e}"))?;
    let raw_items = String::from_utf8_lossy(&out.stdout);
    let mut items: Vec<Value> = serde_json::from_str(raw_items.trim()).unwrap_or_default();
    if items.is_empty() {
        return Err(format!("address: source '{source_key}' produced no items"));
    }

    // ?params: keep items where every key=value matches (case-insensitive
    // substring) against the item's top-level field or its metadata field.
    if !params.is_empty() {
        items.retain(|it| {
            params.iter().all(|(k, v)| {
                let field = it
                    .get(k)
                    .or_else(|| it.get("metadata").and_then(|m| m.get(k)));
                field_to_string(field).to_lowercase().contains(&v.to_lowercase())
            })
        });
        if items.is_empty() {
            return Err(format!("address: no item in source '{source_key}' matches the given ?params"));
        }
    }

    let tagged = |it: &Value| -> Value {
        let mut o = it.clone();
        if let Some(m) = o.as_object_mut() {
            m.insert("type".into(), json!(emits));
        }
        o
    };

    if input.is_empty() {
        return Ok(tagged(&items[0]));
    }
    let q = input.to_lowercase();
    items
        .iter()
        .find(|it| {
            let id = it.get("id").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let title = it.get("title").and_then(Value::as_str).unwrap_or("").to_lowercase();
            id.contains(&q) || title.contains(&q)
        })
        .map(tagged)
        .ok_or_else(|| format!("address: no item matching '{input}' in source '{source_key}'"))
}

/// Parse `k=v&k2=v2` into pairs; strip `*` wildcards (substring match). Skips
/// fragments without `=`. Matches bash `_addr_params_to_json` (last wins is
/// irrelevant here since we keep all pairs and `all()` them).
fn params_to_pairs(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((k.to_string(), v.replace('*', "")))
        })
        .collect()
}

fn field_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reg_with_sigil(ch: &str, expands: &str) -> Value {
        json!({ "sigils": [{ "char": ch, "expands": expands }], "sources": [] })
    }

    fn things_reg() -> Value {
        json!({
            "sigils": [{ "char": "%", "expands": ":thing:" }, { "char": "^", "expands": "+clip:" }],
            "sources": [{
                "name": "things", "prefix": "thing", "emits": "application/vnd.test.thing",
                "list_cmd": "echo '[{\"id\":\"alpha\",\"title\":\"Alpha Thing\"},{\"id\":\"beta\",\"title\":\"Beta Thing\"}]'"
            }]
        })
    }

    // ---- canonicalize (mirror tests/address.bats) ----
    #[test]
    fn canon_source_to_goo_url() {
        assert_eq!(canonicalize(":app:firefox", &json!({})), "goo://app/firefox");
    }
    #[test]
    fn canon_source_no_input() {
        assert_eq!(canonicalize(":things", &json!({})), "goo://things/");
    }
    #[test]
    fn canon_embedded_colons() {
        assert_eq!(canonicalize(":ws:0:1", &json!({})), "goo://ws/0:1");
    }
    #[test]
    fn canon_params_ride_along() {
        assert_eq!(canonicalize(":things:thing?title=beta", &json!({})), "goo://things/thing?title=beta");
    }
    #[test]
    fn canon_custom_sigil() {
        assert_eq!(canonicalize("%alpha", &reg_with_sigil("%", ":thing:")), "goo://thing/alpha");
    }
    #[test]
    fn canon_undefined_at_is_text() {
        assert_eq!(canonicalize("@app:firefox", &json!({})), "goo+text:@app:firefox");
    }
    #[test]
    fn canon_plus_handoff() {
        assert_eq!(canonicalize("+file:a.md", &json!({})), "goo+file:a.md");
    }
    #[test]
    fn canon_native_url() {
        assert_eq!(canonicalize("https://example.com/x", &json!({})), "goo+https://example.com/x");
    }
    #[test]
    fn canon_absolute_path() {
        assert_eq!(canonicalize("/tmp/foo", &json!({})), "goo+file:///tmp/foo");
    }
    #[test]
    fn canon_bare_text() {
        assert_eq!(canonicalize("hello world", &json!({})), "goo+text:hello world");
    }
    #[test]
    fn canon_already_canonical() {
        assert_eq!(canonicalize("goo://app/firefox", &json!({})), "goo://app/firefox");
    }

    // ---- is_explicit ----
    #[test]
    fn explicit_recognizes_shapes() {
        let r = things_reg();
        for s in [":app:firefox", "+file:x", "./foo", "../foo", "/abs/foo", "~/foo",
                  "https://example.com", "goo://app/x", "goo+file:x", "%alpha"] {
            assert!(is_explicit(s, &r), "should be explicit: {s}");
        }
        for s in ["hello world", "docs/foo.md", "firefox", "@app:firefox"] {
            assert!(!is_explicit(s, &r), "should NOT be explicit: {s}");
        }
    }

    // ---- resolve: scheme handlers ----
    #[test]
    fn resolve_text_and_url() {
        let r = json!({});
        let t = resolve("just some words", &r, None).unwrap();
        assert_eq!(t["type"], "text/plain");
        assert_eq!(t["text"], "just some words");
        let u = resolve("https://example.com", &r, None).unwrap();
        assert_eq!(u["type"], "text/x-uri");
        assert_eq!(u["text"], "https://example.com");
        assert_eq!(u["id"], "https://example.com"); // .id = the locator
    }
    #[test]
    fn resolve_file_reads_contents_and_path() {
        let dir = std::env::temp_dir().join(format!("goo-addr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sample.txt");
        std::fs::write(&f, "file body here\n").unwrap();
        let p = f.to_str().unwrap();
        let s = resolve(p, &json!({}), None).unwrap();
        assert_eq!(s["text"], "file body here\n");
        assert_eq!(s["metadata"]["path"], p);
        assert!(s["type"].as_str().unwrap().starts_with("text/"));
        // missing file errors
        assert!(resolve(&format!("{p}.nope"), &json!({}), None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn resolve_named_clip_unsupported() {
        let e = resolve("^somebuffer", &things_reg(), None).unwrap_err();
        assert!(e.contains("not yet supported"));
    }

    // ---- resolve: source handler ----
    #[test]
    fn resolve_source_by_id_title_prefix_first() {
        let r = things_reg();
        assert_eq!(resolve(":things:alpha", &r, None).unwrap()["id"], "alpha");
        assert_eq!(resolve(":things:beta thing", &r, None).unwrap()["id"], "beta");
        assert_eq!(resolve(":thing:alpha", &r, None).unwrap()["id"], "alpha"); // prefix
        assert_eq!(resolve(":things", &r, None).unwrap()["id"], "alpha"); // first
        assert_eq!(resolve(":things:alpha", &r, None).unwrap()["type"], "application/vnd.test.thing");
    }
    #[test]
    fn resolve_source_errors() {
        let r = things_reg();
        assert!(resolve(":things:zeta", &r, None).is_err());
        assert!(resolve(":nosuchsource:x", &r, None).unwrap_err().contains("no source"));
    }

    // ---- resolve: ?params ----
    #[test]
    fn resolve_params_filter() {
        let r = things_reg();
        assert!(resolve(":things:alpha?foo=bar", &r, None).is_err()); // unknown field excludes
        assert_eq!(resolve(":things?title=beta", &r, None).unwrap()["id"], "beta");
        assert_eq!(resolve(":things?title=*Alpha*", &r, None).unwrap()["id"], "alpha"); // * stripped
        assert_eq!(resolve(":things:thing?title=beta", &r, None).unwrap()["id"], "beta"); // combine
    }
}
