//! Implicit-subject capture from the Wayland clipboard / PRIMARY selection —
//! the Rust port of `lib/selection.sh`. Both shell out to `wl-paste` and yield
//! the empty string when there's nothing (or no compositor), never an error.

use std::process::Command;

/// The PRIMARY selection (middle-click buffer): `wl-paste --primary --no-newline`.
pub fn primary() -> String {
    wl_paste(&["--primary", "--no-newline"])
}

/// The clipboard: `wl-paste --no-newline`.
pub fn clipboard() -> String {
    wl_paste(&["--no-newline"])
}

fn wl_paste(args: &[&str]) -> String {
    Command::new("wl-paste")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}
