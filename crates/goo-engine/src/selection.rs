//! Implicit-subject capture from the Wayland clipboard / PRIMARY selection —
//! the Rust port of `lib/selection.sh`. Both shell out to `wl-paste` and yield
//! the empty string when there's nothing (or no compositor), never an error.

use std::io::Write;
use std::process::{Command, Stdio};

/// The PRIMARY selection (middle-click buffer): `wl-paste --primary --no-newline`.
pub fn primary() -> String {
    wl_paste(&["--primary", "--no-newline"])
}

/// The clipboard: `wl-paste --no-newline`.
pub fn clipboard() -> String {
    wl_paste(&["--no-newline"])
}

/// Write `bytes` to the Wayland clipboard via `wl-copy` (the write counterpart of
/// [`clipboard`]). v1 is text-shaped; MIME-tagged (image) copies (`wl-copy --type`)
/// are deferred. See doc/design/goo-protocol.md §12.
pub fn set_clipboard(bytes: &[u8]) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wl-copy: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("wl-copy: no stdin")?
        .write_all(bytes)
        .map_err(|e| format!("wl-copy: {e}"))?;
    let status = child.wait().map_err(|e| format!("wl-copy: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("wl-copy: failed".into())
    }
}

fn wl_paste(args: &[&str]) -> String {
    Command::new("wl-paste")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Bounded peek of the PRIMARY selection for interactive *previews* (tab
/// completion): `timeout <secs> wl-paste --primary`. Unlike [`primary`], a hung
/// or absent compositor can't stall the caller — it degrades to "" after `secs`.
pub fn peek_primary_timed(secs: &str) -> String {
    wl_paste_timed(secs, &["--primary", "--no-newline"])
}

/// Bounded peek of the clipboard — the [`peek_primary_timed`] counterpart.
pub fn peek_clipboard_timed(secs: &str) -> String {
    wl_paste_timed(secs, &["--no-newline"])
}

/// `timeout <secs> wl-paste <args>` → stdout, or "" on timeout / missing tool /
/// no compositor. `secs` is handed to coreutils `timeout` (e.g. "0.15").
fn wl_paste_timed(secs: &str, args: &[&str]) -> String {
    Command::new("timeout")
        .arg(secs)
        .arg("wl-paste")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}
