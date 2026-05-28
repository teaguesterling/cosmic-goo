//! Plugin discovery, parsing, and registry assembly — the Rust port of
//! `lib/plugin-loader.sh`.
//!
//! The registry is kept as a `serde_json::Value` (not lossy typed structs) so it
//! is byte-for-byte comparable with the bash engine's `registry.json` and
//! preserves every plugin-authored field, exactly like the jq passthrough in the
//! shell loader. Typed accessors live in the modules that consume it
//! (`address`, `verbs`, …).
//!
//! Parity-critical details mirrored from the shell:
//!   - search dirs, lowest→highest precedence (`plugin_dirs`)
//!   - discovery: `<dir>/*.toml` then `<dir>/*/plugin.toml`, each glob in
//!     alphabetical order (the shell glob is sorted)
//!   - per-item provenance fields `_plugin` / `_plugin_dir`
//!   - merge: override-by-`name` (sigils by `char`) keeping the *new* (later,
//!     higher-precedence) item and returning the array **sorted by key** — this
//!     matches jq's `unique_by`, which sorts. `dispatch` is *concatenated* in
//!     load order (first match wins), not keyed.

use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const COLLECTIONS_BY_NAME: &[&str] = &["types", "sources", "verbs", "adverbs", "aliases", "channels", "detectors", "checkers"];

/// Plugin search dirs, lowest → highest precedence (later wins on name clash).
pub fn dirs() -> Vec<PathBuf> {
    let builtin = std::env::var("COSMIC_GOO_BUILTIN_PLUGINS_DIR")
        .unwrap_or_else(|_| "/usr/share/cosmic-goo/plugins".to_string());
    let xdg = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.config")
    });
    let pwd = std::env::var("PWD").unwrap_or_else(|_| ".".to_string());
    vec![
        PathBuf::from(builtin),
        PathBuf::from("/etc/cosmic-goo/plugins"),
        PathBuf::from(format!("{xdg}/cosmic-goo/plugins")),
        PathBuf::from(format!("{pwd}/.cosmic-goo/plugins")),
    ]
}

/// Plugin TOML files in precedence order: per dir, `*.toml` (alphabetical) then
/// `*/plugin.toml` (alphabetical), mirroring the shell globs.
pub fn discover() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in dirs() {
        if !d.is_dir() {
            continue;
        }
        let mut single: Vec<PathBuf> = read_sorted(&d)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "toml"))
            .collect();
        out.append(&mut single);
        let mut nested: Vec<PathBuf> = read_sorted(&d)
            .into_iter()
            .filter(|p| p.is_dir())
            .map(|p| p.join("plugin.toml"))
            .filter(|p| p.is_file())
            .collect();
        out.append(&mut nested);
    }
    out
}

fn read_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort();
    entries
}

/// Parse one plugin TOML file into its contribution object (provenance added).
/// Returns None (with a warning) if the file can't be parsed, mirroring the
/// shell loader skipping a bad file.
pub fn load_one(file: &Path) -> Option<Value> {
    let content = std::fs::read_to_string(file).ok()?;
    let parsed: Value = match toml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("plugin_load: failed to parse {}: {e}", file.display());
            return None;
        }
    };
    // Use the file's parent as-is (absolutized if relative) — do NOT resolve
    // symlinks. The shell loader uses logical `pwd`, so a path under a symlinked
    // dir (e.g. ~/Projects -> /mnt/...) stays as given; matching that keeps the
    // registry byte-identical and resolves relative cmd paths the same way.
    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    let dir = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::path::absolute(parent).unwrap_or_else(|_| parent.to_path_buf())
    };
    Some(contrib(file, &dir, &parsed))
}

/// Build a single plugin's contribution from its parsed TOML — the port of
/// `plugin_load`'s jq transform.
pub fn contrib(file: &Path, dir: &Path, parsed: &Value) -> Value {
    let pname = parsed
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            file.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });
    let dir_str = dir.to_string_lossy().to_string();
    let file_str = file.to_string_lossy().to_string();

    let with_provenance = |key: &str| -> Value {
        let items = parsed.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        Value::Array(
            items
                .into_iter()
                .map(|mut it| {
                    if let Some(obj) = it.as_object_mut() {
                        obj.insert("_plugin".into(), json!(pname));
                        obj.insert("_plugin_dir".into(), json!(dir_str));
                    }
                    it
                })
                .collect(),
        )
    };

    json!({
        "plugins": [{
            "name": pname,
            "dir": dir_str,
            "file": file_str,
            "description": parsed.get("description").cloned().unwrap_or(Value::Null),
            "tier": parsed.get("tier").cloned().unwrap_or(Value::Null),
        }],
        "types":    with_provenance("types"),
        "sources":  with_provenance("sources"),
        "verbs":    with_provenance("verbs"),
        "adverbs":  with_provenance("adverbs"),
        "sigils":   with_provenance("sigils"),
        "aliases":  with_provenance("aliases"),
        "channels": with_provenance("channels"),
        "detectors": with_provenance("detectors"),
        "checkers": with_provenance("checkers"),
        "dispatch": with_provenance("dispatch"),
    })
}

fn empty_registry() -> Value {
    json!({
        "plugins": [], "types": [], "sources": [], "verbs": [],
        "adverbs": [], "sigils": [], "aliases": [], "channels": [],
        "detectors": [], "checkers": [], "dispatch": []
    })
}

/// override-by-`key`, keeping the *first* occurrence (so `new` before `reg` =
/// new wins) and returning the array sorted by key — matching jq `unique_by`.
fn override_by(new: &Value, reg: &Value, key: &str) -> Value {
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    for arr in [new, reg] {
        if let Some(items) = arr.as_array() {
            for it in items {
                let k = it.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                map.entry(k).or_insert_with(|| it.clone());
            }
        }
    }
    Value::Array(map.into_values().collect())
}

/// Merge a plugin contribution into a running registry. Later (= `new`) wins on
/// `name` (`char` for sigils). `dispatch` concatenates in load order.
pub fn merge(reg: &Value, new: &Value) -> Value {
    let mut out = Map::new();
    out.insert("plugins".into(), override_by(&new["plugins"], &reg["plugins"], "name"));
    for c in COLLECTIONS_BY_NAME {
        out.insert((*c).into(), override_by(&new[*c], &reg[*c], "name"));
    }
    out.insert("sigils".into(), override_by(&new["sigils"], &reg["sigils"], "char"));
    // Dispatch rules are ordered, not keyed: reg ++ new (load order).
    let mut dispatch = reg["dispatch"].as_array().cloned().unwrap_or_default();
    dispatch.extend(new["dispatch"].as_array().cloned().unwrap_or_default());
    out.insert("dispatch".into(), Value::Array(dispatch));
    Value::Object(out)
}

/// The embedded **core** plugin — declarations the engine guarantees are present
/// (the structural checkers the OS MIME DB can't provide; see detection.md).
/// Seeded before discovered plugins, so a real plugin overrides by name and tests
/// get it without re-declaring. Always present — independent of the discovery dir.
const CORE_TOML: &str = include_str!("../core.toml");

fn core_contribution() -> Value {
    let parsed: Value = toml::from_str(CORE_TOML).expect("embedded core.toml must parse");
    contrib(Path::new("<core>/core.toml"), Path::new("<core>"), &parsed)
}

/// Assemble the full registry: embedded core, the OS MIME DB (opt-in), then all
/// discovered plugins (later wins by name, so plugins override both). No cache.
pub fn load_all() -> Value {
    let mut reg = merge(&empty_registry(), &core_contribution());
    if let Some(osmime) = os_mime_contribution() {
        reg = merge(&reg, &osmime);
    }
    for file in discover() {
        if let Some(c) = load_one(&file) {
            reg = merge(&reg, &c);
        }
    }
    reg
}

// ---- OS MIME DB importer (shared-mime-info → [[types]]); see detection.md ----
//
// Opt-in via `COSMIC_GOO_MIME_DIRS` (colon-separated mime dirs). **Unset = no
// import** — so conformance is deterministic and the populated host /usr/share/mime
// never leaks into tests. Rust-only; bash is the frozen reference. Production sets
// the var (packaging); tests point it at a fixture.

/// The OS MIME contribution, or `None` when `COSMIC_GOO_MIME_DIRS` is unset/empty.
fn os_mime_contribution() -> Option<Value> {
    let dirs: Vec<PathBuf> = std::env::var("COSMIC_GOO_MIME_DIRS")
        .ok()?
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if dirs.is_empty() {
        return None;
    }
    Some(import_mime_dirs(&dirs))
}

/// Parse shared-mime-info `globs2` + `subclasses` from `dirs` into a `[[types]]`
/// contribution (extensions + `is_a`, `_source`-tagged) — the same shape a plugin
/// emits, so `merge` and the declarative `is_subtype` consume it unchanged. A
/// missing dir/file is skipped (off-desktop yields nothing).
pub fn import_mime_dirs(dirs: &[PathBuf]) -> Value {
    // name -> (extensions, is_a supertypes)
    let mut types: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for d in dirs {
        if let Ok(t) = std::fs::read_to_string(d.join("globs2")) {
            for (ty, ext) in parse_globs2(&t) {
                types.entry(ty).or_default().0.insert(ext);
            }
        }
        if let Ok(t) = std::fs::read_to_string(d.join("subclasses")) {
            for (sub, sup) in parse_subclasses(&t) {
                types.entry(sub).or_default().1.insert(sup);
            }
        }
    }
    let arr: Vec<Value> = types
        .into_iter()
        .map(|(name, (exts, isa))| {
            let mut o = Map::new();
            o.insert("name".into(), json!(name));
            if !exts.is_empty() {
                o.insert("extensions".into(), json!(exts.into_iter().collect::<Vec<_>>()));
            }
            if !isa.is_empty() {
                o.insert("is_a".into(), json!(isa.into_iter().collect::<Vec<_>>()));
            }
            o.insert("_source".into(), json!("shared-mime-info"));
            Value::Object(o)
        })
        .collect();
    json!({ "types": arr })
}

/// `globs2` lines (`priority:type:glob[:flags]`) → `(type, ".ext")`, taking only
/// clean single extensions; multi-dot / glob-meta / bare names are skipped.
fn parse_globs2(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut f = line.splitn(3, ':');
        let (_prio, Some(ty), Some(rest)) = (f.next(), f.next(), f.next()) else { continue };
        let glob = rest.split(':').next().unwrap_or(rest); // drop any trailing :flags
        if let Some(ext) = glob.strip_prefix("*.") {
            if !ext.is_empty()
                && ext.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
            {
                out.push((ty.to_string(), format!(".{ext}")));
            }
        }
    }
    out
}

/// The declared type whose `extensions` contains `ext` (with the dot, e.g.
/// `".json"`). On ambiguity (several types claim the extension — `globs2` maps
/// `.json` to both `application/json` and `application/schema+json`) prefer the
/// **shorter type name** (length, then lexicographic): shorter ≈ more general, the
/// right default until a glob-priority tie-break lands. See detection.md (slice 4).
pub fn type_for_extension<'a>(reg: &'a Value, ext: &str) -> Option<&'a str> {
    reg.get("types")?
        .as_array()?
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?;
            let exts = t.get("extensions")?.as_array()?;
            exts.iter().any(|e| e.as_str() == Some(ext)).then_some(name)
        })
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
}

/// `subclasses` lines (`subtype supertype`) → `(sub, super)`; skip blank/`#`.
fn parse_subclasses(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

/// Build a registry from a single in-memory plugin TOML, mirroring the real
/// discover→parse→merge path. Test-only helper shared across module test suites
/// (e.g. the `tests/verbs.bats` `fixture.toml`).
#[cfg(test)]
pub(crate) fn from_fixture_toml(name: &str, src: &str) -> Value {
    let parsed: Value = toml::from_str(src).unwrap();
    let c = contrib(Path::new(&format!("/p/{name}.toml")), Path::new("/p"), &parsed);
    merge(&empty_registry(), &c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn contrib_of(name: &str, toml_src: &str) -> Value {
        let parsed: Value = toml::from_str(toml_src).unwrap();
        contrib(Path::new(&format!("/p/{name}.toml")), Path::new("/p"), &parsed)
    }

    #[test]
    fn provenance_added_to_items() {
        let c = contrib_of("p", "name=\"p\"\n[[verbs]]\nname=\"go\"\naccepts=[\"text/*\"]\n");
        let v = &c["verbs"][0];
        assert_eq!(v["_plugin"], json!("p"));
        assert_eq!(v["_plugin_dir"], json!("/p"));
        assert_eq!(c["plugins"][0]["name"], json!("p"));
    }

    // Parity guard for the negotiation [[channels]] collection — same fixture +
    // assertions as tests/plugin-loader.bats `plugin_load passes [[channels]]
    // through with provenance` (the bash loader must produce the same shape).
    #[test]
    fn channels_pass_through_with_provenance() {
        let c = contrib_of(
            "chtest",
            "name=\"chtest\"\n[[channels]]\nname=\"chafa\"\naccepts=[\"image/*\"]\nemits=\"text/x-ansi\"\ncost=\"lossy\"\ncmd=\"chafa {in.path|q}\"\n",
        );
        let reg = merge(&empty_registry(), &c);
        let ch = reg["channels"].as_array().unwrap();
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0]["name"], json!("chafa"));
        assert_eq!(ch[0]["emits"], json!("text/x-ansi"));
        assert_eq!(ch[0]["_plugin"], json!("chtest"));
    }

    // Parity guard for the [[detectors]]/[[checkers]] collections — same fixture +
    // assertions as tests/plugin-loader.bats `plugin_load passes
    // [[detectors]]/[[checkers]] through with provenance`.
    #[test]
    fn detectors_and_checkers_pass_through_with_provenance() {
        let c = contrib_of(
            "dtest",
            "name=\"dtest\"\n\
             [[detectors]]\nname=\"libmagic\"\ncmd=\"file --mime-type -b\"\n\
             [[checkers]]\nname=\"json\"\ntarget=\"application/json\"\ncmd=\"jq -e .\"\n",
        );
        let reg = merge(&empty_registry(), &c);
        let d = reg["detectors"].as_array().unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0]["name"], json!("libmagic"));
        assert_eq!(d[0]["_plugin"], json!("dtest"));
        let ch = reg["checkers"].as_array().unwrap();
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0]["name"], json!("json"));
        assert_eq!(ch[0]["target"], json!("application/json"));
        assert_eq!(ch[0]["_plugin"], json!("dtest"));
    }

    // The embedded core.toml is seeded into every load_all() registry — so the
    // json checker is always present (production AND tests), independent of the
    // discovery dir. (Behavior-preserving home for the old hardwired looks_like_json.)
    #[test]
    fn core_seed_carries_the_json_checker() {
        let reg = merge(&empty_registry(), &core_contribution());
        let ch = reg["checkers"].as_array().unwrap();
        assert!(
            ch.iter().any(|c| c["name"] == json!("json") && c["builtin"] == json!("json")),
            "core.toml must seed the json checker: {ch:?}"
        );
    }

    // ---- OS MIME DB importer ----
    fn mime_fixture() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/mime")
    }

    #[test]
    fn os_mime_import_parses_globs_and_subclasses() {
        let c = import_mime_dirs(&[mime_fixture()]);
        let types = c["types"].as_array().unwrap();
        let find = |n: &str| types.iter().find(|t| t["name"] == json!(n));
        // svg: clean extension + is_a, source-tagged
        let svg = find("image/svg+xml").expect("svg type imported");
        assert!(svg["extensions"].as_array().unwrap().contains(&json!(".svg")));
        assert!(svg["is_a"].as_array().unwrap().contains(&json!("application/xml")));
        assert_eq!(svg["_source"], json!("shared-mime-info"));
        // the chain link + a couple of globs
        assert!(find("application/xml").unwrap()["is_a"].as_array().unwrap().contains(&json!("text/plain")));
        assert!(find("application/json").unwrap()["extensions"].as_array().unwrap().contains(&json!(".json")));
        // SKIPPED: glob-meta (*.so.[0-9]*), bare name (credits), multi-dot (*.eps.gz)
        assert!(find("application/x-sharedlib").is_none(), "glob-meta must be skipped");
        assert!(find("text/x-credits").is_none(), "bare name must be skipped");
        assert!(find("image/x-gzeps").is_none(), "multi-dot must be skipped");
    }

    #[test]
    fn imported_is_a_feeds_is_subtype_transitively() {
        let reg = merge(&empty_registry(), &import_mime_dirs(&[mime_fixture()]));
        // the marquee payoff: svg IS text, transitively, via the imported chain
        assert!(crate::mime::is_subtype("image/svg+xml", "text/plain", &reg));
        assert!(crate::mime::is_subtype("text/csv", "text/plain", &reg));
    }

    #[test]
    fn os_mime_import_yields_nothing_when_dir_absent() {
        let c = import_mime_dirs(&[std::path::PathBuf::from("/nonexistent/mime/xyz")]);
        assert!(c["types"].as_array().unwrap().is_empty());
    }

    #[test]
    fn type_for_extension_finds_and_disambiguates() {
        let reg = json!({ "types": [
            { "name": "application/json", "extensions": [".json"] },
            { "name": "application/schema+json", "extensions": [".json"] },
            { "name": "text/csv", "extensions": [".csv"] },
        ]});
        // unambiguous
        assert_eq!(type_for_extension(&reg, ".csv"), Some("text/csv"));
        // ambiguous .json → shorter name wins (application/json, not schema+json)
        assert_eq!(type_for_extension(&reg, ".json"), Some("application/json"));
        // no match / no types
        assert_eq!(type_for_extension(&reg, ".nope"), None);
        assert_eq!(type_for_extension(&json!({}), ".csv"), None);
    }

    #[test]
    fn name_falls_back_to_basename() {
        let parsed: Value = toml::from_str("[[verbs]]\nname=\"go\"\n").unwrap();
        let c = contrib(Path::new("/p/myplugin.toml"), Path::new("/p"), &parsed);
        assert_eq!(c["plugins"][0]["name"], json!("myplugin"));
        assert_eq!(c["verbs"][0]["_plugin"], json!("myplugin"));
    }

    #[test]
    fn merge_later_wins_and_sorts_by_name() {
        let a = contrib_of("a", "name=\"a\"\n[[verbs]]\nname=\"zeta\"\ncmd=\"old\"\n");
        let b = contrib_of("b", "name=\"b\"\n[[verbs]]\nname=\"alpha\"\ncmd=\"x\"\n[[verbs]]\nname=\"zeta\"\ncmd=\"new\"\n");
        // reg=a, then merge b (later → wins on zeta).
        let reg = merge(&merge(&empty_registry(), &a), &b);
        let verbs = reg["verbs"].as_array().unwrap();
        // sorted by name: alpha, zeta
        assert_eq!(verbs[0]["name"], json!("alpha"));
        assert_eq!(verbs[1]["name"], json!("zeta"));
        assert_eq!(verbs[1]["cmd"], json!("new")); // later plugin won
    }

    #[test]
    fn sigils_keyed_by_char() {
        let a = contrib_of("a", "name=\"a\"\n[[sigils]]\nchar=\"^\"\nexpands=\"+clip:\"\n");
        let b = contrib_of("b", "name=\"b\"\n[[sigils]]\nchar=\"^\"\nexpands=\"+other:\"\n");
        let reg = merge(&merge(&empty_registry(), &a), &b);
        let sigils = reg["sigils"].as_array().unwrap();
        assert_eq!(sigils.len(), 1);
        assert_eq!(sigils[0]["expands"], json!("+other:")); // later wins by char
    }

    #[test]
    fn dispatch_concatenates_in_load_order() {
        let a = contrib_of("a", "name=\"a\"\n[[dispatch]]\nmatches=\"X\"\nverb=\"v1\"\n");
        let b = contrib_of("b", "name=\"b\"\n[[dispatch]]\nmatches=\"Y\"\nverb=\"v2\"\n");
        let reg = merge(&merge(&empty_registry(), &a), &b);
        let d = reg["dispatch"].as_array().unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0]["verb"], json!("v1")); // reg first
        assert_eq!(d[1]["verb"], json!("v2")); // then new
    }
}
