//! Entity-name inference — bare token → subject resolution via per-source
//! candidate scoring + confidence bands. The keystone of data-entry-ux.md §3:
//! makes `firefox` resolve like `:app/firefox` would, **only** when the match is
//! safe to make silently (DEFINITIVE band) or worth nudging about (HIGH band).
//!
//! Spec lock: `doc/design/data-entry-ux.md` §3.2 (bands), §3.2.2 (scoring),
//! §3.2.3 (context adaptation), §3.2.4 (source weights). The spec is the
//! authority; this module is a faithful implementation.
//!
//! **v1 scope** (slice #7):
//!  - scoring + bands + per-source weights ✓
//!  - source enumeration (calls `list_cmd` per inferable source), now with a
//!    per-source TTL cache at `$XDG_RUNTIME_DIR/cosmic-goo/entities/<name>.json`
//!    (slice 7b, task #24, §3.3) + the `inferable` opt-in field. The cache is
//!    an optimization, never a correctness gate: any IO/parse miss falls back
//!    to running `list_cmd` directly. mtime/dbus-signal invalidation hooks are
//!    deferred — the TTL is §3.3's stated universal fallback for every source.
//!  - verb-position only (noun-first dispatch); subject-position inference is
//!    slice #8 (verb-aware bias) — caller passes the bare token, not a
//!    verb+token pair
//!  - `Context` parameter accepted but engine returns the SAME band regardless;
//!    the (band, context) → action mapping is the caller's responsibility per
//!    §3.2.3 ("bands are the user-facing model; UX response differs by context")
//!  - `Reason` is minimal — band + scores + winner + matched-pattern. No
//!    alternatives list (that's picker-UI data; deferred with the picker)

use crate::shell::bash_capture;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ---------- bands and floors (§3.2.1, §3.2.2) ----------

/// The four user-facing confidence bands. UX response per (band, context) is
/// the caller's responsibility; this enum is the wire shape between engine
/// and caller. Order: stronger → weaker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// Unique candidate at exact-id/title (single-candidate by construction).
    /// Safe to resolve silently in ANY context, including scripts.
    Definitive,
    /// Top score ≥ HIGH_FLOOR AND top ≥ 2× second AND ≤3 candidates.
    /// Interactive: resolve + one-line nudge. Script: nudge-then-fallback.
    High,
    /// Top score ≥ MEDIUM_FLOOR but ambiguous (close second or many results).
    /// Interactive: surface picker. Script: always fall through.
    Medium,
    /// Top score < MEDIUM_FLOOR. Fall through to text/plain (current default).
    Low,
}

/// Why a candidate beat the others — feeds the §3.5 nudge log and tests that
/// assert "this input hit DEFINITIVE because exact-id match". Order corresponds
/// to the score buckets in `score_item`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchedPattern {
    ExactId,
    ExactTitle,
    WordBoundary,
    IdSubstring,
    TitleSubstring,
}

/// The context in which inference is being attempted. Passed through to the
/// `Reason` so logs/callers can correlate, but **the engine returns the same
/// band regardless of context** — the (band, context) → action mapping is the
/// caller's job per §3.2.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// Non-TTY (pipes, cron, CI). Only DEFINITIVE resolves silently.
    Script,
    /// TTY (human at a terminal). All bands behave per §3.2.1.
    Interactive,
    /// compose-GUI / inline launcher. MEDIUM is the primary picker mode.
    Gui,
}

/// **EXACT_FLOOR**: above this only an exact id (1000) or exact title (800)
/// can land. Clears the natural gap between exact and substring scores.
const EXACT_FLOOR: f64 = 800.0;
/// **HIGH_FLOOR**: above this is "meaningful match" territory (id-substring
/// 200 max, decent title-substring × source weight). Below = guess.
const HIGH_FLOOR: f64 = 200.0;
/// **MEDIUM_FLOOR**: above this is "ambiguous but plausible" territory. Below =
/// noise; fall through to text.
const MEDIUM_FLOOR: f64 = 60.0;

/// Default per-source list-cache TTL (seconds) when a source declares no
/// `cache_ttl`. 5s matches §3.3's stated fallback for the cheap-but-volatile
/// launcher sources (apps/windows/workspaces/focused). A source can override
/// (a quieter source raises it; `cache_ttl = 0` opts a volatile source out of
/// caching entirely — always re-run `list_cmd`).
const DEFAULT_CACHE_TTL_SECS: u64 = 5;

// ---------- public output types ----------

/// A scored, source-tagged candidate. Intermediate value — not returned to
/// callers directly (the orchestrator builds a `Reason` from the top-N).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub source_name: String,
    pub source_prefix: String,
    pub emits: String,
    pub id: String,
    pub title: String,
    pub score: f64,
    pub pattern: MatchedPattern,
}

/// Explanation of the inference decision — enough to power §3.5's nudge log,
/// the MEDIUM-band picker message, and tests asserting correctness.
///
/// `alternatives` carries up to `MAX_ALTERNATIVES` of the top candidates
/// (INCLUDING the winner at index 0), as `(source_prefix, id, title)` tuples
/// — small payload, no `Candidate` type leak, enough for a numbered picker
/// render. Capped because picker UX gets useless past ~5 choices anyway.
#[derive(Debug, Clone)]
pub struct Reason {
    pub band: Band,
    pub top_score: f64,
    pub second_score: Option<f64>,
    pub candidate_count: usize,
    pub winner_source: String, // the source's `prefix` (e.g. "app"); name if no prefix
    pub winner_label: String,  // the candidate's `title`
    pub matched_pattern: MatchedPattern,
    /// Top-N (prefix, id, title). `[0]` is the winner. Length is min(N,
    /// total candidates). Useful for the MEDIUM-band picker; HIGH and
    /// DEFINITIVE consumers can ignore it.
    pub alternatives: Vec<(String, String, String)>,
}

/// Cap on `Reason::alternatives` length — picker UX past 5 choices stops
/// being useful (the user starts re-typing instead). Engine-side cap so the
/// payload stays small whether the caller wants it or not.
pub const MAX_ALTERNATIVES: usize = 5;

// ---------- public API ----------

/// Resolve a bare token to an inferred subject, with the band + reason that
/// the caller maps to a UX response per §3.2.3.
///
/// `ctx` is passed through to the caller's UX decision; the engine returns
/// the SAME band regardless of context (the spec keeps band-assignment
/// context-independent and lets the caller adapt the action).
///
/// Errors:
///  - `Err("not an inferable shape")` — `raw` failed `is_inferable_shape`.
///    Caller should fall through to existing logic (text/plain, verb lookup).
///  - `Err("no candidates")` — no inferable source had ANY matching item.
///    Caller should fall through.
pub fn infer_entity(raw: &str, reg: &Value, ctx: Context) -> Result<(Value, Band, Reason), String> {
    infer_impl(raw, reg, ctx, None)
}

/// Verb-aware entity inference (slice 8 / §3.4). Same as `infer_entity` but
/// the candidate pool is biased toward sources the verb can actually consume:
/// a source participates only if it passes the §3.3 participation gate **AND**
/// the verb `accepts` its `emits` type. The accepts-filter *narrows*; it does
/// not widen past the privacy gate — §3.6 guarantees `inferable = false`
/// sources (clipboard-history, …) never enter the scan, even when a verb
/// accepts their type. (`enumerate = false` sources a verb accepts therefore
/// drop out of *scored* inference too; the bin resolves those via its ungated
/// `handle_search` fallback until they earn `inferable = true`.)
pub fn infer_entity_for_verb(
    raw: &str,
    reg: &Value,
    ctx: Context,
    verb: &Value,
) -> Result<(Value, Band, Reason), String> {
    infer_impl(raw, reg, ctx, Some(verb))
}

/// Shared orchestration for the noun-first (`verb_filter = None`) and
/// verb-aware (`Some(verb)`) entry points.
fn infer_impl(
    raw: &str,
    reg: &Value,
    ctx: Context,
    verb_filter: Option<&Value>,
) -> Result<(Value, Band, Reason), String> {
    let _ = ctx; // accepted for symmetry / future use; v1 engine is ctx-agnostic
    if !is_inferable_shape(raw) {
        return Err("not an inferable shape".into());
    }
    let candidates = enumerate_and_score(raw, reg, verb_filter);
    if candidates.is_empty() {
        return Err("no candidates".into());
    }
    let (band, reason) = assign_band(&candidates);
    let winner = &candidates[0];
    let subject = build_subject(winner);
    Ok((subject, band, reason))
}

/// Shape gate — cheap pre-check the caller runs before paying for source
/// enumeration. Inferable shapes:
///   - single token (no whitespace)
///   - length 2..=80
///   - no addressing characters (`/` `:` `=` `+` `^`), no path-leaders (`.` `~`)
///   - no leading dash (CLI flags)
///   - no leading digit if the rest is also digits/operators (`2+2` belongs to
///     the calc verb's text path, not inference)
///
/// Returns true if the shape COULD be an entity name; false otherwise.
/// Negative-result is conservative: shapes that pass this gate are still
/// subject to "no candidates matched" errors.
pub fn is_inferable_shape(raw: &str) -> bool {
    let len = raw.chars().count();
    if !(2..=80).contains(&len) {
        return false;
    }
    if raw.chars().any(char::is_whitespace) {
        return false;
    }
    if raw.starts_with('-') {
        return false;
    }
    // Addressing characters are handled by `address::canonicalize` stages A-D
    // (per data-entry-ux.md §3.1); inference is only stage E (the fallback).
    if raw.contains('/')
        || raw.contains(':')
        || raw.contains('=')
        || raw.starts_with('+')
        || raw.starts_with('^')
        || raw.starts_with('.')
        || raw.starts_with('~')
    {
        return false;
    }
    // Pure-digits-and-operators stays text (calc verb territory). The decimal
    // point is part of the arithmetic alphabet too (`100*3.14`).
    if raw.chars().all(|c| c.is_ascii_digit() || "+-*/^%().".contains(c)) {
        return false;
    }
    true
}

// ---------- pure scoring (testable without bash subprocesses) ----------

/// Score one (id, title) pair for token `t`. Returns `None` if no match — the
/// `0` score from the spec is represented as `None` so callers can drop
/// non-matching items without filtering on a magic constant. The pattern
/// identifies WHICH branch fired (§3.2.2's cascade).
pub fn score_item(t: &str, id: &str, title: &str) -> Option<(f64, MatchedPattern)> {
    if id == t {
        return Some((1000.0, MatchedPattern::ExactId));
    }
    if title == t {
        return Some((800.0, MatchedPattern::ExactTitle));
    }
    let tl = t.to_lowercase();
    let idl = id.to_lowercase();
    let titlel = title.to_lowercase();
    if let Some(score) = word_boundary_score(&tl, &titlel) {
        return Some((score, MatchedPattern::WordBoundary));
    }
    if !id.is_empty() && idl.contains(&tl) {
        let ratio = tl.chars().count() as f64 / id.chars().count() as f64;
        return Some((200.0 * ratio, MatchedPattern::IdSubstring));
    }
    if !title.is_empty() && titlel.contains(&tl) {
        let ratio = tl.chars().count() as f64 / title.chars().count() as f64;
        return Some((100.0 * ratio, MatchedPattern::TitleSubstring));
    }
    None
}

/// Word-boundary match: title starts with `t`, or contains ` t` or `-t` or
/// `_t` (the typical word-separator chars in titles). v1 keeps this simple —
/// regex-based boundary detection is future polish.
fn word_boundary_score(t: &str, title: &str) -> Option<f64> {
    if title.starts_with(t)
        || title.contains(&format!(" {t}"))
        || title.contains(&format!("-{t}"))
        || title.contains(&format!("_{t}"))
    {
        let ratio = t.chars().count() as f64 / title.chars().count().max(1) as f64;
        return Some(400.0 * ratio);
    }
    None
}

/// Assign a band given the sorted candidates (highest score first). Pure —
/// no IO, no registry access. The (band, reason) is what gets returned to
/// the caller; the caller maps to a UX action.
pub fn assign_band(candidates: &[Candidate]) -> (Band, Reason) {
    let top = &candidates[0];
    let second_score = candidates.get(1).map(|c| c.score);
    let exact_count = candidates.iter().filter(|c| c.score >= EXACT_FLOOR).count();

    let band = if exact_count == 1 && top.score >= EXACT_FLOOR {
        // DEFINITIVE: single candidate at the exact floor. The uniqueness
        // requirement is what makes script-context resolution safe.
        Band::Definitive
    } else if top.score >= HIGH_FLOOR
        && second_score.is_none_or(|s| top.score >= 2.0 * s)
        && candidates.len() <= 3
    {
        Band::High
    } else if top.score >= MEDIUM_FLOOR {
        Band::Medium
    } else {
        Band::Low
    };

    let source_id = |c: &Candidate| -> String {
        if c.source_prefix.is_empty() {
            c.source_name.clone()
        } else {
            c.source_prefix.clone()
        }
    };
    let alternatives: Vec<(String, String, String)> = candidates
        .iter()
        .take(MAX_ALTERNATIVES)
        .map(|c| (source_id(c), c.id.clone(), c.title.clone()))
        .collect();

    let reason = Reason {
        band,
        top_score: top.score,
        second_score,
        candidate_count: candidates.len(),
        winner_source: source_id(top),
        winner_label: top.title.clone(),
        matched_pattern: top.pattern,
        alternatives,
    };
    (band, reason)
}

// ---------- per-source list cache (§3.3) ----------

/// Pure freshness predicate, factored out so the TTL policy is unit-testable
/// without touching disk. `age_secs` is the cache entry's age; `None` means
/// the age couldn't be determined (missing/future mtime → treat as stale, not
/// an error). `ttl_secs == 0` means "never cache" → always stale.
pub fn cache_is_fresh(age_secs: Option<u64>, ttl_secs: u64) -> bool {
    if ttl_secs == 0 {
        return false;
    }
    match age_secs {
        Some(age) => age < ttl_secs,
        None => false,
    }
}

/// `$XDG_RUNTIME_DIR/cosmic-goo/entities/`, or `None` if the runtime dir is
/// unset (then caching is disabled and we always run `list_cmd`).
fn entity_cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(base).join("cosmic-goo").join("entities"))
}

/// Fetch a source's list items, served from the per-source TTL cache when warm.
/// The cache is an optimization, never a correctness gate: an unset
/// `XDG_RUNTIME_DIR`, `ttl == 0`, an empty source name, or any IO/parse failure
/// all fall back to running `list_cmd` directly. We store the `cmd` alongside
/// the `items` so a changed `list_cmd` busts the entry, and we never write an
/// empty result (a transient `list_cmd` failure must not pin a source out of
/// inference for the whole TTL).
fn fetch_source_items(name: &str, list_cmd: &str, ttl_secs: u64) -> Vec<Value> {
    let cache_file = if ttl_secs > 0 && !name.is_empty() {
        entity_cache_dir().map(|d| d.join(format!("{name}.json")))
    } else {
        None
    };

    if let Some(ref path) = cache_file {
        if let Some(items) = read_fresh_cache(path, list_cmd, ttl_secs) {
            return items;
        }
    }

    let output = bash_capture(list_cmd);
    let items: Vec<Value> = serde_json::from_str(output.trim()).unwrap_or_default();

    if let Some(ref path) = cache_file {
        if !items.is_empty() {
            write_cache_atomic(path, list_cmd, &items);
        }
    }
    items
}

/// Read `<name>.json` if it's fresh AND was written for this exact `list_cmd`.
/// Any failure (missing, stale, parse error, cmd mismatch) returns `None` →
/// the caller re-runs `list_cmd`.
fn read_fresh_cache(path: &Path, list_cmd: &str, ttl_secs: u64) -> Option<Vec<Value>> {
    let meta = std::fs::metadata(path).ok()?;
    // `duration_since` errs on a future mtime (clock skew / `touch -d future`);
    // `.ok()` maps that to `None` → `cache_is_fresh(None, _) == false` → stale.
    let age = SystemTime::now().duration_since(meta.modified().ok()?).ok();
    if !cache_is_fresh(age.map(|d| d.as_secs()), ttl_secs) {
        return None;
    }
    let cached: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    // Bust if the source's command changed since this entry was written.
    if cached.get("cmd").and_then(Value::as_str) != Some(list_cmd) {
        return None;
    }
    Some(cached.get("items")?.as_array()?.clone())
}

/// Write `{cmd, items}` to `<name>.json` atomically (temp + rename) so a reader
/// — or an overlapping per-keystroke writer — never sees a half-written file.
/// Best-effort: every failure is swallowed (the cache is non-load-bearing).
fn write_cache_atomic(path: &Path, list_cmd: &str, items: &[Value]) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(serialized) = serde_json::to_string(&json!({ "cmd": list_cmd, "items": items })) else {
        return;
    };
    // pid-unique temp in the same dir → rename is atomic on the same filesystem.
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, serialized.as_bytes()).is_ok() {
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ---------- orchestration (touches IO) ----------

/// Enumerate every inferable source, run its `list_cmd` (through the per-source
/// TTL cache), score each item against `raw`, apply the source's weight.
/// Returns the candidates sorted by score descending (so `[0]` is the winner).
///
/// **Participation rule** (§3.3): a source participates if its `inferable`
/// field is set, honored verbatim; if absent, it falls back to `enumerate !=
/// false`. This is opt-*out* semantics under an opt-*in* spec — a deliberate
/// v1 choice so the spec's default-true sources (apps/windows/recent/…) and
/// every existing test fixture participate without per-source tagging, while
/// the genuinely-undesirable sources (processes/containers/branches/hist —
/// already `enumerate = false`) stay out for free. The two sources §3.3 wants
/// participating *despite* `enumerate = false` (bluetooth, services) are slow
/// (a timeout-bounded probe / a full unit scan) and stay deferred under this
/// TTL-only cache — they opt back in with `inferable = true` once the entity
/// cache grows signal-based invalidation (§3.3).
///
/// `verb_filter` (slice 8 / §3.4): when `Some(verb)`, a source ALSO has to have
/// its `emits` accepted by the verb. This *narrows* on top of the participation
/// gate — never widens past it — so §3.6's privacy guarantee (no `inferable =
/// false` source ever enters the scan) holds in verb-aware mode too.
fn enumerate_and_score(raw: &str, reg: &Value, verb_filter: Option<&Value>) -> Vec<Candidate> {
    let Some(sources) = reg.get("sources").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = Vec::new();
    for source in sources {
        let participates = match source.get("inferable").and_then(Value::as_bool) {
            Some(flag) => flag,
            None => source.get("enumerate") != Some(&json!(false)),
        };
        if !participates {
            continue;
        }
        // Verb-aware bias: keep only sources whose emit type the verb accepts.
        if let Some(verb) = verb_filter {
            let emits = source.get("emits").and_then(Value::as_str).unwrap_or("");
            if emits.is_empty() || !verb_accepts_emits(verb, emits, reg) {
                continue;
            }
        }
        let Some(list_cmd) = source.get("list_cmd").and_then(Value::as_str) else {
            continue;
        };
        if list_cmd.is_empty() {
            continue;
        }
        let weight = source.get("weight").and_then(Value::as_f64).unwrap_or(1.0);
        let source_name = source.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        let source_prefix = source.get("prefix").and_then(Value::as_str).unwrap_or("").to_string();
        let emits = source.get("emits").and_then(Value::as_str).unwrap_or("").to_string();
        let ttl = source
            .get("cache_ttl")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_CACHE_TTL_SECS);

        let items = fetch_source_items(&source_name, list_cmd, ttl);
        for item in items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let title = item.get("title").and_then(Value::as_str).unwrap_or("").to_string();
            if let Some((score, pattern)) = score_item(raw, &id, &title) {
                let weighted = score * weight;
                if weighted > 0.0 {
                    out.push(Candidate {
                        source_name: source_name.clone(),
                        source_prefix: source_prefix.clone(),
                        emits: emits.clone(),
                        id,
                        title,
                        score: weighted,
                        pattern,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// True when the verb's `accepts` admits a source's `emits` type — subtype-
/// aware (mirrors the bin's `handle_search` accept test). A verb with no
/// `accepts` (subjectless) admits nothing, which is correct here: the
/// verb-aware path is only reached for verbs that take a subject.
fn verb_accepts_emits(verb: &Value, emits: &str, reg: &Value) -> bool {
    verb.get("accepts")
        .and_then(Value::as_array)
        .map(|accepts| {
            accepts
                .iter()
                .filter_map(|p| p.as_str())
                .any(|pat| crate::mime::is_subtype(emits, pat, reg))
        })
        .unwrap_or(false)
}

/// Build the subject Value for the winning candidate — same shape
/// `address::resolve` produces for the equivalent `:<prefix>/<id>` query.
/// The caller can hand this straight to `cmd_goo` / verb dispatch.
fn build_subject(winner: &Candidate) -> Value {
    json!({
        "id": winner.id,
        "title": winner.title,
        "type": winner.emits,
    })
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    // ---------- is_inferable_shape ----------

    #[test]
    fn shape_accepts_plain_identifiers() {
        assert!(is_inferable_shape("firefox"));
        assert!(is_inferable_shape("build1"));
        assert!(is_inferable_shape("com.system76.CosmicEdit"));
        assert!(is_inferable_shape("Notes.md")); // dots inside are OK (not at start)
        assert!(is_inferable_shape("a1")); // 2-char minimum
    }

    #[test]
    fn shape_rejects_addressing_characters() {
        assert!(!is_inferable_shape(":app:firefox")); // sigil — stage B
        assert!(!is_inferable_shape("app/firefox")); // prefix-shape — stage D
        assert!(!is_inferable_shape("=text/markdown")); // type sigil
        assert!(!is_inferable_shape("+literal text"));
        assert!(!is_inferable_shape("^buf"));
        assert!(!is_inferable_shape("./path"));
        assert!(!is_inferable_shape("~/home"));
    }

    #[test]
    fn shape_rejects_whitespace_and_flags_and_extremes() {
        assert!(!is_inferable_shape("hello world")); // multi-word: text
        assert!(!is_inferable_shape("--help")); // flag
        assert!(!is_inferable_shape("-x")); // short flag
        assert!(!is_inferable_shape("a")); // too short
        assert!(!is_inferable_shape(&"x".repeat(81))); // too long
        assert!(!is_inferable_shape("")); // empty
    }

    #[test]
    fn shape_rejects_pure_arithmetic_for_calc_verb() {
        // The calc verb consumes these as text; don't waste a source scan.
        assert!(!is_inferable_shape("2+2"));
        assert!(!is_inferable_shape("100*3.14"));
        assert!(!is_inferable_shape("(1+2)/3"));
    }

    // ---------- score_item ----------

    #[test]
    fn score_exact_id_wins_above_all_else() {
        let (s, p) = score_item("firefox", "firefox", "Firefox Browser").unwrap();
        assert_eq!(s, 1000.0);
        assert_eq!(p, MatchedPattern::ExactId);
    }

    #[test]
    fn score_exact_title_when_id_differs() {
        let (s, p) = score_item("Firefox", "org.mozilla.firefox", "Firefox").unwrap();
        assert_eq!(s, 800.0);
        assert_eq!(p, MatchedPattern::ExactTitle);
    }

    #[test]
    fn score_word_boundary_lower_than_exact_id_or_title() {
        // "fox" matches "Fox-recipe.md" at a word boundary (after the dash).
        let (s, p) = score_item("fox", "fox-recipe-md", "Fox-recipe.md").unwrap();
        assert!(s < 800.0);
        assert_eq!(p, MatchedPattern::WordBoundary);
    }

    #[test]
    fn score_id_substring_under_word_boundary() {
        // "ox" doesn't word-boundary-match "firefox" but is in the id.
        let (s, p) = score_item("ox", "firefox", "Firefox").unwrap();
        assert!(s < 200.0); // 200 * (2/7) ≈ 57
        assert_eq!(p, MatchedPattern::IdSubstring);
    }

    #[test]
    fn score_title_substring_lowest() {
        // "ire" is inside "Firefox" title but not id.
        let (s, p) = score_item("ire", "fx", "Firefox").unwrap();
        assert!(s < 100.0); // 100 * (3/7) ≈ 42
        assert_eq!(p, MatchedPattern::TitleSubstring);
    }

    #[test]
    fn score_no_match_is_none() {
        assert_eq!(score_item("chrome", "firefox", "Firefox Browser"), None);
    }

    #[test]
    fn score_is_case_insensitive_for_substring_branches() {
        // Exact branches are case-SENSITIVE (per spec); substring branches use
        // lowercase compare so "FIREFOX" finds "firefox" inside a longer string.
        let (s, _) = score_item("FIREFOX", "firefox", "Firefox").unwrap();
        // "FIREFOX" != "firefox" exact-id, but lowercased equals → word-boundary
        // hits via `title.starts_with(t)` after lowercasing both sides.
        assert!(s >= 200.0);
    }

    // ---------- assign_band ----------

    fn cand(source: &str, id: &str, title: &str, score: f64, pat: MatchedPattern) -> Candidate {
        Candidate {
            source_name: source.to_string(),
            source_prefix: source.to_string(),
            emits: "x/y".to_string(),
            id: id.to_string(),
            title: title.to_string(),
            score,
            pattern: pat,
        }
    }

    #[test]
    fn band_definitive_requires_unique_above_exact_floor() {
        let cs = vec![cand("app", "firefox", "Firefox", 1000.0, MatchedPattern::ExactId)];
        let (b, r) = assign_band(&cs);
        assert_eq!(b, Band::Definitive);
        assert_eq!(r.candidate_count, 1);
        assert_eq!(r.winner_source, "app");
        assert_eq!(r.matched_pattern, MatchedPattern::ExactId);
    }

    #[test]
    fn band_high_when_top_dominates_second_with_few_results() {
        // HIGH requires: top ≥ HIGH_FLOOR (200) AND top ≥ 2× second AND count ≤ 3
        // AND no exact-floor candidate (which would have made it DEFINITIVE).
        // Top 500 (word-boundary, below EXACT_FLOOR), second 200 (id-substring),
        // count 2. 500 ≥ 200 ✓, 500 ≥ 2*200 ✓, count ≤ 3 ✓, no exact floor.
        let cs = vec![
            cand("app", "fox-thing", "Fox thing", 500.0, MatchedPattern::WordBoundary),
            cand("recent", "fox-doc", "Fox doc", 200.0, MatchedPattern::IdSubstring),
        ];
        let (b, _) = assign_band(&cs);
        assert_eq!(b, Band::High);
    }

    #[test]
    fn band_high_when_single_substring_match_is_well_above_floor() {
        // Lone candidate at HIGH_FLOOR (no second to compare). 250 >= 200 ✓,
        // no second → 2× check vacuously passes, count 1 ≤ 3 ✓, not exact.
        let cs = vec![cand("a", "fox-x", "X", 250.0, MatchedPattern::IdSubstring)];
        let (b, _) = assign_band(&cs);
        assert_eq!(b, Band::High);
    }

    #[test]
    fn band_medium_when_top_does_not_dominate() {
        // 600 vs 400 — top < 2*second → not HIGH. Top ≥ MEDIUM_FLOOR → MEDIUM.
        let cs = vec![
            cand("a", "x", "X", 600.0, MatchedPattern::WordBoundary),
            cand("b", "y", "Y", 400.0, MatchedPattern::WordBoundary),
        ];
        let (b, _) = assign_band(&cs);
        assert_eq!(b, Band::Medium);
    }

    #[test]
    fn band_medium_when_count_exceeds_three_even_if_dominant() {
        // top 500 (word-boundary, below EXACT_FLOOR so not DEFINITIVE), four
        // candidates total: HIGH requires count ≤ 3, so falls to MEDIUM even
        // though top dominates second (500 ≥ 2*100). The count gate prevents
        // a clear winner "amid noise" from feeling like a guess.
        let cs = vec![
            cand("a", "1", "X", 500.0, MatchedPattern::WordBoundary),
            cand("b", "2", "Y", 100.0, MatchedPattern::TitleSubstring),
            cand("c", "3", "Z", 100.0, MatchedPattern::TitleSubstring),
            cand("d", "4", "W", 100.0, MatchedPattern::TitleSubstring),
        ];
        let (b, _) = assign_band(&cs);
        assert_eq!(b, Band::Medium);
    }

    #[test]
    fn band_low_when_top_below_medium_floor() {
        let cs = vec![cand("a", "x", "Mostly-unrelated-text-with-xyz", 30.0, MatchedPattern::TitleSubstring)];
        let (b, _) = assign_band(&cs);
        assert_eq!(b, Band::Low);
    }

    #[test]
    fn band_is_not_definitive_when_two_candidates_both_hit_exact_floor() {
        // Spec safety property: DEFINITIVE requires UNIQUENESS at exact-floor.
        // Two exact-title matches across different sources → must NOT be
        // DEFINITIVE (would auto-resolve to one when both are valid).
        let cs = vec![
            cand("a", "1", "Notes", 800.0, MatchedPattern::ExactTitle),
            cand("b", "2", "Notes", 800.0, MatchedPattern::ExactTitle),
        ];
        let (b, _) = assign_band(&cs);
        assert_ne!(b, Band::Definitive, "DEFINITIVE must require unique exact-floor candidate");
        // 800 >= 2*800? No → not HIGH. Top >= MEDIUM_FLOOR → MEDIUM.
        assert_eq!(b, Band::Medium);
    }

    #[test]
    fn reason_carries_second_score_and_count() {
        let cs = vec![
            cand("a", "1", "X", 600.0, MatchedPattern::WordBoundary),
            cand("b", "2", "Y", 400.0, MatchedPattern::WordBoundary),
            cand("c", "3", "Z", 200.0, MatchedPattern::IdSubstring),
        ];
        let (_, r) = assign_band(&cs);
        assert_eq!(r.top_score, 600.0);
        assert_eq!(r.second_score, Some(400.0));
        assert_eq!(r.candidate_count, 3);
    }

    #[test]
    fn reason_alternatives_cap_at_max_and_winner_first() {
        // 7 candidates → alternatives capped at MAX_ALTERNATIVES (5);
        // alternatives[0] is the winner by score (sorted desc).
        let mut cs: Vec<Candidate> = (0..7)
            .map(|i| cand(&format!("s{i}"), &format!("id{i}"), &format!("T{i}"), 700.0 - i as f64 * 50.0, MatchedPattern::WordBoundary))
            .collect();
        cs.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap()); // assign_band assumes sorted
        let (_, r) = assign_band(&cs);
        assert_eq!(r.alternatives.len(), MAX_ALTERNATIVES);
        assert_eq!(r.alternatives[0].1, "id0"); // winner
        assert_eq!(r.candidate_count, 7); // total stays in candidate_count
    }

    #[test]
    fn reason_alternatives_carry_prefix_id_label_tuples() {
        let cs = vec![
            cand("app", "firefox", "Firefox Browser", 800.0, MatchedPattern::ExactTitle),
            cand("recent", "fox-doc.md", "Fox doc", 200.0, MatchedPattern::IdSubstring),
        ];
        let (_, r) = assign_band(&cs);
        assert_eq!(r.alternatives[0], ("app".into(), "firefox".into(), "Firefox Browser".into()));
        assert_eq!(r.alternatives[1], ("recent".into(), "fox-doc.md".into(), "Fox doc".into()));
    }

    // ---------- infer_entity (orchestration) ----------
    //
    // These tests use a registry with sources whose `list_cmd` is a literal
    // `echo` of a JSON array — so the bash subprocess returns deterministic
    // items. Real sources call real tools (cos-cli, findmnt, …); we don't
    // exercise those here.

    // `cache_ttl: 0` keeps these orchestration tests hermetic — they exercise
    // the live `list_cmd` path (deterministic `echo`s) without writing to the
    // dev's real `$XDG_RUNTIME_DIR`. The caching path itself is proven in
    // `tests/integration/entity-inference.bats` (witness-file reuse) + the
    // `cache_is_fresh` unit tests below.
    fn fixture_reg() -> Value {
        j!({
            "sources": [
                {
                    "name": "apps", "prefix": "app", "emits": "application/vnd.app",
                    "weight": 1.3, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"firefox\",\"title\":\"Firefox Browser\"},{\"id\":\"thunderbird\",\"title\":\"Thunderbird Mail\"}]'"
                },
                {
                    "name": "recent", "prefix": "recent", "emits": "text/plain",
                    "weight": 1.1, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"fox-recipe.md\",\"title\":\"Fox recipe\"},{\"id\":\"Notes.md\",\"title\":\"Notes.md\"}]'"
                },
                {
                    "name": "hist", "prefix": "hist", "emits": "text/plain",
                    "weight": 0.6, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"14\",\"title\":\"fox news headline\"}]'"
                }
            ]
        })
    }

    #[test]
    fn infer_firefox_is_definitive_silent_resolution() {
        let r = fixture_reg();
        let (subj, band, reason) = infer_entity("firefox", &r, Context::Interactive).unwrap();
        assert_eq!(band, Band::Definitive);
        assert_eq!(subj["id"], "firefox");
        assert_eq!(subj["type"], "application/vnd.app");
        assert_eq!(reason.winner_source, "app");
        assert_eq!(reason.matched_pattern, MatchedPattern::ExactId);
    }

    #[test]
    fn infer_notes_md_is_definitive_via_exact_title() {
        let r = fixture_reg();
        let (subj, band, reason) = infer_entity("Notes.md", &r, Context::Interactive).unwrap();
        assert_eq!(band, Band::Definitive);
        assert_eq!(subj["id"], "Notes.md");
        assert_eq!(reason.winner_source, "recent");
    }

    #[test]
    fn infer_fox_is_medium_picker_territory() {
        // "fox" matches: recent/fox-recipe.md (word-boundary on title "Fox recipe"),
        // hist/14 (title substring "fox news..."), apps doesn't contain "fox".
        // Several candidates with comparable scores → MEDIUM.
        let r = fixture_reg();
        let (_, band, reason) = infer_entity("fox", &r, Context::Interactive).unwrap();
        assert!(matches!(band, Band::Medium | Band::High),
            "expected MEDIUM (or HIGH if margins favor it), got {band:?} from reason {reason:?}");
    }

    #[test]
    fn infer_unmatchable_returns_no_candidates_error() {
        let r = fixture_reg();
        let err = infer_entity("chrome", &r, Context::Interactive).unwrap_err();
        assert!(err.contains("no candidates"), "expected 'no candidates', got {err:?}");
    }

    #[test]
    fn infer_addressing_shape_returns_shape_error() {
        let r = fixture_reg();
        // Caller should never pass these — the shape gate fires first.
        let err = infer_entity(":app/firefox", &r, Context::Interactive).unwrap_err();
        assert!(err.contains("not an inferable shape"));
    }

    #[test]
    fn infer_source_weight_breaks_ties_toward_apps() {
        // apps weight 1.3 makes a same-pattern hit on apps win over a non-apps
        // source. Construct a registry where both have the same id but apps
        // wins by weight.
        let r = j!({
            "sources": [
                {
                    "name": "apps", "prefix": "app", "emits": "x/app",
                    "weight": 1.3, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"thing\",\"title\":\"App thing\"}]'"
                },
                {
                    "name": "hist", "prefix": "hist", "emits": "x/hist",
                    "weight": 0.6, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"thing\",\"title\":\"Hist thing\"}]'"
                }
            ]
        });
        // Both score 1000 raw; apps weighted = 1300, hist weighted = 600.
        // Two candidates at exact-id, but only ONE is at-or-above EXACT_FLOOR
        // after weighting? Both are. Uniqueness rule means NOT DEFINITIVE.
        // What we ARE testing: apps comes first in the sorted list.
        let (subj, _band, reason) = infer_entity("thing", &r, Context::Interactive).unwrap();
        assert_eq!(reason.winner_source, "app");
        assert_eq!(subj["type"], "x/app");
    }

    #[test]
    fn infer_skips_enumerate_false_sources() {
        let r = j!({
            "sources": [
                {
                    "name": "skipped", "prefix": "skip", "emits": "x/skip",
                    "enumerate": false, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"firefox\",\"title\":\"shouldn't see this\"}]'"
                },
                {
                    "name": "apps", "prefix": "app", "emits": "x/app", "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"firefox\",\"title\":\"Firefox\"}]'"
                }
            ]
        });
        let (subj, _, reason) = infer_entity("firefox", &r, Context::Interactive).unwrap();
        assert_eq!(reason.winner_source, "app");
        assert_eq!(subj["type"], "x/app");
    }

    #[test]
    fn infer_inferable_false_overrides_enumerate_true() {
        // `inferable = false` keeps a normally-enumerable source out of
        // inference (the opt-out half of §3.3's participation field).
        let r = j!({
            "sources": [
                {
                    "name": "noisy", "prefix": "noisy", "emits": "x/noisy",
                    "inferable": false, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"firefox\",\"title\":\"shouldn't see this\"}]'"
                },
                {
                    "name": "apps", "prefix": "app", "emits": "x/app", "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"firefox\",\"title\":\"Firefox\"}]'"
                }
            ]
        });
        let (subj, _, reason) = infer_entity("firefox", &r, Context::Interactive).unwrap();
        assert_eq!(reason.winner_source, "app");
        assert_eq!(subj["type"], "x/app");
    }

    #[test]
    fn infer_inferable_true_overrides_enumerate_false() {
        // `inferable = true` opts an `enumerate = false` source back IN — the
        // bluetooth/services case from §3.3. The lone candidate must surface.
        let r = j!({
            "sources": [
                {
                    "name": "bluetooth", "prefix": "bt", "emits": "x/bt",
                    "enumerate": false, "inferable": true, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"earbuds\",\"title\":\"Earbuds\"}]'"
                }
            ]
        });
        let (subj, _, reason) = infer_entity("earbuds", &r, Context::Interactive).unwrap();
        assert_eq!(reason.winner_source, "bt");
        assert_eq!(subj["type"], "x/bt");
    }

    // ---------- cache_is_fresh (pure TTL policy) ----------

    #[test]
    fn cache_freshness_respects_ttl_and_edge_cases() {
        assert!(cache_is_fresh(Some(0), 5), "just-written entry is fresh");
        assert!(cache_is_fresh(Some(4), 5), "within TTL is fresh");
        assert!(!cache_is_fresh(Some(5), 5), "at TTL boundary is stale");
        assert!(!cache_is_fresh(Some(9), 5), "past TTL is stale");
        assert!(!cache_is_fresh(Some(0), 0), "ttl=0 (never-cache) is always stale");
        assert!(!cache_is_fresh(None, 5), "unknown/future mtime is stale, not an error");
    }

    #[test]
    fn infer_context_does_not_change_engine_band() {
        // Per §3.2.3: same band regardless of context; caller maps band+ctx → action.
        let r = fixture_reg();
        let (_, band_tty, _) = infer_entity("firefox", &r, Context::Interactive).unwrap();
        let (_, band_script, _) = infer_entity("firefox", &r, Context::Script).unwrap();
        let (_, band_gui, _) = infer_entity("firefox", &r, Context::Gui).unwrap();
        assert_eq!(band_tty, band_script);
        assert_eq!(band_tty, band_gui);
    }

    // ---------- infer_entity_for_verb (slice 8 / §3.4 verb-aware bias) ----------

    // Two sources emitting distinct types; verbs that accept one or the other
    // let us prove the accepts-filter narrows the pool.
    fn verb_fixture_reg() -> Value {
        j!({
            "sources": [
                {
                    "name": "devices", "prefix": "dev", "emits": "application/vnd.bt.device",
                    "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"foxbuds\",\"title\":\"Foxbuds\"}]'"
                },
                {
                    "name": "recent", "prefix": "recent", "emits": "text/plain",
                    "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"foxnote\",\"title\":\"Foxnote\"}]'"
                }
            ]
        })
    }

    fn verb_accepting(types: &[&str]) -> Value {
        j!({ "name": "v", "accepts": types })
    }

    #[test]
    fn verb_aware_keeps_only_sources_the_verb_accepts() {
        let r = verb_fixture_reg();
        // A verb that only accepts the device type must never surface the
        // recent/text item — even though "fox" substring-matches both.
        let connect = verb_accepting(&["application/vnd.bt.device"]);
        let (subj, _, reason) = infer_entity_for_verb("fox", &r, Context::Interactive, &connect).unwrap();
        assert_eq!(reason.winner_source, "dev");
        assert_eq!(subj["type"], "application/vnd.bt.device");
        // The text source contributed no candidate.
        assert_eq!(reason.candidate_count, 1);
    }

    #[test]
    fn verb_aware_glob_accept_admits_subtype_source() {
        let r = verb_fixture_reg();
        // `text/*` accepts the recent source (text/plain) but not the device.
        let summarize = verb_accepting(&["text/*"]);
        let (subj, _, reason) = infer_entity_for_verb("fox", &r, Context::Interactive, &summarize).unwrap();
        assert_eq!(reason.winner_source, "recent");
        assert_eq!(subj["type"], "text/plain");
        assert_eq!(reason.candidate_count, 1);
    }

    #[test]
    fn verb_aware_no_accepted_source_yields_no_candidates() {
        let r = verb_fixture_reg();
        // A verb accepting a type no source emits → fall-through signal.
        let v = verb_accepting(&["application/vnd.nonesuch"]);
        let err = infer_entity_for_verb("fox", &r, Context::Interactive, &v).unwrap_err();
        assert!(err.contains("no candidates"), "got {err:?}");
    }

    #[test]
    fn verb_aware_respects_participation_gate_over_accepts() {
        // §3.6 privacy guarantee: an `inferable = false` source NEVER enters the
        // scan, even when the verb accepts its emit type. The accepts-filter
        // narrows; it must not widen past the participation gate.
        let r = j!({
            "sources": [
                {
                    "name": "hist", "prefix": "hist", "emits": "text/plain",
                    "inferable": false, "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"foxclip\",\"title\":\"fox clipboard fragment\"}]'"
                },
                {
                    "name": "recent", "prefix": "recent", "emits": "text/plain",
                    "cache_ttl": 0,
                    "list_cmd": "echo '[{\"id\":\"foxnote\",\"title\":\"Foxnote\"}]'"
                }
            ]
        });
        let summarize = verb_accepting(&["text/*"]);
        let (_, _, reason) = infer_entity_for_verb("fox", &r, Context::Interactive, &summarize).unwrap();
        // Only the participating recent source survives — hist is gated out.
        assert_eq!(reason.candidate_count, 1);
        assert_eq!(reason.winner_source, "recent");
    }
}
