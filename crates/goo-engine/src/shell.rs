//! Shell execution helpers. Library functions (the CLI *and* the future `good`
//! daemon drive them) — they shell out via `bash -c`, like the rest of the
//! engine's effectful surface (`mime::detect_path`, the jq predicate eval).

use std::process::Command;

/// Run `bash -c <cmd>` inheriting stdio (the command's output flows through to
/// the caller's stdout/stderr). Returns the child's exit code.
pub fn bash_exec(cmd: &str) -> i32 {
    match Command::new("bash").arg("-c").arg(cmd).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 1,
    }
}

/// Run `bash -c <cmd>`, capturing stdout (for `list` / handle search / the
/// negotiation executor's intermediate hops).
pub fn bash_capture(cmd: &str) -> String {
    Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}
