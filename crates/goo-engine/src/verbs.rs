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

use serde_json::Value;
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
    use super::satisfies;
    use serde_json::json;

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
