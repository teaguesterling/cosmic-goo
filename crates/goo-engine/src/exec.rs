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
use crate::shell::{bash_capture_bytes, bash_exec};
use crate::{mime, template, verbs};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Run the plan for `verb`, the **final step inheriting stdout** (the CLI path).
/// Returns the final command's exit code.
pub fn execute(plan: &Plan, subject_path: &str, verb: &Value, reg: &Value) -> Result<i32, String> {
    Ok(run_pipeline(plan, subject_path, verb, reg, false)?.0)
}

/// Run the plan, **capturing** the final output as **raw bytes** (binary-safe —
/// the byte-sink routing path: `--to`/`-o`).
pub fn execute_capture_bytes(plan: &Plan, subject_path: &str, verb: &Value, reg: &Value) -> Result<Vec<u8>, String> {
    Ok(run_pipeline(plan, subject_path, verb, reg, true)?.1.unwrap_or_default())
}

/// Run the plan, capturing the final output as **text** (lossy from bytes — for
/// tests and text consumers).
pub fn execute_capture(plan: &Plan, subject_path: &str, verb: &Value, reg: &Value) -> Result<String, String> {
    Ok(execute_capture_bytes(plan, subject_path, verb, reg).map(|b| String::from_utf8_lossy(&b).into_owned())?)
}

fn is_present(verb: &Value) -> bool {
    verb.get("kind").and_then(Value::as_str) == Some("present")
}

fn run_pipeline(plan: &Plan, subject_path: &str, verb: &Value, reg: &Value, capture_final: bool) -> Result<(i32, Option<Vec<u8>>), String> {
    // A present verb's A→B step is identity (the subject *is* the result) — drop
    // it. A real verb's step stays and runs its cmd (4b).
    let present = is_present(verb);
    let steps: Vec<&Step> = plan
        .steps
        .iter()
        .filter(|s| !(present && matches!(s.kind, StepKind::Verb(_))))
        .collect();

    // No real steps (pure presentation / identity): deliver the subject itself.
    if steps.is_empty() {
        return deliver_file(subject_path, capture_final);
    }

    let tmp = make_tmpdir()?;
    let mut current = PathBuf::from(subject_path);
    let mut out: (i32, Option<Vec<u8>>) = (0, None);
    let last = steps.len() - 1;
    for (i, step) in steps.iter().enumerate() {
        let rendered = render_step(step, &current, verb, reg)?;
        if i == last && !capture_final {
            out = (bash_exec(&rendered), None); // final → inherit stdout
        } else {
            let captured = bash_capture_bytes(&rendered); // raw bytes — binary-safe
            if i == last {
                out = (0, Some(captured));
            } else {
                let buf = tmp.join(format!("buf{i}")); // buffer between hops
                fs::write(&buf, &captured).map_err(|e| format!("exec: buffer write: {e}"))?;
                current = buf;
            }
        }
    }
    let _ = fs::remove_dir_all(&tmp);
    Ok(out)
}

/// The ready-to-run shell command for a step.
///   - Converter → the channel's `cmd` with `{in.path}` = the current buffer.
///   - Verb (4b) → the verb's `cmd`, rendered against a subject synthesized from
///     the current buffer (`{subject.metadata.path}`, `{subject.text}` if texty).
fn render_step(step: &Step, current: &Path, verb: &Value, reg: &Value) -> Result<String, String> {
    let cur = current.to_string_lossy().into_owned();
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
            Ok(template::substitute(cmd, &json!({ "in": { "path": cur } })))
        }
        StepKind::Verb(inst) => {
            // Synthesize the subject from the current buffer; only read bytes
            // into `text` for a text subtype (don't slurp media).
            let text = if mime::is_subtype(&step.from, "text/*", reg) {
                fs::read_to_string(current).unwrap_or_default()
            } else {
                String::new()
            };
            let subject = json!({
                "type": step.from, "text": text, "id": cur,
                "metadata": { "path": cur }
            });
            if inst.is_empty() {
                // Plain verb: its own cmd.
                verbs::render(reg, verb, &subject, &Value::Null, &json!({}))
                    .map(|r| r.cmd)
                    .map_err(|e| format!("exec: verb render: {e}"))
            } else {
                // A `usage` channel implements the verb (2b): run THE CHANNEL's
                // cmd in the verb's context — accepts/valid_when were cleared at
                // plan time. Convention: a channel as `usage` reads {subject.*}/
                // {verb.*}; auto-plugged as a coercion converter (the Convert arm)
                // it reads {in.path}. v1 splits the two by role; the eventual
                // unification is one context (negotiation §2.3 / goo-protocol §3).
                let cmd = reg
                    .get("channels")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().find(|c| c.get("name").and_then(Value::as_str) == Some(inst.as_str())))
                    .and_then(|c| c.get("cmd").and_then(Value::as_str))
                    .filter(|s| !s.is_empty());
                match cmd {
                    Some(cmd) => Ok(verbs::render_template_in_context(reg, verb, &subject, &Value::Null, &json!({}), cmd).cmd),
                    None => Err(format!("exec: usage channel '{inst}' not found or has no cmd")),
                }
            }
        }
    }
}

fn deliver_file(path: &str, capture: bool) -> Result<(i32, Option<Vec<u8>>), String> {
    if capture {
        let b = fs::read(path).map_err(|e| format!("exec: read subject: {e}"))?;
        Ok((0, Some(b)))
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
    fn present_verb() -> Value {
        json!({ "name": "view", "kind": "present" })
    }
    fn write_subject(body: &str) -> PathBuf {
        // Unique per call — tests run in parallel; keying by body.len() collides.
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let id = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("goo-subj-{}-{id}.txt", std::process::id()));
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
        let out = execute_capture(&plan, subj.to_str().unwrap(), &present_verb(), &reg).unwrap();
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
        let out = execute_capture(&plan, subj.to_str().unwrap(), &present_verb(), &reg).unwrap();
        assert_eq!(out, "CBA");
    }

    // Pure identity (present verb only, no converters): deliver the subject.
    #[test]
    fn identity_delivers_the_subject() {
        let plan = Plan { steps: vec![present_id("text/plain")], delivered: "text/plain".into(), cost: 0 };
        let subj = write_subject("verbatim");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &present_verb(), &json!({})).unwrap();
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
        let err = execute_capture(&plan, subj.to_str().unwrap(), &present_verb(), &reg).unwrap_err();
        assert!(err.contains("has no cmd"), "{err}");
    }

    // 4b: a real (non-present) verb step renders its cmd against the current
    // buffer and runs (`{subject.metadata.path}`).
    #[test]
    fn real_verb_step_runs() {
        let verb = json!({ "name": "up", "accepts": ["text/plain"], "cmd": "tr a-z A-Z < {subject.metadata.path|q}" });
        let plan = Plan {
            steps: vec![step(StepKind::Verb(String::new()), "text/plain", "text/x-up")],
            delivered: "text/x-up".into(),
            cost: 4,
        };
        let subj = write_subject("hello");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &verb, &json!({})).unwrap();
        assert_eq!(out, "HELLO");
    }

    // 4b end-to-end: input coercion (converter) THEN the real verb.
    #[test]
    fn coerces_then_runs_the_verb() {
        let reg = json!({ "channels": [
            { "name": "up", "accepts": ["text/plain"], "emits": "text/x-up", "cmd": "tr a-z A-Z < {in.path|q}" }
        ]});
        let verb = json!({ "name": "rev", "accepts": ["text/x-up"], "cmd": "rev < {subject.metadata.path|q}" });
        let plan = Plan {
            steps: vec![
                step(StepKind::Convert("up".into()), "text/plain", "text/x-up"),
                step(StepKind::Verb(String::new()), "text/x-up", "text/x-rev"),
            ],
            delivered: "text/x-rev".into(),
            cost: 5,
        };
        let subj = write_subject("hello");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &verb, &reg).unwrap();
        assert_eq!(out, "OLLEH"); // up → HELLO, then rev → OLLEH
    }

    // 2b: a chosen `usage` channel implements the verb — run THE CHANNEL's cmd
    // in the verb's context ({subject.text} + {verb.*} both resolve).
    #[test]
    fn usage_channel_step_runs_in_verb_context() {
        let reg = json!({ "channels": [
            { "name": "shout", "accepts": ["text/*"], "emits": "text/x-shout", "cost": "normal",
              "cmd": "printf '%s%s' {subject.text|q} {verb.suffix|q}" }
        ]});
        let verb = json!({ "name": "yell", "accepts": ["text/*"], "suffix": "!!!",
            "usage": ["shout"] });
        let plan = Plan {
            steps: vec![step(StepKind::Verb("shout".into()), "text/plain", "text/x-shout")],
            delivered: "text/x-shout".into(),
            cost: 4,
        };
        let subj = write_subject("hi");
        let out = execute_capture(&plan, subj.to_str().unwrap(), &verb, &reg).unwrap();
        assert_eq!(out, "hi!!!"); // {subject.text}=hi + {verb.suffix}=!!!
    }

    // 2b: a usage channel name that doesn't resolve → a clean error (the
    // replacement for the old "multi-instrument not supported").
    #[test]
    fn usage_channel_not_found_errors() {
        let verb = json!({ "name": "summarize", "accepts": ["text/*"], "usage": ["ghost"] });
        let plan = Plan {
            steps: vec![step(StepKind::Verb("ghost".into()), "text/plain", "text/plain")],
            delivered: "text/plain".into(),
            cost: 4,
        };
        let subj = write_subject("essay");
        let err = execute_capture(&plan, subj.to_str().unwrap(), &verb, &json!({})).unwrap_err();
        assert!(err.contains("usage channel 'ghost' not found"), "{err}");
    }
}
