//! `goo-compose` (v0) — a picker-driven sentence builder over the `goo` CLI.
//!
//! Walks subject → verb (type-filtered) → object → adverbs → confirm with a
//! dmenu-protocol picker, then **execs `goo <verb> <subject-addr> [object-addr]
//! [--k=v …]`** for the action — so composing runs the exact same path as
//! typing the command, and `goo` re-resolves the addresses (one canonical
//! execution path). Read-only data (candidates, applicable verbs, adverb
//! values, address resolution for the type) comes from `goo-engine` in-process.
//!
//! This is the interactive front-end the `goo` CLI deliberately is *not*: it
//! spawns a picker and touches the clipboard/selection (via `wl-paste`), so it
//! is Wayland-coupled in a way `goo` isn't. v1 swaps the dmenu picker for a
//! native libcosmic/iced GUI, keeping this engine-data + exec-`goo` backend.
//!
//! The `goo` binary is found via `$GOO_BIN` (else `goo` on PATH) — the same
//! override the bats suite uses, which also makes scripted testing trivial.

mod dmenu;

use goo_engine::{address, registry, selection, verbs};
use serde_json::{json, Value};
use std::process::Command;

fn main() {
    std::process::exit(run());
}

fn cancel() -> i32 {
    eprintln!("goo-compose: cancelled");
    130
}

fn run() -> i32 {
    let reg = registry::load_all();

    // 1. Subject.
    let subj_cands = compose_subject_candidates(&reg);
    let subj_addr = match dmenu::pick("Subject", &subj_cands) {
        Some(line) => line.split('\t').next().unwrap_or("").to_string(),
        None => return cancel(),
    };
    let subject = match address::resolve(&subj_addr, &reg, None) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("goo-compose: could not resolve subject '{subj_addr}'");
            return 1;
        }
    };
    let subject_type = subject.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain").to_string();

    // 2. Verb (must accept the subject's type).
    let mut verb_names: Vec<String> = verbs::for_subject(&reg, &subject)
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    verb_names.sort();
    verb_names.dedup();
    if verb_names.is_empty() {
        eprintln!("goo-compose: no verbs accept type {subject_type}");
        return 1;
    }
    let verb_name = match dmenu::pick(&format!("Verb [{subject_type}]"), &verb_names.join("\n")) {
        Some(v) => v,
        None => return cancel(),
    };
    let verb = match verbs::lookup(&reg, &verb_name, None) {
        Some(v) => v,
        None => {
            eprintln!("goo-compose: unknown verb '{verb_name}'");
            return 1;
        }
    };

    // 3. Object, if the verb takes one (picked from the addressable candidates).
    let mut object_addr: Option<String> = None;
    if verb.get("object_type").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).is_some() {
        let obj_cands: String = subj_cands
            .lines()
            .filter(|l| l.contains(':'))
            .collect::<Vec<_>>()
            .join("\n");
        let obj_type = verb.get("object_type").and_then(|t| t.as_str()).unwrap_or("");
        let oa = match dmenu::pick(&format!("Object [{obj_type}]"), &obj_cands) {
            Some(line) => line.split('\t').next().unwrap_or("").to_string(),
            None => return cancel(),
        };
        if address::resolve(&oa, &reg, None).is_err() {
            eprintln!("goo-compose: could not resolve object '{oa}'");
            return 1;
        }
        object_addr = Some(oa);
    }

    // 4. Adverbs the verb opts into.
    let mut adverbs: Vec<(String, String)> = Vec::new();
    if let Some(uses) = verb.get("uses_adverbs").and_then(|u| u.as_array()) {
        for aname_v in uses {
            let aname = match aname_v.as_str() {
                Some(a) => a,
                None => continue,
            };
            let adverb = reg
                .get("adverbs")
                .and_then(|a| a.as_array())
                .and_then(|arr| arr.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(aname)));
            let adverb = match adverb {
                Some(a) => a,
                None => continue,
            };
            let kind = adverb.get("kind").and_then(|k| k.as_str()).unwrap_or("selector");
            let value = if kind == "selector" {
                let vals: Vec<String> = adverb
                    .get("values")
                    .and_then(|v| v.as_object())
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                dmenu::pick(&format!("--{aname}"), &vals.join("\n"))
            } else {
                dmenu::pick(&format!("--{aname} (type a value)"), "")
            };
            if let Some(v) = value {
                if !v.is_empty() {
                    adverbs.push((aname.to_string(), v));
                }
            }
        }
    }

    // 5. Preview + confirm.
    let mut preview = format!("goo {verb_name} {subj_addr}");
    if let Some(oa) = &object_addr {
        preview += &format!(" {oa}");
    }
    for (k, v) in &adverbs {
        preview += &format!(" --{k}={v}");
    }
    if !dmenu::confirm(&format!("Run: {preview} ?")) {
        return cancel();
    }

    // 6. Execute through the goo CLI (re-resolves the addresses).
    let goo = std::env::var("GOO_BIN").unwrap_or_else(|_| "goo".to_string());
    let mut cmd = Command::new(&goo);
    cmd.arg(&verb_name).arg(&subj_addr);
    if let Some(oa) = &object_addr {
        cmd.arg(oa);
    }
    for (k, v) in &adverbs {
        cmd.arg(format!("--{k}={v}"));
    }
    match cmd.status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("goo-compose: failed to exec {goo}: {e}");
            1
        }
    }
}

/// Emit subject candidates as `address<TAB>label` lines (implicit selection /
/// clipboard first, then items from enumerable prefixed sources). Touches the
/// clipboard via `goo-engine::selection` (Wayland-coupled — see module docs).
fn compose_subject_candidates(reg: &Value) -> String {
    let mut out = String::new();
    let trunc = |s: String| s.chars().take(60).collect::<String>();
    let sel = trunc(selection::primary());
    let clip = trunc(selection::clipboard());
    if !sel.is_empty() {
        out += &format!("goo://sel/\tselection: {sel}\n");
    }
    if !clip.is_empty() {
        out += &format!("goo://clip/\tclipboard: {clip}\n");
    }
    if let Some(sources) = reg.get("sources").and_then(|s| s.as_array()) {
        for source in sources {
            if source.get("enumerate") == Some(&json!(false)) {
                continue;
            }
            let name = source.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name == "selection" || name == "clipboard" {
                continue;
            }
            let prefix = match source.get("prefix").and_then(|p| p.as_str()).filter(|s| !s.is_empty()) {
                Some(p) => p,
                None => continue,
            };
            let lc = match source.get("list_cmd").and_then(|c| c.as_str()).filter(|s| !s.is_empty()) {
                Some(l) => l,
                None => continue,
            };
            let items: Vec<Value> = serde_json::from_str(bash_capture(lc).trim()).unwrap_or_default();
            for it in items {
                let id = it.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let title = it.get("title").and_then(|t| t.as_str()).unwrap_or(id);
                out += &format!("goo://{prefix}/{id}\t{title} ({name})\n");
            }
        }
    }
    out
}

/// `bash -c <cmd>` capturing stdout (a source's `list_cmd`).
fn bash_capture(cmd: &str) -> String {
    Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}
