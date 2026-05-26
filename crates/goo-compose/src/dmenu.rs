//! dmenu-protocol picker for goo-compose.
//!
//! Reads newline-separated candidates, returns the chosen line. Two modes:
//! a **scripted** mode (`GOO_COMPOSE_ANSWERS` names a file; each call pops its
//! first line — for tests/automation), and **live backends** (`fuzzel`/`rofi`/
//! `wofi`/`fzf`, `zenity` as a GTK fallback) auto-detected or forced via
//! `GOO_PICKER`. This is the picker logic deliberately kept out of the `goo`
//! CLI — a front-end's job, not the CLI's.

use std::process::Command;

/// Pick one of `candidates` (newline-separated) with `prompt`. `None` on
/// cancel/empty.
pub fn pick(prompt: &str, candidates: &str) -> Option<String> {
    if let Ok(file) = std::env::var("GOO_COMPOSE_ANSWERS") {
        if std::path::Path::new(&file).is_file() {
            let content = std::fs::read_to_string(&file).unwrap_or_default();
            let mut lines: Vec<&str> = content.lines().collect();
            let ans = if lines.is_empty() { "" } else { lines.remove(0) };
            let rest = lines.join("\n");
            let rest = if rest.is_empty() { String::new() } else { format!("{rest}\n") };
            let _ = std::fs::write(&file, rest);
            return if ans.is_empty() { None } else { Some(ans.to_string()) };
        }
    }
    pick_backend(prompt, candidates)
}

/// yes/no via the picker.
pub fn confirm(prompt: &str) -> bool {
    matches!(pick(prompt, "yes\nno").as_deref(), Some("yes"))
}

fn is_on_path(cmd: &str) -> bool {
    std::env::var("PATH")
        .map(|path| path.split(':').any(|dir| std::path::Path::new(dir).join(cmd).is_file()))
        .unwrap_or(false)
}

/// Run the chosen dmenu-protocol picker, writing `candidates` to its stdin and
/// returning the selected line. `zenity` takes rows as argv instead of stdin.
fn pick_backend(prompt: &str, candidates: &str) -> Option<String> {
    use std::io::Write;
    use std::process::Stdio;

    let backend = std::env::var("GOO_PICKER").ok().filter(|s| !s.is_empty()).or_else(|| {
        ["fuzzel", "rofi", "wofi", "fzf", "zenity"]
            .iter()
            .find(|c| is_on_path(c))
            .map(|s| s.to_string())
    });
    let backend = match backend {
        Some(b) => b,
        None => {
            eprintln!("goo-compose: no picker found (install fuzzel/rofi/wofi/fzf/zenity or set GOO_PICKER)");
            return None;
        }
    };

    let mut cmd = Command::new(&backend);
    match backend.as_str() {
        "fuzzel" => {
            cmd.args(["--dmenu", "--prompt", &format!("{prompt} ❯ ")]);
        }
        "rofi" => {
            cmd.args(["-dmenu", "-i", "-p", prompt]);
        }
        "wofi" => {
            cmd.args(["--dmenu", "--insensitive", "--prompt", prompt]);
        }
        "fzf" => {
            cmd.args([&format!("--prompt={prompt} ❯ "), "--height=40%", "--reverse"]);
        }
        "zenity" => {
            let rows: Vec<&str> = candidates.lines().filter(|l| !l.is_empty()).collect();
            if rows.is_empty() {
                return None;
            }
            cmd.args(["--list", "--title=goo", &format!("--text={prompt}"), &format!("--column={prompt}"), "--hide-header"]);
            cmd.args(&rows);
            let out = cmd.stderr(Stdio::null()).output().ok()?;
            if !out.status.success() {
                return None;
            }
            let sel = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
            return if sel.is_empty() { None } else { Some(sel) };
        }
        other => {
            eprintln!("goo-compose: unknown picker '{other}'");
            return None;
        }
    }

    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(candidates.as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let sel = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string();
    if sel.is_empty() {
        None
    } else {
        Some(sel)
    }
}
