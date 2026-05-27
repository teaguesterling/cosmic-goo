//! The negotiation plan executor (slice 4).
//!
//! Runs a [`negotiation::Plan`] hop by hop, threading the subject's *value*
//! (a file on disk in v1) through each converter's `cmd`. The planner deals in
//! *types*; this is where types become bytes. Impure (shells out via
//! [`crate::shell`], writes temp-file buffers) — the executor side of the
//! engine, like `mime::detect_path`.
//!
//! v1 boundaries (doc/design/negotiation.md §5 "Executor v1 boundaries"):
//!   - the **initial value is the caller-supplied subject** path; buffering
//!     starts at the *first converter's output*, never the input;
//!   - **intermediate steps capture stdout** into a temp buffer; the **final
//!     step inherits stdout** (so a terminal-aware converter like `chafa` sees a
//!     tty) — `execute`; tests capture the final output via `execute_capture`;
//!   - a **present-verb identity step is elided explicitly** (`from == to`); a
//!     non-present step missing a `cmd`, or a real-verb step in the pipeline, is
//!     a v1 **error to surface**, not a silent skip (real-verb-in-pipeline is
//!     slice 4b).

use crate::negotiation::{Plan, Step, StepKind};
use crate::shell::{bash_capture, bash_exec};
use crate::template;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// Run the plan, the **final step inheriting stdout** (the CLI path). Returns
/// the final command's exit code.
pub fn execute(plan: &Plan, subject_path: &str, reg: &Value) -> Result<i32, String> {
    Ok(run_pipeline(plan, subject_path, reg, false)?.0)
}

/// Run the plan, **capturing** the final output instead of inheriting stdout
/// (for tests and non-terminal consumers). Returns the delivered bytes as text.
pub fn execute_capture(plan: &Plan, subject_path: &str, reg: &Value) -> Result<String, String> {
    Ok(run_pipeline(plan, subject_path, reg, true)?.1.unwrap_or_default())
}

/// A present-verb identity step (`from == to`): the subject is the result, no
/// command. Elided explicitly — *not* via "empty cmd" (that's a real-verb bug).
fn is_identity_verb(s: &Step) -> bool {
    matches!(s.kind, StepKind::Verb(_)) && s.from == s.to
}

fn run_pipeline(plan: &Plan, subject_path: &str, reg: &Value, capture_final: bool) -> Result<(i32, Option<String>), String> {
    let steps: Vec<&Step> = plan.steps.iter().filter(|s| !is_identity_verb(s)).collect();

    // No real steps (pure presentation / identity): deliver the subject itself.
    if steps.is_empty() {
        return deliver_file(subject_path, capture_final);
    }

    let tmp = make_tmpdir()?;
    let mut current = PathBuf::from(subject_path);
    let mut out = (0, None);
    let last = steps.len() - 1;
    for (i, step) in steps.iter().enumerate() {
        let cmd = step_cmd(step, reg)?;
        let ctx = json!({ "in": { "path": current.to_string_lossy() } });
        let rendered = template::substitute(&cmd, &ctx);
        if i == last && !capture_final {
            out = (bash_exec(&rendered), None); // final → inherit stdout
        } else {
            let captured = bash_capture(&rendered);
            if i == last {
                out = (0, Some(captured));
            } else {
                let buf = tmp.join(format!("buf{i}")); // buffer between hops
                fs::write(&buf, captured.as_bytes()).map_err(|e| format!("exec: buffer write: {e}"))?;
                current = buf;
            }
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    Ok(out)
}

/// The shell command for a step. Converter → the channel's `cmd`. A non-identity
/// Verb step (a real verb in the pipeline) is unsupported in v1 — surfaced.
fn step_cmd(step: &Step, reg: &Value) -> Result<String, String> {
    match &step.kind {
        StepKind::Convert(name) => {
            let cmd = reg
                .get("channels")
                .and_then(Value::as_array)
                .and_then(|a| a.iter().find(|c| c.get("name").and_then(Value::as_str) == Some(name)))
                .and_then(|c| c.get("cmd").and_then(Value::as_str))
                .unwrap_or("");
            if cmd.is_empty() {
                return Err(format!("exec: converter '{name}' has no cmd"));
            }
            Ok(cmd.to_string())
        }
        StepKind::Verb(inst) => Err(format!(
            "exec: real-verb pipeline execution not supported in v1 (verb '{}', {} → {})",
            if inst.is_empty() { "<unnamed>" } else { inst },
            step.from, step.to
        )),
    }
}

fn deliver_file(path: &str, capture: bool) -> Result<(i32, Option<String>), String> {
    if capture {
        let s = fs::read_to_string(path).map_err(|e| format!("exec: read subject: {e}"))?;
        Ok((0, Some(s)))
    } else {
        // Stream the bytes to the inherited stdout (a byte sink, e.g. a pipe).
        Ok((bash_exec(&format!("cat -- {}", shell_quote(path))), None))
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn make_tmpdir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!(
        "goo-exec-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    fs::create_dir_all(&dir).map_err(|e| format!("exec: tmpdir: {e}"))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(kind: StepKind, from: &str, to: &str) -> Step {
        Step { kind, from: from.into(), to: to.into() }
    }
    fn present_id(t: &str) -> Step {
        step(StepKind::Verb(String::new()), t, t)
    }
    fn write_subject(body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("goo-subj-{}-{}.txt", std::process::id(), body.len()));
        fs::write(&p, body).unwrap();
        p
    }

    // A present verb + one output converter: subject → [upper] → delivered.
    #[test]
    fn runs_a_converter_after_a_present_verb() {
        let reg = json!({ "channels": [
            { "name": "upper", "accepts": ["text/plain"], "emits": "text/x-upper", "cmd": "tr a-z A-Z < {in.path|q}" }
        ]});
        let plan = Plan {
            steps: vec![present_id("text/plain"), step(StepKind::Convert("upper".into()), "text/plain", "text/x-upper")],
            delivered: "text/x-upper".into(),
            cost: 1,
        };
        let subj = write_subject("hello world");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &reg).unwrap();
        assert_eq!(out, "HELLO WORLD");
    }

    // Two converters chain through a temp buffer (upper, then reverse).
    #[test]
    fn chains_converters_through_a_buffer() {
        let reg = json!({ "channels": [
            { "name": "upper", "accepts": ["text/plain"], "emits": "text/x-upper", "cmd": "tr a-z A-Z < {in.path|q}" },
            { "name": "rev",   "accepts": ["text/x-upper"], "emits": "text/x-rev",  "cmd": "rev < {in.path|q}" }
        ]});
        let plan = Plan {
            steps: vec![
                present_id("text/plain"),
                step(StepKind::Convert("upper".into()), "text/plain", "text/x-upper"),
                step(StepKind::Convert("rev".into()), "text/x-upper", "text/x-rev"),
            ],
            delivered: "text/x-rev".into(),
            cost: 2,
        };
        let subj = write_subject("abc");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &reg).unwrap();
        assert_eq!(out, "CBA");
    }

    // Pure identity (present verb only, no converters): deliver the subject.
    #[test]
    fn identity_delivers_the_subject() {
        let plan = Plan { steps: vec![present_id("text/plain")], delivered: "text/plain".into(), cost: 0 };
        let subj = write_subject("verbatim");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &json!({})).unwrap();
        assert_eq!(out, "verbatim");
    }

    // A converter with no cmd is a surfaced error, not a silent skip.
    #[test]
    fn missing_converter_cmd_errors() {
        let reg = json!({ "channels": [ { "name": "x", "accepts": ["text/plain"], "emits": "text/y" } ] });
        let plan = Plan {
            steps: vec![present_id("text/plain"), step(StepKind::Convert("x".into()), "text/plain", "text/y")],
            delivered: "text/y".into(),
            cost: 0,
        };
        let subj = write_subject("z");
        let err = execute_capture(&plan, subj.to_str().unwrap(), &reg).unwrap_err();
        assert!(err.contains("has no cmd"), "{err}");
    }

    // A real (non-identity) verb step in the pipeline is a v1 error to surface.
    #[test]
    fn real_verb_step_is_unsupported_in_v1() {
        let plan = Plan {
            steps: vec![step(StepKind::Verb("summarize".into()), "text/plain", "text/x-summary")],
            delivered: "text/x-summary".into(),
            cost: 4,
        };
        let subj = write_subject("essay");
        let err = execute_capture(&plan, subj.to_str().unwrap(), &json!({})).unwrap_err();
        assert!(err.contains("not supported in v1"), "{err}");
    }
}
