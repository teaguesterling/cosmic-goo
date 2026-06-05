//! Persistent action history — the backing store for `goo again` (§6.1) and,
//! later, recent-action completion bias (§6.3). One append-only JSONL file at
//! `$XDG_STATE_HOME/cosmic-goo/history.jsonl` (this is *state*, not cache: it
//! survives a reboot, unlike the entity cache under `$XDG_RUNTIME_DIR`).
//!
//! Each record carries the resolved verb, the subject's TYPE, and the selector
//! adverbs — never the subject's id/text (no content), and no timestamp (append
//! order already encodes recency). Recording is on by default; `GOO_NO_HISTORY`
//! disables it and [`clear`] (the `goo forget` subcommand) drops the file.
//!
//! **Concurrency.** An append is a single `write()` of one sub-`PIPE_BUF` line
//! under `O_APPEND`, which the kernel serialises across the many concurrent
//! `goo` processes — no read-modify-write, no lock, no truncate-rewrite race
//! (the lesson carried over from the entity cache's mtime model). Reading never
//! mutates the file, so it can't race a writer either.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};

/// One recorded action. `adverbs` is a `{name: value}` map that the CALLER has
/// already filtered to declared `kind = "selector"` adverbs only — enumerated,
/// content-free, behaviour-defining. Run-control flags (`to`, `using`, `hops`,
/// `confirm-dangerous`, …) that the arg parser also folds into the adverb map
/// are NOT persisted (path leaks, and replaying `confirm-dangerous` would skip a
/// safety gate). The store itself is dumb: it records whatever map it is given.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub verb: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub adverbs: Value,
}

/// The history file path, or `None` when no state dir can be located:
/// `$XDG_STATE_HOME/cosmic-goo/history.jsonl`, else `~/.local/state/...`.
fn history_path() -> Option<std::path::PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        Some(x) => std::path::PathBuf::from(x),
        None => {
            let home = std::env::var_os("HOME").filter(|s| !s.is_empty())?;
            std::path::PathBuf::from(home).join(".local/state")
        }
    };
    Some(base.join("cosmic-goo").join("history.jsonl"))
}

/// Recording disabled via `GOO_NO_HISTORY` (set to anything).
fn disabled() -> bool {
    std::env::var_os("GOO_NO_HISTORY").is_some()
}

/// Append one action. Best-effort and silent: history is a convenience, never a
/// reason to fail — or even warn about — the command the user actually ran.
/// No-op when disabled, when the verb is empty, or when the line would exceed
/// the atomic-append budget.
pub fn record(verb: &str, type_: &str, adverbs: &Value) {
    if disabled() || verb.is_empty() {
        return;
    }
    let Some(path) = history_path() else { return };
    let action = Action { verb: verb.to_string(), type_: type_.to_string(), adverbs: adverbs.clone() };
    let Ok(mut line) = serde_json::to_string(&action) else { return };
    line.push('\n');
    // Beyond PIPE_BUF (4096 on Linux) a write is no longer atomic against
    // concurrent appenders — skip rather than risk an interleaved record.
    if line.len() > 4096 {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// The most recent recorded action, or `None` if history is empty/absent.
/// Returns the last well-formed line, tolerating a stray bad line or a partial
/// trailing write from a crashed appender.
pub fn last() -> Option<Action> {
    let path = history_path()?;
    let f = std::fs::File::open(&path).ok()?;
    let mut found = None;
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(a) = serde_json::from_str::<Action>(&line) {
            found = Some(a);
        }
    }
    found
}

/// The distinct verbs most recently run on a subject of `type_`, most-recent
/// first, capped at `n`. Backs the `goo what` recency hint (§6.3) and is the
/// query the compose-GUI verb menu (#9) will reuse. Reads the whole file (it
/// stays small; a lazy tail-read is a future optimisation). Empty type → empty.
pub fn recent_verbs_for_type(type_: &str, n: usize) -> Vec<String> {
    if type_.is_empty() || n == 0 {
        return Vec::new();
    }
    let Some(path) = history_path() else { return Vec::new() };
    let Ok(f) = std::fs::File::open(&path) else { return Vec::new() };
    // Gather the matching verbs in chronological (append) order…
    let mut chrono: Vec<String> = Vec::new();
    for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
        if let Ok(a) = serde_json::from_str::<Action>(&line) {
            if a.type_ == type_ {
                chrono.push(a.verb);
            }
        }
    }
    // …then walk from the most recent, keeping first-seen (= latest) of each.
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in chrono.into_iter().rev() {
        if seen.insert(v.clone()) {
            out.push(v);
            if out.len() >= n {
                break;
            }
        }
    }
    out
}

/// Drop the history file (the `goo forget` subcommand). `true` if a file was
/// actually removed.
pub fn clear() -> bool {
    match history_path() {
        Some(path) => std::fs::remove_file(&path).is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    // XDG_STATE_HOME / GOO_NO_HISTORY are process-global and cargo runs tests in
    // parallel threads, so every test that touches them holds this lock for its
    // whole body — serialising env access. Poison-tolerant: a panicking test
    // shouldn't cascade into spurious failures in the rest.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fresh_state(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("goo-hist-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_STATE_HOME", &dir);
        std::env::remove_var("GOO_NO_HISTORY");
        dir
    }

    #[test]
    fn record_then_last_roundtrips() {
        let _g = lock();
        let dir = fresh_state("roundtrip");
        record("summarize", "text/plain", &json!({"via": "fabric"}));
        let a = last().expect("an action");
        assert_eq!(a.verb, "summarize");
        assert_eq!(a.type_, "text/plain");
        assert_eq!(a.adverbs, json!({"via": "fabric"}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_returns_the_most_recent() {
        let _g = lock();
        let dir = fresh_state("recency");
        record("summarize", "text/plain", &json!({}));
        record("critique", "text/plain", &json!({}));
        assert_eq!(last().unwrap().verb, "critique");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_records_nothing() {
        let _g = lock();
        let dir = fresh_state("disabled");
        std::env::set_var("GOO_NO_HISTORY", "1");
        record("summarize", "text/plain", &json!({}));
        std::env::remove_var("GOO_NO_HISTORY");
        assert!(last().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_drops_history() {
        let _g = lock();
        let dir = fresh_state("clear");
        record("summarize", "text/plain", &json!({}));
        assert!(last().is_some());
        assert!(clear());
        assert!(last().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_verbs_dedupes_latest_first_per_type() {
        let _g = lock();
        let dir = fresh_state("recent");
        record("summarize", "text/plain", &json!({}));
        record("critique", "text/plain", &json!({}));
        record("summarize", "text/plain", &json!({})); // re-run → moves summarize ahead
        record("open", "inode/file", &json!({})); // different type → excluded
        // Most-recent-first, deduped: summarize (latest), then critique.
        assert_eq!(recent_verbs_for_type("text/plain", 8), vec!["summarize", "critique"]);
        // Type filter holds; cap honoured.
        assert_eq!(recent_verbs_for_type("inode/file", 8), vec!["open"]);
        assert_eq!(recent_verbs_for_type("text/plain", 1), vec!["summarize"]);
        assert!(recent_verbs_for_type("text/x-none", 8).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_line_is_skipped_not_fatal() {
        let _g = lock();
        let dir = fresh_state("badline");
        record("summarize", "text/plain", &json!({}));
        // Append garbage, then a good record — last() must skip the garbage.
        let path = history_path().unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"not json at all\n").unwrap();
        record("critique", "text/plain", &json!({}));
        assert_eq!(last().unwrap().verb, "critique");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
