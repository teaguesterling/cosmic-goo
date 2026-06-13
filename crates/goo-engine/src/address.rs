//! Subject addressing — the goo:// domain model (Rust port of `lib/address.sh`).
//!
//! One canonical form: `goo://<domain>/<path>[;q=<query>][?<refine>]`.
//!   - a **value** is `goo://<domain>/<path>` — `<path>` is an **exact** locator
//!     (a source item's exact `id`, a file path, a literal text, a URL).
//!   - a **search** is `goo://<domain>/;q=<query>` — a **fuzzy** query over a
//!     source's `list_cmd` output (id/title substring).
//! Resolution is **strict**: the syntax says which you mean, no fuzzy fallback.
//!
//! Built-in **value domains** resolved here: `text` / `file` / `clip` / `sel` /
//! `stdin` / `url` / `type` (virtual-type assertion, sigil `=`). Every other
//! domain is a registry **source** (`[[sources]]`, matched by `name` *or*
//! `prefix`) — value = exact id, search = fuzzy.
//!
//! Human **sigils** (terminal shorthand; machines emit canonical `goo://`):
//!   bare / `./ ~/ / scheme://` → infer (text/file/url) · `+x` → text ·
//!   `:dom/path` → value · `:dom:query` → search · `^`/`^name` → clip ·
//!   `=<mime>` → virtual-type subject (shipped via `core.toml`; user-overridable) ·
//!   any other first char → user `[[sigils]]` alias.

use crate::{mime, registry, selection};
use serde_json::{json, Value};
use std::io::Read;
use std::path::Path;

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

/// `[A-Za-z]…://…` — the loose native-URL shape.
fn has_scheme_sep(s: &str) -> bool {
    s.contains("://") && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

/// Native file/url shapes that infer without a sigil.
fn starts_native(s: &str) -> bool {
    s.starts_with("./") || s.starts_with("../") || s.starts_with('/') || s.starts_with("~/")
}

/// A built-in sigil prefix (`: + ^`) or `goo://`.
fn starts_builtin(s: &str) -> bool {
    s.starts_with("goo://") || s.starts_with(':') || s.starts_with('+') || s.starts_with('^')
}

/// **Prefix-shape inference** (data-entry-ux.md §3.1, roadmap slice #4).
/// A bare input of the shape `<prefix>/<rest>` where `<prefix>` matches a
/// registered source's `prefix` field — treated as if the user had typed
/// `:<prefix>/<rest>`. Examples (given `apps` source with `prefix = "app"`):
///   `app/firefox`  → `(Some("app"), Some("firefox"))`  → routes to `goo://app/firefox`
///   `app/`         → `(Some("app"), Some(""))`         → source default
///   `usr/foo`      → `None`                             → no source `usr`, falls through
///   `hello world`  → `None`                             → no `/`, bare text
///   `app` (alone)  → `None`                             → no `/`, slice-#7 territory
///
/// **The `/` is required.** Bare `app` (no slash) does NOT route to the apps
/// source — `:app` would (via `colon_sigil`), but the bare form belongs to
/// entity-name inference (slice #7). The slash is the disambiguator that
/// says "I mean a domain reference, not a free word." A future slice #7
/// will resolve `firefox` to `:app/firefox` via scoring; that's *additive*
/// to this slice, not a replacement.
///
/// Deterministic — no source enumeration, no scoring, no fuzzy matching: just
/// a string split + registry lookup. **Cost is O(n_sources)** per call
/// (linear scan of the sources array). Fine at current scale (~20 sources);
/// if a profile ever shows it, build a `HashSet<&str>` of prefixes once per
/// dispatch and pass it through.
fn match_source_prefix<'a>(raw: &'a str, reg: &Value) -> Option<(&'a str, &'a str)> {
    let (prefix, rest) = raw.split_once('/')?;
    if prefix.is_empty() {
        return None; // leading slash is a native path, handled by starts_native
    }
    let sources = reg.get("sources")?.as_array()?;
    let known = sources.iter().any(|s| s.get("prefix").and_then(Value::as_str) == Some(prefix));
    if known {
        Some((prefix, rest))
    } else {
        None
    }
}

/// True if RAW carries an explicit sigil / native shape / canonical URI (vs a
/// bare word, which routes through the bin's subject inference). `+foo` (force
/// text) and `^` (clip) are explicit; a bare `foo` is not. **`app/firefox` IS**
/// explicit when `app` is a known source prefix (prefix-shape inference, §3.1)
/// — that's what makes GOO default-verb dispatch fire for the inferred subject.
pub fn is_explicit(raw: &str, reg: &Value) -> bool {
    starts_builtin(raw)
        || starts_native(raw)
        || has_scheme_sep(raw)
        || raw.chars().next().is_some_and(|c| sigil_expand(c, reg).is_some())
        || match_source_prefix(raw, reg).is_some()
}

/// Absolutize a path without resolving symlinks; expand a leading `~`.
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

/// Expand a `:`-sigil tail into canonical: the first `/` (→ value path) or `:`
/// (→ `;q=` search) after the domain decides. `:dom` alone → the domain default.
fn colon_sigil(rest: &str) -> String {
    // Split off `?refine` first so it isn't swallowed by the domain/path split.
    let (rest, refine) = match rest.split_once('?') {
        Some((r, q)) => (r, format!("?{q}")),
        None => (rest, String::new()),
    };
    let slash = rest.find('/');
    let colon = rest.find(':');
    match (slash, colon) {
        (Some(s), Some(c)) if s < c => format!("goo://{}/{}{refine}", &rest[..s], &rest[s + 1..]),
        (Some(_), Some(c)) => format!("goo://{}/;q={}{refine}", &rest[..c], &rest[c + 1..]),
        (Some(s), None) => format!("goo://{}/{}{refine}", &rest[..s], &rest[s + 1..]),
        (None, Some(c)) => format!("goo://{}/;q={}{refine}", &rest[..c], &rest[c + 1..]),
        (None, None) => format!("goo://{rest}/{refine}"),
    }
}

/// Rewrite a user-typed argument into a canonical `goo://` URI.
pub fn canonicalize(raw: &str, reg: &Value) -> String {
    if let Some(rest) = raw.strip_prefix("goo://") {
        return format!("goo://{rest}");
    }
    // Custom sigil expansion (unless already a built-in/native shape), then
    // re-canonicalize the expansion (it may itself be `:dom:…` / `+…` / goo://).
    if !(starts_builtin(raw) || starts_native(raw) || has_scheme_sep(raw)) {
        if let Some(first) = raw.chars().next() {
            if let Some(exp) = sigil_expand(first, reg) {
                return canonicalize(&format!("{exp}{}", &raw[first.len_utf8()..]), reg);
            }
        }
    }

    if raw == "^" {
        "goo://clip/".to_string()
    } else if let Some(name) = raw.strip_prefix('^') {
        format!("goo://clip/{name}")
    } else if let Some(text) = raw.strip_prefix('+') {
        format!("goo://text/{text}")
    } else if let Some(rest) = raw.strip_prefix(':') {
        colon_sigil(rest)
    } else if starts_native(raw) {
        format!("goo://file/{raw}")
    } else if has_scheme_sep(raw) {
        format!("goo://url/{raw}")
    } else if let Some((prefix, rest)) = match_source_prefix(raw, reg) {
        // Prefix-shape inference (§3.1): same canonical form `:prefix/rest` would
        // produce. Only fires when the prefix is a registered source — bare text
        // that happens to contain a `/` (e.g. `path/to/file` with no `tmp` source)
        // falls through to the text fallback below.
        format!("goo://{prefix}/{rest}")
    } else {
        format!("goo://text/{raw}")
    }
}

/// Resolve a canonical/sigil/native address to a subject. `verb` is reserved.
pub fn resolve(raw: &str, reg: &Value, _verb: Option<&Value>) -> Result<Value, String> {
    let uri = canonicalize(raw, reg);
    let rest = uri
        .strip_prefix("goo://")
        .ok_or_else(|| format!("cannot canonicalize '{raw}'"))?;
    let (domain, after) = rest.split_once('/').unwrap_or((rest, ""));
    if domain.is_empty() {
        return Err(format!("empty domain in '{uri}'"));
    }
    let (locator, refine) = match after.split_once('?') {
        Some((l, r)) => (l, params_to_pairs(r)),
        None => (after, Vec::new()),
    };
    let (is_search, q) = match locator.strip_prefix(";q=") {
        Some(query) => (true, query),
        None => (false, locator),
    };

    match domain {
        "text" => Ok(json!({ "type": mime::detect_content(q), "text": q })),
        "file" => resolve_file(q, Some(reg)),
        "clip" => {
            if !q.is_empty() {
                return Err(format!("named clipboard buffers ('^{q}') not yet supported"));
            }
            Ok(json!({ "type": "text/plain", "text": selection::clipboard() }))
        }
        "sel" | "selection" => Ok(json!({ "type": "text/plain", "text": selection::primary() })),
        // The current working directory as a subject — a contextual/virtual
        // subject like the selection or clipboard. Type `application/vnd.goo.cwd`
        // (declared in the working-directory plugin); the natural home for dynamic
        // `[[providers]]` (e.g. blq's per-project command registry). `:cwd` / `:wd`.
        "cwd" | "wd" | "working-directory" => {
            let p = std::env::current_dir()
                .ok()
                .map(|d| d.to_string_lossy().into_owned())
                .or_else(|| std::env::var("PWD").ok())
                .unwrap_or_default();
            let name = p.rsplit('/').find(|s| !s.is_empty()).unwrap_or("/").to_string();
            Ok(json!({
                "type": "application/vnd.goo.cwd",
                "id": p,
                "title": name,
                "metadata": { "path": p },
            }))
        }
        "stdin" => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).ok();
            Ok(json!({ "type": "text/plain", "text": s }))
        }
        "url" => Ok(json!({ "type": "text/x-uri", "text": q, "id": q })),
        // Virtual-type assertion (`goo://type/<mime>`, sigil `=<mime>`): a subject
        // with just `.type` set — used by `--explain` / `goo options` to preview a
        // plan or discovery surface for a hypothetical subject of that type. No
        // content, no id; the planner / OPTIONS only need the type.
        "type" => Ok(json!({ "type": q })),
        _ => resolve_source(domain, q, is_search, &refine, reg),
    }
}

/// Write `bytes` to a `{write}` destination — the output counterpart of [`resolve`].
/// The dest is canonicalized through the same addressing as a subject (so `^`→clip
/// and native paths→file come for free), then dispatched on the domain. v1: `file`
/// (write the path) and `clip` (the clipboard); other domains error cleanly. See
/// goo-protocol §12.
pub fn write_to(dest: &str, bytes: &[u8], reg: &Value) -> Result<(), String> {
    let uri = canonicalize(dest, reg);
    let rest = uri
        .strip_prefix("goo://")
        .ok_or_else(|| format!("invalid destination '{dest}'"))?;
    let (domain, path) = rest.split_once('/').unwrap_or((rest, ""));
    match domain {
        "file" => {
            if path.is_empty() {
                return Err("destination 'file' needs a path".into());
            }
            std::fs::write(abspath(path), bytes).map_err(|e| format!("write {path}: {e}"))
        }
        "clip" => {
            if !path.is_empty() {
                return Err(format!("named clipboard buffers ('^{path}') not yet supported"));
            }
            selection::set_clipboard(bytes)
        }
        other => Err(format!("destination not writable yet: goo://{other}/")),
    }
}

/// The last path segment's extension, lowercased, including the dot
/// (`data.tar.gz` → `.gz`, last only); dotfiles (`.bashrc`) and extensionless
/// names → `None`. See detection.md (slice 4).
fn path_extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None; // ".bashrc" (leading dot) or "foo." (trailing dot)
    }
    Some(name[dot..].to_ascii_lowercase())
}

/// Type of an existing absolute path + its provenance: a declared extension is
/// authoritative (`"extension"`); else libmagic (`"libmagic"`). The detection half
/// of [`resolve_file`], shared with `--explain`. See detection.md (slice 5).
fn mime_of_abs(abs: &str, reg: &Value) -> Result<(String, &'static str), String> {
    if let Some(ext) = path_extension(abs) {
        if let Some(t) = registry::type_for_extension(reg, &ext) {
            return Ok((t.to_string(), "extension"));
        }
    }
    Ok((mime::detect_path(abs)?, "libmagic"))
}

/// Type + provenance for a user-given file path (abspath'd, existence-checked) —
/// lets `--explain` type a file exactly as the run does. See detection.md (slice 5).
pub fn type_for_path(path: &str, reg: &Value) -> Result<(String, &'static str), String> {
    let abs = abspath(path);
    if !Path::new(&abs).exists() {
        return Err(format!("no such file: {abs}"));
    }
    mime_of_abs(&abs, reg)
}

fn resolve_file(path: &str, reg: Option<&Value>) -> Result<Value, String> {
    let abs = abspath(path);
    if !Path::new(&abs).exists() {
        return Err(format!("no such file: {abs}"));
    }
    // Extension signal (Rust-only enhancement): a declared extension is
    // authoritative and beats libmagic. Inert when `reg` is None or has no match,
    // so the None path is byte-identical to libmagic (== the bash reference).
    let mt = match reg {
        Some(r) => mime_of_abs(&abs, r)?.0,
        None => mime::detect_path(&abs)?,
    };
    let title = abs.rsplit('/').next().unwrap_or(&abs);
    let text = if mt.starts_with("text/") || mt == "application/json" || mt == "application/xml" {
        std::fs::read_to_string(&abs).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(json!({
        "type": mt, "text": text, "id": abs, "title": title,
        "metadata": { "path": abs }
    }))
}

/// A source domain (`[[sources]]`, matched by `name` or `prefix`). Value = exact
/// `id`; search = fuzzy id/title substring. Empty locator = the domain's first
/// item. `refine` (`?k=v`) filters by field-or-`.metadata` substring.
fn resolve_source(domain: &str, q: &str, is_search: bool, refine: &[(String, String)], reg: &Value) -> Result<Value, String> {
    let source = reg
        .get("sources")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter().find(|s| {
                s.get("name").and_then(Value::as_str) == Some(domain)
                    || s.get("prefix").and_then(Value::as_str) == Some(domain)
            })
        })
        .ok_or_else(|| format!("no domain or source named '{domain}'"))?;

    let emits = source.get("emits").and_then(Value::as_str).unwrap_or("text/plain");
    let list_cmd = source
        .get("list_cmd")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("domain '{domain}' has no list_cmd"))?;

    let out = std::process::Command::new("bash")
        .arg("-c")
        .arg(list_cmd)
        .output()
        .map_err(|e| format!("domain '{domain}' failed: {e}"))?;
    let mut items: Vec<Value> = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap_or_default();
    if items.is_empty() {
        return Err(format!("domain '{domain}' produced no items"));
    }

    if !refine.is_empty() {
        items.retain(|it| {
            refine.iter().all(|(k, v)| {
                let field = it.get(k).or_else(|| it.get("metadata").and_then(|m| m.get(k)));
                field_to_string(field).to_lowercase().contains(&v.to_lowercase())
            })
        });
        if items.is_empty() {
            return Err(format!("no item in '{domain}' matches the given ?refine"));
        }
    }

    let tagged = |it: &Value| -> Value {
        let mut o = it.clone();
        if let Some(m) = o.as_object_mut() {
            m.insert("type".into(), json!(emits));
        }
        o
    };

    if q.is_empty() {
        return Ok(tagged(&items[0]));
    }
    if is_search {
        // Fuzzy: id or title contains the query (case-insensitive).
        let needle = q.to_lowercase();
        items
            .iter()
            .find(|it| fuzzy_matches(it, &needle))
            .map(tagged)
            .ok_or_else(|| format!("no item matching '{q}' in '{domain}'"))
    } else {
        // Value: exact id.
        items
            .iter()
            .find(|it| it.get("id").and_then(Value::as_str) == Some(q))
            .map(tagged)
            .ok_or_else(|| format!("no item with id '{q}' in '{domain}'"))
    }
}

/// The shared fuzzy-search predicate: `item`'s `id` or `title` contains
/// `needle` (which must already be lowercased). Used by domain search here and
/// by the bin's bare-positional handle search, so the two can't drift.
pub fn fuzzy_matches(item: &Value, needle: &str) -> bool {
    let f = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or("").to_lowercase();
    f("id").contains(needle) || f("title").contains(needle)
}

/// Parse `k=v&k2=v2` into pairs; strip `*` wildcards (substring match).
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

    // ---- canonicalize ----
    #[test]
    fn canon_value_and_search_sigils() {
        let r = json!({});
        assert_eq!(canonicalize(":app/firefox", &r), "goo://app/firefox"); // value (/)
        assert_eq!(canonicalize(":app:firefox", &r), "goo://app/;q=firefox"); // search (:)
        assert_eq!(canonicalize(":things", &r), "goo://things/"); // domain default
        assert_eq!(canonicalize(":ws/0:1", &r), "goo://ws/0:1"); // value path keeps later ':'
    }
    #[test]
    fn canon_text_clip_native_url() {
        let r = json!({});
        assert_eq!(canonicalize("+FOO==", &r), "goo://text/FOO=="); // force literal text
        assert_eq!(canonicalize("hello world", &r), "goo://text/hello world"); // bare → text
        assert_eq!(canonicalize("^", &r), "goo://clip/");
        assert_eq!(canonicalize("^buf", &r), "goo://clip/buf");
        assert_eq!(canonicalize("/tmp/foo", &r), "goo://file//tmp/foo"); // native abs
        assert_eq!(canonicalize("~/x", &r), "goo://file/~/x");
        assert_eq!(canonicalize("https://example.com/x", &r), "goo://url/https://example.com/x");
    }
    #[test]
    fn canon_custom_sigil_and_passthrough() {
        assert_eq!(canonicalize("%alpha", &reg_with_sigil("%", ":thing:")), "goo://thing/;q=alpha");
        assert_eq!(canonicalize("goo://app/firefox", &json!({})), "goo://app/firefox");
        // undefined first char with no sigil → text
        assert_eq!(canonicalize("@app", &json!({})), "goo://text/@app");
    }

    // ---- is_explicit ----
    #[test]
    fn explicit_recognizes_shapes() {
        let r = things_reg();
        for s in [":app/x", ":app:x", "+x", "^", "^buf", "./f", "../f", "/abs", "~/f",
                  "https://x", "goo://app/x", "%alpha"] {
            assert!(is_explicit(s, &r), "should be explicit: {s}");
        }
        for s in ["hello world", "firefox", "docs/foo.md", "@app"] {
            assert!(!is_explicit(s, &r), "should NOT be explicit: {s}");
        }
    }

    // ---- resolve: value domains ----
    #[test]
    fn resolve_text_and_url() {
        let r = json!({});
        let t = resolve("just words", &r, None).unwrap();
        assert_eq!(t["type"], "text/plain");
        assert_eq!(t["text"], "just words");
        let u = resolve("https://example.com", &r, None).unwrap();
        assert_eq!(u["type"], "text/x-uri");
        assert_eq!(u["id"], "https://example.com");
        let plus = resolve("+./not-a-path", &r, None).unwrap(); // forced text, not a file
        assert_eq!(plus["type"], "text/plain");
        assert_eq!(plus["text"], "./not-a-path");
    }

    // The virtual-type value-domain — `goo://type/<mime>` and its `=<mime>` sigil
    // (shipped in `core.toml`). Resolves to a content-less subject `{type}`, used
    // by `--explain` / `goo options` to preview against a hypothetical subject.
    #[test]
    fn resolve_type_domain_and_equals_sigil() {
        // canonical URI: just `.type`, no `.text`/`.id` (it's a *virtual* subject).
        let canonical = resolve("goo://type/text/markdown", &json!({}), None).unwrap();
        assert_eq!(canonical["type"], "text/markdown");
        assert!(canonical.get("text").is_none(), "type-domain subject has no .text");
        assert!(canonical.get("id").is_none(), "type-domain subject has no .id");

        // `:type/<mime>` (the colon-sigil value form) reaches the same place.
        let colon = resolve(":type/image/png", &json!({}), None).unwrap();
        assert_eq!(colon["type"], "image/png");

        // `=<mime>` via the built-in sigil (declared in core.toml — feed it directly).
        let reg = json!({ "sigils": [{ "char": "=", "expands": "goo://type/" }] });
        assert_eq!(canonicalize("=text/csv", &reg), "goo://type/text/csv");
        let eq = resolve("=text/csv", &reg, None).unwrap();
        assert_eq!(eq["type"], "text/csv");
    }
    #[test]
    fn resolve_file_reads_contents_and_errors() {
        let dir = std::env::temp_dir().join(format!("goo-addr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sample.txt");
        std::fs::write(&f, "file body here\n").unwrap();
        let p = f.to_str().unwrap();
        let s = resolve(p, &json!({}), None).unwrap();
        assert_eq!(s["text"], "file body here\n");
        assert_eq!(s["metadata"]["path"], p);
        assert!(s["type"].as_str().unwrap().starts_with("text/"));
        assert!(resolve(&format!("{p}.nope"), &json!({}), None).unwrap_err().contains("no such file"));
        // :file/<path> value form too
        assert_eq!(resolve(&format!(":file/{p}"), &json!({}), None).unwrap()["text"], "file body here\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---- slice 4: the extension signal ----
    #[test]
    fn path_extension_extracts_last_lowercased() {
        assert_eq!(path_extension("a/b/data.JSON").as_deref(), Some(".json"));
        assert_eq!(path_extension("x.tar.gz").as_deref(), Some(".gz")); // last only
        assert_eq!(path_extension("/p/.bashrc"), None); // dotfile
        assert_eq!(path_extension("noext"), None);
        assert_eq!(path_extension("trailing."), None);
    }

    #[test]
    fn resolve_file_extension_beats_libmagic_none_path_is_libmagic() {
        let dir = std::env::temp_dir().join(format!("goo-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sample.goo");
        std::fs::write(&f, "plain words, not a known format\n").unwrap();
        let p = f.to_str().unwrap();
        // the conformance contract: the None path is byte-identical to libmagic.
        let libmagic = mime::detect_path(p).unwrap();
        assert_eq!(resolve_file(p, None).unwrap()["type"], json!(libmagic));
        // a declared `.goo` extension is authoritative — it beats libmagic.
        let reg = json!({ "types": [{ "name": "application/x-goo", "extensions": [".goo"] }] });
        assert_eq!(resolve_file(p, Some(&reg)).unwrap()["type"], json!("application/x-goo"));
        // Some(reg) with no matching extension still falls back to libmagic.
        assert_eq!(resolve_file(p, Some(&json!({}))).unwrap()["type"], json!(libmagic));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_to_file_and_unknown_domain() {
        let dir = std::env::temp_dir().join(format!("goo-wt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("out.txt");
        write_to(&format!("goo://file/{}", f.display()), b"hello dest", &json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "hello dest");
        // a non-writable domain errors cleanly (not a panic)
        assert!(write_to("goo://text/x", b"x", &json!({})).unwrap_err().contains("not writable"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn type_for_path_reports_source() {
        let dir = std::env::temp_dir().join(format!("goo-tfp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sample.goo");
        std::fs::write(&f, "words\n").unwrap();
        let p = f.to_str().unwrap();
        // no declared extension → libmagic source
        assert_eq!(type_for_path(p, &json!({})).unwrap().1, "libmagic");
        // declared extension → extension source (authoritative)
        let reg = json!({ "types": [{ "name": "application/x-goo", "extensions": [".goo"] }] });
        let (t, src) = type_for_path(p, &reg).unwrap();
        assert_eq!((t.as_str(), src), ("application/x-goo", "extension"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_named_clip_unsupported() {
        assert!(resolve("^somebuffer", &things_reg(), None).unwrap_err().contains("not yet supported"));
    }

    // ---- resolve: source domains (value=exact, search=fuzzy) ----
    #[test]
    fn resolve_source_value_is_exact() {
        let r = things_reg();
        assert_eq!(resolve(":things/alpha", &r, None).unwrap()["id"], "alpha"); // exact
        assert_eq!(resolve(":thing/beta", &r, None).unwrap()["id"], "beta"); // prefix as domain
        assert_eq!(resolve(":things", &r, None).unwrap()["id"], "alpha"); // default = first
        assert_eq!(resolve(":things/alpha", &r, None).unwrap()["type"], "application/vnd.test.thing");
        assert!(resolve(":things/alph", &r, None).is_err()); // not an exact id → no value
        assert!(resolve(":things/Alpha", &r, None).is_err()); // case-sensitive exact
    }
    #[test]
    fn resolve_source_search_is_fuzzy() {
        let r = things_reg();
        assert_eq!(resolve(":things:alph", &r, None).unwrap()["id"], "alpha"); // substring id
        assert_eq!(resolve(":things:beta thing", &r, None).unwrap()["id"], "beta"); // title (ci)
        assert_eq!(resolve("%alpha", &r, None).unwrap()["id"], "alpha"); // custom sigil → search
        assert!(resolve(":things:zeta", &r, None).is_err());
        assert!(resolve(":nosuch:x", &r, None).unwrap_err().contains("no domain or source"));
    }
    #[test]
    fn resolve_refine_filters() {
        let r = things_reg();
        // refine with no query (domain default + filter)
        assert_eq!(resolve(":things?title=beta", &r, None).unwrap()["id"], "beta");
        // refine on an explicit (empty) search
        assert_eq!(resolve(":things:?title=beta", &r, None).unwrap()["id"], "beta");
        assert_eq!(resolve(":things?title=*Alpha*", &r, None).unwrap()["id"], "alpha"); // * stripped
        // unknown field excludes
        assert!(resolve(":things/alpha?foo=bar", &r, None).is_err());
    }

    // ---- prefix-shape inference (§3.1, roadmap slice #4) ----
    //
    // Bare `<known-prefix>/<rest>` resolves as `:<prefix>/<rest>` would —
    // cheap, deterministic, no source enumeration. Same canonical form, same
    // resolution path, same downstream errors. The keystone: entity-name
    // inference (slice #7) builds on this with scoring / bands; this slice
    // is the cheap prequel.

    #[test]
    fn canon_prefix_shape_routes_bare_input_to_source() {
        let r = things_reg(); // sources: [{name: "things", prefix: "thing", …}]
        // Bare `thing/alpha` canonicalizes identically to `:thing/alpha`.
        assert_eq!(canonicalize("thing/alpha", &r), "goo://thing/alpha");
        assert_eq!(canonicalize(":thing/alpha", &r), "goo://thing/alpha");
        // Empty rest → source default (matches existing `:thing/` behavior).
        assert_eq!(canonicalize("thing/", &r), "goo://thing/");
    }

    #[test]
    fn canon_prefix_shape_falls_through_when_prefix_unknown() {
        let r = things_reg(); // no source with prefix `path`
        // `path/to/file` looks shape-y but `path` isn't a registered prefix.
        // Falls to text fallback. This is what protects accidental bare
        // multi-component text strings from getting hijacked.
        assert_eq!(canonicalize("path/to/file", &r), "goo://text/path/to/file");
        // Same input with empty registry — definitely text.
        assert_eq!(canonicalize("path/to/file", &json!({})), "goo://text/path/to/file");
    }

    #[test]
    fn canon_prefix_shape_does_not_steal_native_paths() {
        let r = things_reg();
        // Leading `./` `../` `/` `~/` all hit starts_native FIRST — file domain
        // wins, prefix inference never runs. (We deliberately don't want
        // `./thing/foo` to suddenly mean `:thing/foo`.)
        assert_eq!(canonicalize("/thing/foo", &r), "goo://file//thing/foo");
        assert_eq!(canonicalize("./thing/foo", &r), "goo://file/./thing/foo");
        assert_eq!(canonicalize("~/thing/foo", &r), "goo://file/~/thing/foo");
    }

    #[test]
    fn canon_prefix_shape_does_not_match_source_name_only_prefix() {
        // Source name is "things"; its prefix is "thing". Only the `prefix`
        // field is checked — using the full name as a bare prefix doesn't
        // count (matching the spec's "source-prefix" wording and the user
        // convention that `:` sigils use prefix not name).
        let r = things_reg();
        assert_eq!(canonicalize("thing/alpha", &r), "goo://thing/alpha");      // prefix → matched
        assert_eq!(canonicalize("things/alpha", &r), "goo://text/things/alpha"); // name → NOT matched
    }

    #[test]
    fn canon_prefix_shape_bare_word_without_slash_stays_text() {
        let r = things_reg();
        // `thing` alone (no `/`) is bare text — that's entity-name inference's
        // territory (slice #7), not §3.1's. Stays text/plain.
        assert_eq!(canonicalize("thing", &r), "goo://text/thing");
    }

    #[test]
    fn is_explicit_recognizes_prefix_shape_so_goo_default_fires() {
        // The whole point of teaching `is_explicit`: `goo app/firefox` should
        // route through GOO default-verb dispatch (cmd_goo) rather than verb
        // lookup (cmd_verb → "unknown verb"). This test locks that.
        let r = things_reg();
        assert!(is_explicit("thing/alpha", &r), "known prefix should be explicit");
        assert!(!is_explicit("unknown/alpha", &r), "unknown prefix should NOT be explicit");
        assert!(!is_explicit("hello", &r), "bare word should NOT be explicit");
        // Explicit forms remain explicit regardless of prefix-shape.
        assert!(is_explicit(":thing/alpha", &r));
        assert!(is_explicit("/tmp/foo", &r));
        assert!(is_explicit("goo://thing/alpha", &r));
    }

    #[test]
    fn resolve_prefix_shape_yields_same_subject_as_colon_form() {
        // End-to-end: resolving `thing/alpha` produces the same subject as
        // resolving `:thing/alpha`. Locks "prefix-shape inference is sugar,
        // not a parallel resolution path."
        let r = things_reg();
        let bare = resolve("thing/alpha", &r, None).unwrap();
        let colon = resolve(":thing/alpha", &r, None).unwrap();
        assert_eq!(bare, colon);
        assert_eq!(bare["id"], "alpha");
        assert_eq!(bare["type"], "application/vnd.test.thing");
    }
}
