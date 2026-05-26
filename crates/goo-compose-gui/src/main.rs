//! goo-compose-gui (v1) — the native compose dialog over the `goo` CLI (iced).
//!
//! Same backend contract as the v0 dmenu `goo-compose`: read-only data comes
//! from `goo-engine` in-process; the action will exec `goo <verb> <addr> …`.
//! The v0 bin stays the headless/scripted **test surface**; this is the GUI.
//!
//! **First increment (this commit):** open a window, load the registry, and list
//! the subject candidates as canonical `goo://<domain>/<id>` lines. No verb pick
//! or exec yet — that's the next increment. The MVU bones (Message/update/view)
//! and the engine link carry over to the eventual **libcosmic** swap (libcosmic
//! widgets are iced widgets with COSMIC styling).
//!
//! Built only on demand (`cargo build -p goo-compose-gui`) — it's a workspace
//! member but not a default-member, so it never slows the core build.

use goo_engine::{registry, selection};
use iced::widget::{column, scrollable, text};
use iced::{Element, Task};
use serde_json::Value;

fn main() -> iced::Result {
    iced::run(update, view)
}

struct App {
    candidates: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        App {
            candidates: subject_candidates(),
        }
    }
}

// No interactions yet — the first increment only displays.
#[derive(Debug, Clone)]
enum Message {}

fn update(_app: &mut App, message: Message) -> Task<Message> {
    match message {}
}

fn view(app: &App) -> Element<'_, Message> {
    let mut col = column![text("goo-compose · subjects").size(24)].spacing(6);
    for c in &app.candidates {
        col = col.push(text(c.clone()));
    }
    scrollable(col).into()
}

/// Subject candidates as canonical `goo://<domain>/<id>` lines: the implicit
/// selection / clipboard first, then items from enumerable prefixed sources.
fn subject_candidates() -> Vec<String> {
    let mut out = Vec::new();
    let trunc = |s: &str| s.chars().take(60).collect::<String>();

    let sel = selection::primary();
    if !sel.is_empty() {
        out.push(format!("goo://sel/        selection: {}", trunc(&sel)));
    }
    let clip = selection::clipboard();
    if !clip.is_empty() {
        out.push(format!("goo://clip/       clipboard: {}", trunc(&clip)));
    }

    let reg = registry::load_all();
    if let Some(sources) = reg.get("sources").and_then(|s| s.as_array()) {
        for source in sources {
            if source.get("enumerate").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let name = source.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name == "selection" || name == "clipboard" {
                continue;
            }
            let prefix = match source.get("prefix").and_then(|p| p.as_str()).filter(|s| !s.is_empty()) {
                Some(p) => p,
                None => continue,
            };
            let lc = match source.get("list_cmd").and_then(|c| c.as_str()).filter(|s| !s.is_empty()) {
                Some(l) => l,
                None => continue,
            };
            let items: Vec<Value> = serde_json::from_str(bash_capture(lc).trim()).unwrap_or_default();
            for it in items {
                let id = it.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let title = it.get("title").and_then(|t| t.as_str()).unwrap_or(id);
                out.push(format!("goo://{prefix}/{id}       {title} ({name})"));
            }
        }
    }

    if out.is_empty() {
        out.push("(no subject candidates — select some text or check your plugins)".into());
    }
    out
}

/// `bash -c <cmd>` capturing stdout (a source's `list_cmd`).
fn bash_capture(cmd: &str) -> String {
    std::process::Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}
