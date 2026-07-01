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

/// The types a subject claims membership in: its `type` first, then any provenance
/// `_facets` (e.g. a file subject is also `inode/file` — see `address::resolve_file`).
/// Accept-matching scores a verb against the best over all of these, so a file gains
/// `inode/*` handle verbs (`open`/`reveal`/`copy-path`) while keeping its refined
/// content type. `type` stays the content type; facets are consulted *only* here, not
/// in templating or serialization, and are minted only by a resolver that knows the
/// subject came from disk (so clipboard `text/csv` never gains file verbs).
pub fn subject_types(subject: &Value) -> Vec<&str> {
    let mut out = Vec::new();
    if let Some(t) = subject.get("type").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        out.push(t);
    }
    if let Some(facets) = subject.get("_facets").and_then(Value::as_array) {
        out.extend(facets.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()));
    }
    out
}

/// True if `verb` accepts any of `subject`'s membership types ([`subject_types`]).
fn accepts_subject(verb: &Value, subject: &Value, reg: &Value) -> bool {
    subject_types(subject)
        .iter()
        .any(|t| accepts_type(verb, t, reg))
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

/// The most-specific impl of verb `name` for `subject` — scoring `accepts` over the
/// subject's full **membership** (its content `type` plus any provenance `_facets`),
/// the dispatch counterpart of [`for_subject`]'s per-name selection. `None` if the
/// subject is untyped or no impl of `name` accepts it (the caller then keeps its
/// by-name pick). This is what lets verb-first dispatch pick the impl that actually
/// accepts the *resolved* subject — `goo show :br/main` runs git's `show`, not the
/// first-registered `show` — instead of `lookup(name, None)`'s first impl. Honours
/// `valid_when` so the chosen impl matches what `for_subject` lists.
pub fn lookup_subject(reg: &Value, name: &str, subject: &Value) -> Option<Value> {
    let types = subject_types(subject);
    if types.is_empty() {
        return None;
    }
    let verbs = reg.get("verbs")?.as_array()?;
    let mut best: Option<(i32, &Value)> = None;
    for v in verbs {
        if v.get("name").and_then(Value::as_str) != Some(name) || !valid_for(v, subject) {
            continue;
        }
        if let Some(s) = types.iter().filter_map(|t| verb_specificity(v, t, reg)).max() {
            if best.is_none_or(|(b, _)| s >= b) {
                best = Some((s, v));
            }
        }
    }
    best.map(|(_, v)| v.clone())
}

/// True if any impl of the verb named `name` declares a non-empty `accepts` — i.e.
/// the name is subject-taking for at least one type, even when a subjectless
/// (empty-`accepts`) impl is registered first. Dispatch (`cmd_verb`) consults this to
/// decide whether a positional should be resolved as a subject (source/address)
/// rather than taken as literal text: a *mixed* family — a typed impl beside a
/// subjectless one, like `stop` (containers' `t/container` + media's empty
/// `playerctl stop`) — must resolve `:container/x` even if the empty impl sorts first,
/// so [`lookup_subject`] can then re-select the typed impl. A name whose impls are
/// *all* empty-`accepts` (a pure subjectless verb) returns `false`, preserving the
/// literal-text handling of `goo <verb> "some text"`.
pub fn name_accepts_any_type(reg: &Value, name: &str) -> bool {
    reg.get("verbs")
        .and_then(Value::as_array)
        .is_some_and(|verbs| {
            verbs.iter().any(|v| {
                v.get("name").and_then(Value::as_str) == Some(name)
                    && v.get("accepts")
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty())
            })
        })
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
    accepts_specificity(
        &verb
            .get("accepts")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default(),
        t,
        reg,
    )
}

/// Best specificity of type `t` against any of `patterns` — `None` if none
/// match, higher = more specific (exact > lattice > glob-by-prefix-length).
/// The same scoring `lookup`/`for_subject` rank with, exposed so subject-shape
/// completion (slice #5 / §5.1) can rank candidates by accepts-specificity
/// without re-implementing the lattice. `patterns` is a verb's `accepts` (or,
/// for a polymorphic verb, the UNION across its impls).
pub fn accepts_specificity(patterns: &[&str], t: &str, reg: &Value) -> Option<i32> {
    let mut best: Option<i32> = None;
    for p in patterns {
        if let Some(s) = pattern_specificity(t, p, reg) {
            if best.is_none_or(|b| s > b) {
                best = Some(s);
            }
        }
    }
    best
}

/// Like [`default_for`] but over a subject's full membership ([`subject_types`]):
/// the first membership type with a default verb wins. Content type is tried first,
/// then provenance facets — so a file with a content-type default keeps it, while a
/// `.pdf` (no content default) falls through to `open` via its `inode/file`
/// membership. This keeps the bare-address GOO path (`goo report.pdf`) in agreement
/// with listing and dispatch: a file's handle verb lists, runs, *and* is its default.
pub fn default_for_subject(reg: &Value, subject: &Value) -> Option<Value> {
    subject_types(subject).into_iter().find_map(|t| default_for(reg, t))
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

/// The specificity floor for a *specific* match — an exact type or an `is_a`/
/// lattice match. A match scoring below this came only from a broad `family/*`
/// or `*/*` glob. (Couples to the tiers in [`pattern_specificity`]: exact =
/// `i32::MAX`, lattice = `1_000_000`, glob = a small prefix length.)
const SPECIFIC_MATCH_FLOOR: i32 = 1_000_000;

/// The declared `kind` of a type (`handle`/`content`/`surface`/…) from the
/// registry `[[types]]`, if any.
pub fn type_kind(reg: &Value, t: &str) -> Option<String> {
    reg.get("types")?
        .as_array()?
        .iter()
        .find(|d| d.get("name").and_then(Value::as_str) == Some(t))
        .and_then(|d| d.get("kind").and_then(Value::as_str))
        .map(String::from)
}

/// The family ("type") part of a MIME — `application` of `application/vnd.x`.
fn mime_family(t: &str) -> &str {
    t.split('/').next().unwrap_or(t)
}

/// Whether `verb` reaches a `kind=handle` `stype` ONLY by *coincidental namespace
/// glob* — every accept that matches is a glob over the handle's OWN family
/// (`application/*` catching `application/vnd.cos-cli.app`) or the universal
/// `*/*`. Such a match is noise: the handle is an opaque reference, and a content
/// verb caught it only because their MIME families happen to share a prefix.
///
/// **Crucially keeps** a glob that reaches `stype` THROUGH an `is_a` supertype —
/// a *declared* content kinship: `mount is_a inode/directory` legitimately admits
/// an `inode/*` verb (open / reveal / copy-path), because the type author said
/// the handle IS a filesystem object. Also keeps any exact or `is_a`/lattice
/// (non-glob) match. `false` when the verb doesn't match at all.
pub fn glob_noise_for_handle(verb: &Value, stype: &str, reg: &Value) -> bool {
    let accepts: Vec<&str> = verb
        .get("accepts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let sfam = mime_family(stype);
    let mut matched = false;
    for p in &accepts {
        let Some(s) = pattern_specificity(stype, p, reg) else { continue };
        matched = true;
        if s >= SPECIFIC_MATCH_FLOOR {
            return false; // exact or is_a/lattice match → a real affordance
        }
        // A sub-floor (glob) match. Keep the verb if the glob reaches `stype`
        // via is_a — i.e. its family differs from the handle's own family and it
        // isn't the universal catch-all.
        if let Some(gfam) = p.strip_suffix("/*") {
            if gfam != "*" && gfam != sfam {
                return false; // matched through an is_a supertype → keep
            }
        }
        // else: same-family glob or `*/*` → coincidental; keep scanning.
    }
    matched // matched, and every match was coincidental namespace noise
}

/// Every verb applicable to `subject` — type-accepted *and* passing its
/// `valid_when`. With cross-plugin polymorphism (multiple verbs of the same name
/// with different `accepts`), this returns **one verb per name** — the
/// most-specific impl for the subject type. Order: registry order of the kept
/// verbs, so the picker sees the natural sequence.
///
/// A `kind=handle` subject additionally drops verbs that match it only by
/// coincidental namespace glob ([`glob_noise_for_handle`]) — `application/*`
/// catching `application/vnd.*` references — while keeping exact matches and
/// `is_a`-derived ones (so a `mount is_a inode/directory` still admits `inode/*`
/// file verbs).
pub fn for_subject(reg: &Value, subject: &Value) -> Vec<Value> {
    let stype = match subject.get("type").and_then(|t| t.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    let verbs = match reg.get("verbs").and_then(|v| v.as_array()) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let is_handle = type_kind(reg, stype).as_deref() == Some("handle");
    // Match against every type the subject claims (content `type` + any provenance
    // `_facets`, e.g. a file is also `inode/file`) — best specificity across them, so
    // a file lists both its content verbs and its `inode/*` handle verbs. `stype` (the
    // content type) still drives the handle-noise gate below.
    let match_types = subject_types(subject);
    // Walk in registry order; for each name, keep the best-specificity impl seen
    // (>= so later-registered wins ties, matching `lookup`'s tie-break).
    use std::collections::HashMap;
    let mut best: HashMap<String, (i32, usize)> = HashMap::new();
    for (i, v) in verbs.iter().enumerate() {
        if !valid_for(v, subject) {
            continue;
        }
        let Some(name) = v.get("name").and_then(Value::as_str) else { continue };
        if let Some(s) = match_types.iter().filter_map(|t| verb_specificity(v, t, reg)).max() {
            // Handle subjects: a coincidental same-namespace glob match is noise
            // (sha256 over a window), but an is_a-derived glob is real (open over
            // a mount). Skip only the former.
            if is_handle && glob_noise_for_handle(v, stype, reg) {
                continue;
            }
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
    let mut out: Vec<Value> = kept.into_iter().map(|(_, v)| v).collect();

    // Dynamic verbs contributed by `[[providers]]` (e.g. blq's per-cwd command
    // registry). Appended after the static verbs, which WIN on a name collision —
    // a provider can't shadow a built-in. The fast path returns empty unless a
    // provider's `for_type` matches this subject, so only subjects like `:cwd` pay.
    let have: std::collections::HashSet<String> = out
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str).map(String::from))
        .collect();
    for pv in provider_verbs_for(reg, subject) {
        match pv.get("name").and_then(Value::as_str) {
            Some(name) if !have.contains(name) => out.push(pv),
            _ => {} // collision with a static verb (or nameless) — static wins
        }
    }
    out
}

/// Dynamic verbs contributed by `[[providers]]` whose `for_type` matches the
/// subject's type. IMPURE: runs each matching provider's `list_cmd` to enumerate
/// `[{name, description}]`, synthesizing one verb per entry with the provider's
/// `run` template as its `cmd` (so `{verb.name}` resolves at render time, since
/// `build_context` puts the synthesized verb into the substitution context).
///
/// The `list_cmd` is subject-substituted before it runs (like `object_list_cmd`),
/// so a provider can enumerate verbs from the *specific* subject (e.g. a file's
/// MIME handlers) — not just ambient context (`:cwd`). Subject fields reach
/// `bash -c`, so untrusted ones must be `|q`-quoted in the template, the same
/// convention as every other subject-into-shell site.
///
/// Graceful by contract — this runs during verb *listing* (`what`/`do`/OPTIONS/
/// completion/compose). A provider whose `list_cmd` fails, whose tool is absent,
/// or that emits non-JSON yields no verbs; it must never break the listing. The
/// hot path costs nothing unless `reg["providers"]` is non-empty AND the subject
/// type matches a provider's `for_type` (gated by `is_subtype` before any exec).
/// A verb name must be a shell-neutral identifier. A verb name becomes a CLI
/// subcommand token AND can be interpolated into a verb's `cmd` (which runs via
/// `bash -c`). Constraining the charset means a name can never carry shell syntax
/// — so `{verb.name}` is injection-proof *by construction*, not by the author
/// remembering `|q`. Enforced for static names at `goo validate` and for dynamic
/// (provider-supplied, attacker-influenced) names at synthesis.
///
/// Rule: non-empty, starts alphanumeric, then alphanumerics or `-_.:/+` (covers
/// real command names — make targets, `npm`-style `build:prod`, etc. — while
/// excluding whitespace and every bash metacharacter).
pub fn is_valid_verb_name(name: &str) -> bool {
    let mut chars = name.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric()) {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '+'))
}

pub fn provider_verbs_for(reg: &Value, subject: &Value) -> Vec<Value> {
    let providers = match reg.get("providers").and_then(Value::as_array) {
        Some(p) if !p.is_empty() => p,
        _ => return Vec::new(), // fast path: no providers declared
    };
    // The subject must claim at least one type (content `type` and/or a membership
    // facet) for any provider to attach.
    if subject_types(subject).is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for prov in providers {
        let for_type = prov.get("for_type").and_then(Value::as_str).unwrap_or("");
        // Match the subject's content type OR any provenance membership (`_facets`),
        // so a `for_type = inode/file` provider attaches to every file (which is also
        // an inode/file) even though its `type` is the refined content type.
        let matches = !for_type.is_empty()
            && subject_types(subject).iter().any(|t| mime::is_subtype(t, for_type, reg));
        if !matches {
            continue;
        }
        let list_cmd = prov.get("list_cmd").and_then(Value::as_str).unwrap_or("");
        let run = prov.get("run").and_then(Value::as_str).unwrap_or("");
        if list_cmd.is_empty() || run.is_empty() {
            continue;
        }
        // Subject-substitute the list_cmd before running it (mirrors
        // `object_list_cmd`), so a per-subject provider can enumerate verbs from
        // the specific subject — e.g. `xdg-mime query filetype {subject.id|q}`.
        // Untrusted subject fields (a filename can carry shell metacharacters)
        // are the author's to neutralize with `|q`, exactly as everywhere else a
        // subject reaches `bash -c`; an ambient list_cmd with no `{subject.*}`
        // token renders unchanged, so existing `:cwd` providers are unaffected.
        let rendered_list = template::substitute(list_cmd, &json!({ "subject": subject }));
        // Enumerate verb stubs. Non-zero exit / non-JSON / non-array → no verbs.
        let stubs = match serde_json::from_str::<Value>(bash_stdout(&rendered_list).trim()) {
            Ok(Value::Array(a)) => a,
            _ => continue,
        };
        let confirm = prov.get("confirm").and_then(Value::as_bool).unwrap_or(false);
        let pname = prov.get("name").and_then(Value::as_str).unwrap_or("");
        for stub in stubs {
            let name = match stub.get("name").and_then(Value::as_str) {
                // Reject any name that isn't a shell-neutral identifier — an
                // attacker-supplied name like `a;rm -rf ~` never becomes a verb,
                // so it can't reach the bash-rendered cmd. Injection is impossible
                // here regardless of whether the `run` template uses |q.
                Some(n) if is_valid_verb_name(n) => n,
                _ => continue,
            };
            let desc = stub.get("description").and_then(Value::as_str).unwrap_or("");
            out.push(json!({
                "name": name,
                "description": desc,
                "accepts": [for_type],
                "cmd": run,
                "confirm": confirm,
                "dynamic": true,
                "provider": pname,
            }));
        }
    }
    out
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
    // 1. A subject membership type must match accepts (when the subject is typed).
    // `subject_types` includes provenance `_facets`, so e.g. `open` (accepts inode/*)
    // runs on a `text/csv` file via its `inode/file` membership without coercion.
    let stype = subject.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if !stype.is_empty() && !accepts_subject(verb, subject, reg) {
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
    // A dynamic (provider) verb's non-`name` fields — `description` above all — are
    // untrusted free text from the same project-local registry as the name. The
    // name is validated to a shell-neutral identifier; description is prose and
    // CANNOT be. So a dynamic verb exposes ONLY its validated `name` to the
    // template — never a field that could smuggle shell syntax into the cmd
    // (closing `{verb.description}` as an injection vector). Static verbs expose
    // all their author-trusted custom fields (e.g. {verb.fabric_pattern}) as before.
    let verb_ctx = if verb.get("dynamic").and_then(Value::as_bool).unwrap_or(false) {
        json!({ "name": verb.get("name").cloned().unwrap_or(Value::Null) })
    } else {
        verb.clone()
    };
    let mut context = json!({
        "subject": subject,
        "object": object,
        "verb": verb_ctx,
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

    // ---- accepts_specificity (slice #5 ranking SSOT) ----

    #[test]
    fn accepts_specificity_exact_beats_glob_beats_none() {
        let reg = json!({});
        let pats = ["inode/*", "text/x-uri"];
        // Exact match (text/x-uri == pattern) outranks a glob match (inode/*).
        let exact = accepts_specificity(&pats, "text/x-uri", &reg).unwrap();
        let glob = accepts_specificity(&pats, "inode/file", &reg).unwrap();
        assert!(exact > glob, "exact {exact} should beat glob {glob}");
        // A type no pattern admits → None (skip the source).
        assert!(accepts_specificity(&pats, "audio/mpeg", &reg).is_none());
    }

    #[test]
    fn accepts_specificity_longer_glob_prefix_is_more_specific() {
        let reg = json!({});
        let pats = ["*/*", "image/*"];
        let specific = accepts_specificity(&pats, "image/png", &reg).unwrap(); // image/* (len 5)
        let catchall = accepts_specificity(&pats, "audio/mpeg", &reg).unwrap(); // */* (0)
        assert!(specific > catchall);
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

    // ---- provenance membership facets (file-vs-data) ----

    #[test]
    fn subject_types_is_type_then_facets() {
        assert_eq!(
            subject_types(&json!({ "type": "text/csv", "_facets": ["inode/file"] })),
            vec!["text/csv", "inode/file"]
        );
        assert_eq!(subject_types(&json!({ "type": "text/plain" })), vec!["text/plain"]);
        assert!(subject_types(&json!({})).is_empty());
    }

    #[test]
    fn for_subject_matches_handle_verbs_via_inode_file_facet_only_with_provenance() {
        let reg = fixture();
        // A file: content type text/plain PLUS the provenance inode/file membership.
        let file = json!({ "type": "text/plain", "text": "hi", "_facets": ["inode/file"] });
        let n = names(&for_subject(&reg, &file));
        assert!(n.contains(&"open-poly".to_string()), "handle verb via facet missing: {n:?}");
        assert!(n.contains(&"echo-text".to_string()), "content verb missing: {n:?}");
        // The SAME content type WITHOUT the facet (e.g. clipboard text) gets no handle
        // verb — the provenance guard that keeps clipboard data from looking like a file.
        let clip = json!({ "type": "text/plain", "text": "hi" });
        assert!(!names(&for_subject(&reg, &clip)).contains(&"open-poly".to_string()));
    }

    #[test]
    fn render_accepts_a_file_via_facet_for_a_handle_verb() {
        let reg = fixture();
        let open_poly = lookup(&reg, "open-poly", None).unwrap(); // accepts inode/*, text/x-uri
        // text/plain matches neither accept; only the inode/file facet does.
        let file = json!({ "type": "text/plain", "id": "/x", "_facets": ["inode/file"] });
        assert!(render(&reg, &open_poly, &file, &json!({}), &json!({})).is_ok());
        let clip = json!({ "type": "text/plain", "id": "/x" });
        assert!(render(&reg, &open_poly, &clip, &json!({}), &json!({})).is_err());
    }

    #[test]
    fn lookup_subject_picks_the_impl_that_accepts_the_subject_not_the_first() {
        // Two impls of `act`: first accepts A, second accepts B. Dispatch must pick by
        // the SUBJECT, not registration order — the fix for `goo show :br/main` running
        // git's `show` (which accepts branches) instead of clipboard's first-registered.
        let reg = json!({ "verbs": [
            { "name": "act", "accepts": ["application/vnd.a"], "cmd": "A" },
            { "name": "act", "accepts": ["application/vnd.b"], "cmd": "B" },
        ]});
        assert_eq!(lookup_subject(&reg, "act", &json!({"type":"application/vnd.b"})).unwrap()["cmd"], json!("B"));
        assert_eq!(lookup_subject(&reg, "act", &json!({"type":"application/vnd.a"})).unwrap()["cmd"], json!("A"));
        // No impl accepts the subject → None (caller keeps its by-name pick).
        assert!(lookup_subject(&reg, "act", &json!({"type":"application/vnd.z"})).is_none());
        // Untyped subject → None.
        assert!(lookup_subject(&reg, "act", &json!({})).is_none());
    }

    #[test]
    fn lookup_subject_matches_an_impl_via_a_facet_membership() {
        // The contact case: `email` accepts the `emailable` facet; the subject's type is
        // `contact` but it claims `emailable` via a `_facet`, so lookup_subject finds it.
        let reg = json!({ "verbs": [
            { "name": "email", "accepts": ["application/vnd.emailable"], "cmd": "x" },
        ]});
        let contact = json!({ "type": "application/vnd.contact", "_facets": ["application/vnd.emailable"] });
        assert_eq!(lookup_subject(&reg, "email", &contact).unwrap()["cmd"], json!("x"));
        // Without the facet, the contact type alone doesn't match `email` → None.
        assert!(lookup_subject(&reg, "email", &json!({"type":"application/vnd.contact"})).is_none());
    }

    #[test]
    fn lookup_subject_reaches_the_middle_impl_of_a_three_way_family() {
        // `connect`/`stop`/`info` ship with three impls. The two-impl test above only
        // proves first and last are reachable; a middle impl is a distinct position
        // (preceded AND followed by non-matching impls). Pin that it resolves too.
        let reg = json!({ "verbs": [
            { "name": "act", "accepts": ["application/vnd.a"], "cmd": "A" },
            { "name": "act", "accepts": ["application/vnd.b"], "cmd": "B" },
            { "name": "act", "accepts": ["application/vnd.c"], "cmd": "C" },
        ]});
        assert_eq!(lookup_subject(&reg, "act", &json!({"type":"application/vnd.b"})).unwrap()["cmd"], json!("B"));
    }

    #[test]
    fn lookup_subject_prefers_the_more_specific_impl_over_a_glob_regardless_of_order() {
        // `info` ships a glob impl (`image/*`) beside more specific ones. Selection must
        // rank by specificity, not registration order: an exact impl must win over a
        // glob even when the glob is registered FIRST (the order that would lose without
        // the `s >= b` specificity comparison in lookup_subject).
        let reg = json!({ "verbs": [
            { "name": "view", "accepts": ["t/*"],   "cmd": "GLOB"  },
            { "name": "view", "accepts": ["t/png"], "cmd": "EXACT" },
        ]});
        // A type both match → exact wins (i32::MAX beats glob's prefix-length score).
        assert_eq!(lookup_subject(&reg, "view", &json!({"type":"t/png"})).unwrap()["cmd"], json!("EXACT"));
        // A type only the glob matches → glob is the lone candidate.
        assert_eq!(lookup_subject(&reg, "view", &json!({"type":"t/gif"})).unwrap()["cmd"], json!("GLOB"));
    }

    #[test]
    fn lookup_subject_skips_an_empty_accepts_impl_for_a_typed_subject() {
        // media's `stop` has empty `accepts` (a subjectless/global verb), registered
        // alongside container/service impls. verbs.rs:`accepts_type` — "a verb with no
        // accepts never matches a (non-empty) type" — must hold through dispatch: the
        // empty-accepts impl can never shadow a typed subject, even registered first.
        let reg = json!({ "verbs": [
            { "name": "stop", "accepts": [],            "cmd": "GLOBAL" },
            { "name": "stop", "accepts": ["t/unit"],    "cmd": "TYPED"  },
        ]});
        assert_eq!(lookup_subject(&reg, "stop", &json!({"type":"t/unit"})).unwrap()["cmd"], json!("TYPED"));
        // And on a subject no typed impl accepts, the empty-accepts impl still doesn't
        // match — lookup returns None (caller keeps its by-name pick), never GLOBAL.
        assert!(lookup_subject(&reg, "stop", &json!({"type":"t/other"})).is_none());
    }

    #[test]
    fn name_accepts_any_type_ignores_empty_impls_but_needs_one_typed() {
        // Mixed family (empty impl FIRST, typed second — the empty-first `stop` shape):
        // the NAME is subject-taking, so cmd_verb resolves a positional as a subject
        // instead of taking it as literal text (the footgun guard).
        let mixed = json!({ "verbs": [
            { "name": "stop", "accepts": [],         "cmd": "GLOBAL" },
            { "name": "stop", "accepts": ["t/unit"], "cmd": "TYPED"  },
        ]});
        assert!(name_accepts_any_type(&mixed, "stop"));
        // A pure subjectless verb (all impls empty) is NOT subject-taking → keep the
        // literal-text handling of `goo say "hello"`.
        let pure = json!({ "verbs": [ { "name": "say", "accepts": [], "cmd": "x" } ]});
        assert!(!name_accepts_any_type(&pure, "say"));
        // Unknown name → false.
        assert!(!name_accepts_any_type(&mixed, "nope"));
    }

    #[test]
    fn default_for_subject_prefers_content_default_then_falls_through_to_a_facet() {
        let reg = fixture(); // echo-text default_for text/plain; open-poly default_for inode/file
        // Content type with its own default wins over the facet.
        let txt = json!({ "type": "text/plain", "_facets": ["inode/file"] });
        assert_eq!(default_for_subject(&reg, &txt).unwrap()["name"], json!("echo-text"));
        // Content type with NO default falls through to the inode/file facet default —
        // so a bare `goo report.pdf` (no content default) still resolves `open`.
        let pdf_like = json!({ "type": "application/vnd.fixture.thing", "_facets": ["inode/file"] });
        assert_eq!(default_for_subject(&reg, &pdf_like).unwrap()["name"], json!("open-poly"));
        // No facet and no content default → none (unchanged from default_for).
        assert!(default_for_subject(&reg, &json!({ "type": "application/vnd.fixture.thing" })).is_none());
    }

    #[test]
    fn provider_for_type_inode_file_fires_on_a_file_via_facet_not_clipboard() {
        let mut reg = fixture();
        reg["providers"] = json!([{
            "name": "any", "for_type": "inode/file",
            "list_cmd": r#"printf '[{"name":"openwith","description":"d"}]'"#,
            "run": "echo {verb.name}",
        }]);
        let file = json!({ "type": "text/plain", "text": "hi", "_facets": ["inode/file"] });
        assert!(names(&provider_verbs_for(&reg, &file)).contains(&"openwith".to_string()));
        let clip = json!({ "type": "text/plain", "text": "hi" });
        assert!(provider_verbs_for(&reg, &clip).is_empty());
    }

    // ---- membership robustness & edge cases (negative + correctness) ----

    #[test]
    fn subject_types_is_robust_to_malformed_facets() {
        // Non-array `_facets` → ignored, just the type.
        assert_eq!(subject_types(&json!({"type":"text/plain","_facets":"oops"})), vec!["text/plain"]);
        // Empty array → just the type.
        assert_eq!(subject_types(&json!({"type":"text/plain","_facets":[]})), vec!["text/plain"]);
        // Non-string / empty-string / null elements are dropped; valid ones survive, in order.
        assert_eq!(
            subject_types(&json!({"type":"text/plain","_facets":[123,"","inode/file",null,"a/b"]})),
            vec!["text/plain", "inode/file", "a/b"]
        );
        // Missing `type`, facets present → just the facets.
        assert_eq!(subject_types(&json!({"_facets":["inode/file"]})), vec!["inode/file"]);
        // Empty `type` is skipped.
        assert_eq!(subject_types(&json!({"type":"","_facets":["x/y"]})), vec!["x/y"]);
        // Nothing → empty (no panic).
        assert!(subject_types(&json!({})).is_empty());
        assert!(subject_types(&json!({"type":""})).is_empty());
    }

    #[test]
    fn for_subject_inherits_from_each_of_multiple_facets() {
        // A subject claiming TWO capability facets gets the verbs of BOTH (and its content
        // verbs), but not a verb whose accept it never claims. Capability facets — the
        // contact-style case — not bus types.
        let reg = json!({ "verbs": [
            { "name": "text-op", "accepts": ["text/plain"], "cmd": "true" },
            { "name": "ping",    "accepts": ["application/vnd.test.pingable"], "cmd": "true" },
            { "name": "ring",    "accepts": ["application/vnd.test.ringable"], "cmd": "true" },
            { "name": "nope",    "accepts": ["application/vnd.other"], "cmd": "true" },
        ]});
        let subj = json!({ "type": "text/plain", "_facets": [
            "application/vnd.test.pingable", "application/vnd.test.ringable" ]});
        let n = names(&for_subject(&reg, &subj));
        for want in ["text-op", "ping", "ring"] {
            assert!(n.contains(&want.to_string()), "missing {want}: {n:?}");
        }
        assert!(!n.contains(&"nope".to_string()), "claimed an unrelated verb: {n:?}");
    }

    #[test]
    fn for_subject_facet_equal_to_type_does_not_double_count() {
        // A facet duplicating the content type must yield the verb exactly once.
        let reg = json!({ "verbs": [{ "name": "op", "accepts": ["text/plain"], "cmd": "true" }] });
        let subj = json!({ "type": "text/plain", "_facets": ["text/plain"] });
        let n = names(&for_subject(&reg, &subj));
        assert_eq!(n.iter().filter(|x| x.as_str() == "op").count(), 1, "double-counted: {n:?}");
    }

    #[test]
    fn for_subject_polymorphic_impl_resolves_across_type_and_facet() {
        // Two impls of `act`: one accepts the content type, one a facet — both match
        // exactly. `for_subject` keeps ONE impl per name; pin which (specificity tie →
        // later-registered wins, matching `lookup`'s `>=` tie-break).
        let reg = json!({ "verbs": [
            { "name": "act", "accepts": ["text/plain"], "cmd": "content-impl" },
            { "name": "act", "accepts": ["application/vnd.test.pingable"], "cmd": "facet-impl" },
        ]});
        let subj = json!({ "type": "text/plain", "_facets": ["application/vnd.test.pingable"] });
        let got = for_subject(&reg, &subj);
        let acts: Vec<&Value> = got.iter().filter(|v| v["name"] == json!("act")).collect();
        assert_eq!(acts.len(), 1, "polymorphic verb kept more than once");
        assert_eq!(acts[0]["cmd"], json!("facet-impl"), "later-registered tie should win");
    }

    #[test]
    fn default_for_subject_first_facet_with_a_default_wins() {
        // Content type has no default; the FIRST facet that declares one wins (order:
        // type, then facets left-to-right).
        let reg = json!({ "verbs": [
            { "name": "a", "accepts": ["application/vnd.x.a"], "default_for": "application/vnd.x.a", "cmd": "true" },
            { "name": "b", "accepts": ["application/vnd.x.b"], "default_for": "application/vnd.x.b", "cmd": "true" },
        ]});
        let subj = json!({ "type": "text/plain", "_facets": ["application/vnd.x.a", "application/vnd.x.b"] });
        assert_eq!(default_for_subject(&reg, &subj).unwrap()["name"], json!("a"));
    }

    #[test]
    fn handle_subject_drops_coincidental_glob_but_keeps_is_a_derived_glob() {
        let reg = json!({
            "types": [
                { "name": "application/vnd.x.window", "kind": "handle" },
                { "name": "application/vnd.x.mount", "kind": "handle", "is_a": ["inode/directory"] }
            ],
            "verbs": [
                { "name": "activate", "accepts": ["application/vnd.x.window"], "cmd": "x" }, // exact
                { "name": "hash", "accepts": ["application/*"], "cmd": "x" },                // same-family glob
                { "name": "anything", "accepts": ["*/*"], "cmd": "x" },                      // catch-all
                { "name": "open", "accepts": ["inode/*"], "cmd": "x" }                       // is_a-derived glob
            ]
        });
        // Window handle (no is_a): keep only the exact verb; the same-family glob
        // and the catch-all are coincidental noise. `open` (inode/*) doesn't match
        // a window at all.
        let win = names(&for_subject(&reg, &json!({"type":"application/vnd.x.window","id":"a"})));
        assert_eq!(win, vec!["activate".to_string()], "window keeps only its exact verb");

        // Mount handle is_a inode/directory: `open` (inode/*) reaches it THROUGH
        // is_a — a declared content kinship — so it's kept. But `hash`
        // (application/*) catches it only by same-namespace coincidence → dropped,
        // as does the `*/*` catch-all.
        let mnt = names(&for_subject(&reg, &json!({"type":"application/vnd.x.mount","id":"m"})));
        assert!(mnt.contains(&"open".to_string()), "is_a-derived inode/* glob kept on the mount");
        assert!(!mnt.contains(&"hash".to_string()), "coincidental application/* dropped on the mount");
        assert!(!mnt.contains(&"anything".to_string()), "*/* catch-all dropped on the mount");

        // The SAME globs still match a non-handle CONTENT type — the rule only
        // fires for handles.
        let content = names(&for_subject(
            &json!({ "types": [], "verbs": reg["verbs"].clone() }),
            &json!({"type":"application/json","text":"{}"}),
        ));
        assert!(content.contains(&"hash".to_string()), "glob still matches a content type");
        assert!(content.contains(&"anything".to_string()), "catch-all still matches a content type");
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

    // ---------------- dynamic verb providers ----------------

    fn reg_with_provider(list_cmd: &str) -> Value {
        json!({
            "verbs": [],
            "types": [{ "name": "application/vnd.goo.cwd", "kind": "handle" }],
            "providers": [{
                "name": "fix",
                "for_type": "application/vnd.goo.cwd",
                "list_cmd": list_cmd,
                "run": "echo ran-{verb.name}",
            }],
        })
    }

    #[test]
    fn provider_verbs_synthesized_with_run_template_and_for_type() {
        let reg = reg_with_provider(
            r#"printf '[{"name":"foo","description":"d-foo"},{"name":"bar","description":"d-bar"}]'"#,
        );
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": "/x" });
        let v = provider_verbs_for(&reg, &subject);
        let names: Vec<&str> = v.iter().filter_map(|x| x["name"].as_str()).collect();
        assert_eq!(names, ["foo", "bar"]);
        // Each carries the provider's run template as cmd, the for_type as accepts,
        // and the dynamic marker — render substitutes {verb.name} from context.
        assert_eq!(v[0]["cmd"], json!("echo ran-{verb.name}"));
        assert_eq!(v[0]["accepts"], json!(["application/vnd.goo.cwd"]));
        assert_eq!(v[0]["dynamic"], json!(true));
        assert_eq!(v[0]["description"], json!("d-foo"));
    }

    #[test]
    fn verb_name_validity_rule() {
        for ok in ["test", "build-bash", "json-pretty", "build:prod", "a.b", "v2", "x/y", "c++"] {
            assert!(is_valid_verb_name(ok), "{ok} should be valid");
        }
        for bad in [
            "", "-leading", "/leading", "a;b", "a b", "a|b", "a$b", "a`b", "rm -rf ~",
            "a&b", "a>b", "a*b", "a(b", "a\nb", "a'b",
        ] {
            assert!(!is_valid_verb_name(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn provider_drops_unsafe_names() {
        // A hostile name never becomes a verb — so it can't reach the bash cmd,
        // with or without |q in the run template.
        let reg = reg_with_provider(r#"printf '[{"name":"a;touch pwned"},{"name":"ok"}]'"#);
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": "/x" });
        let v = provider_verbs_for(&reg, &subject);
        let names: Vec<&str> = v.iter().filter_map(|x| x["name"].as_str()).collect();
        assert_eq!(names, ["ok"]);
    }

    #[test]
    fn provider_does_not_fire_for_non_matching_type() {
        let reg = reg_with_provider(r#"printf '[{"name":"foo"}]'"#);
        // A text subject: for_type is application/vnd.goo.cwd, so no exec, no verbs.
        let v = provider_verbs_for(&reg, &json!({ "type": "text/plain", "text": "hi" }));
        assert!(v.is_empty());
    }

    #[test]
    fn provider_failure_is_graceful() {
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": "/x" });
        // Non-zero exit, non-JSON, and non-array each yield no verbs (never panics).
        for cmd in ["exit 1", "echo not-json", r#"printf '{"name":"x"}'"#] {
            let reg = reg_with_provider(cmd);
            assert!(provider_verbs_for(&reg, &subject).is_empty(), "cmd: {cmd}");
        }
    }

    #[test]
    fn for_subject_appends_provider_verbs_static_wins_collision() {
        // A static verb `foo` plus a provider that also offers `foo` and a new `bar`.
        let mut reg = reg_with_provider(
            r#"printf '[{"name":"foo","description":"dyn"},{"name":"bar"}]'"#,
        );
        reg["verbs"] = json!([{
            "name": "foo",
            "accepts": ["application/vnd.goo.cwd"],
            "description": "static",
            "cmd": "true",
        }]);
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": "/x" });
        let got = for_subject(&reg, &subject);
        let names: Vec<&str> = got.iter().filter_map(|v| v["name"].as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"bar")); // the new dynamic verb is appended
        // The kept `foo` is the STATIC one (provider can't shadow a built-in).
        let foo = got.iter().find(|v| v["name"] == json!("foo")).unwrap();
        assert_eq!(foo["description"], json!("static"));
        assert!(foo.get("dynamic").is_none());
    }

    #[test]
    fn provider_list_cmd_sees_the_subject() {
        // The new capability: list_cmd is subject-substituted, so a per-subject
        // provider enumerates verbs from THIS subject — here the description
        // reflects the specific subject's id, not just ambient context.
        let reg = reg_with_provider(
            r#"printf '[{"name":"go","description":"opens {subject.id}"}]'"#,
        );
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": "report.pdf" });
        let v = provider_verbs_for(&reg, &subject);
        assert_eq!(v[0]["description"], json!("opens report.pdf"));
    }

    #[test]
    fn provider_list_cmd_subject_field_is_shell_safe_with_q() {
        // Subject fields now reach bash via list_cmd — a filename can carry shell
        // metacharacters, so `{subject.id|q}` must neutralize them. This is the
        // list_cmd analogue of the run-template injection tests: with |q a hostile
        // id can't execute, and the provider still yields its verbs.
        let dir = std::env::temp_dir().join(format!("goo-prov-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("pwned");
        let payload = format!("a; touch {}", marker.display());
        let subject = json!({ "type": "application/vnd.goo.cwd", "id": payload });
        let reg = reg_with_provider(
            r#"echo {subject.id|q} >/dev/null; printf '[{"name":"go","description":"d"}]'"#,
        );
        let v = provider_verbs_for(&reg, &subject);
        assert!(!marker.exists(), "subject-field injection executed via list_cmd!");
        let names: Vec<&str> = v.iter().filter_map(|x| x["name"].as_str()).collect();
        assert_eq!(names, ["go"]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
