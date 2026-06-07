//! goo-compose-gui — the native **noun-first** compose dialog over the `goo` CLI
//! (iced). Pick a subject, then a verb, see the exact `goo …` command it will
//! run, and run it. The CLI stays verb-first; this GUI inverts the grammar
//! ("I have this thing — what can I do with it?").
//!
//! **Thin shell.** All the logic lives in the pure, unit-tested
//! [`goo_engine::compose`] core ([`ComposeState`]); this file is the iced
//! `update`/`view` over it plus the I/O the core deliberately excludes:
//! resolving the picked address, reading the action history for the recency
//! reorder, and spawning the assembled command. The scripted `goo compose` CLI
//! drives the same core, so the bats suite tests it headlessly.
//!
//! **Increment 1** (this revision): subject pane → verb pane (OPTIONS.allow,
//! recency-reordered, with confirm/destructive chips) → live CLI-equivalent
//! preview → confirm pane → Run (spawns `goo argv`, propagating the exit code;
//! a confirm/destructive verb is run with `--confirm-dangerous=<verb>` since a
//! spawned `goo` has no stdin for the y/N gate). Object-needing and
//! adverb-slot verbs are recognised but their panels are increment 2 — an
//! object-needing verb shows Run disabled rather than emitting a broken command.
//!
//! Built on demand (`cargo build -p goo-compose-gui` / `make build-gui`) — a
//! workspace member but not a default-member, so it never slows the core build.
//! The MVU bones port mechanically to the eventual libcosmic swap.

use goo_engine::compose::ComposeState;
use goo_engine::{address, history, registry, selection};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Color, Element, Length, Task, Theme};
use serde_json::Value;

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title(|_: &App| "cosmic-goo · compose".to_string())
        .theme(|_: &App| Theme::Dark)
        .window_size((720.0, 640.0))
        .run()
}

// ============================================================================
// State
// ============================================================================

/// An addressable subject candidate: the canonical `goo://…` address plus a
/// human label. The address is what gets resolved/threaded into the command;
/// the label is display-only.
struct Candidate {
    addr: String,
    label: String,
}

struct App {
    reg: Value,
    candidates: Vec<Candidate>,
    /// The in-progress sentence once a subject is picked (`None` = subject stage).
    state: Option<ComposeState>,
    /// Verbs recently run on the picked subject's type (recency reorder source).
    recent: Vec<String>,
    /// A transient error (e.g. an address that wouldn't resolve).
    error: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        let reg = registry::load_all();
        let candidates = subject_candidates(&reg);
        App { reg, candidates, state: None, recent: Vec::new(), error: None }
    }
}

#[derive(Debug, Clone)]
enum Message {
    SubjectPicked(String), // canonical address
    VerbPicked(String),    // verb name
    Run,                   // execute the assembled sentence
    Back,                  // verb → subject stage (or clear the verb)
    Cancel,                // abort the dialog (exit 130)
}

// ============================================================================
// Update
// ============================================================================

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SubjectPicked(addr) => match address::resolve(&addr, &self.reg, None) {
                Ok(subject) => {
                    let st = ComposeState::from_subject(&self.reg, &subject, addr);
                    // Recency reorder source — read the history here (the core stays
                    // I/O-free) and pass it into `verb_menu` at view time.
                    self.recent = history::recent_verbs_for_type(&st.subject_type, 16);
                    self.state = Some(st);
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("could not resolve {addr}: {e}")),
            },
            Message::VerbPicked(name) => {
                if let Some(st) = self.state.as_mut() {
                    st.select_verb(&name);
                }
            }
            Message::Back => {
                match self.state.as_mut() {
                    // verb picked → drop it, back to the verb pane
                    Some(st) if st.verb.is_some() => st.verb = None,
                    // only a subject → back to the subject pane
                    _ => self.state = None,
                }
                self.error = None;
            }
            Message::Cancel => std::process::exit(130),
            Message::Run => {
                if let Some(st) = self.state.as_ref() {
                    if st.needs_object() {
                        return Task::none(); // guarded in the view; belt-and-braces
                    }
                    std::process::exit(run_sentence(st));
                }
            }
        }
        Task::none()
    }

    // ========================================================================
    // View
    // ========================================================================

    fn view(&self) -> Element<'_, Message> {
        let body: Element<_> = match &self.state {
            None => self.view_subjects(),
            Some(st) if st.verb.is_none() => self.view_verbs(st),
            Some(st) => self.view_review(st),
        };
        let mut col = column![text("goo-compose · noun-first").size(22)].spacing(10);
        if let Some(e) = &self.error {
            col = col.push(text(format!("⚠ {e}")).size(13).color(Color::from_rgb(0.95, 0.5, 0.4)));
        }
        col = col.push(body);
        container(col).padding(16).into()
    }

    /// Stage 1 — pick the subject.
    fn view_subjects(&self) -> Element<'_, Message> {
        let mut list = column![text("Pick a subject:").size(14)].spacing(4);
        for c in &self.candidates {
            let b = button(
                row![
                    text(c.addr.clone()).size(13).font(iced::Font::MONOSPACE).width(Length::Fixed(220.0)),
                    text(c.label.clone()).size(13).color(Color::from_rgb(0.6, 0.64, 0.83)),
                ]
                .spacing(8),
            )
            .on_press(Message::SubjectPicked(c.addr.clone()))
            .padding([5u16, 10])
            .width(Length::Fill)
            .style(button::text);
            list = list.push(b);
        }
        scrollable(list).height(Length::Fill).into()
    }

    /// Stage 2 — pick the verb (recency-reordered, with confirm/destructive chips).
    fn view_verbs<'a>(&'a self, st: &'a ComposeState) -> Element<'a, Message> {
        let header = text(format!("{}   ({})", st.subject_addr, st.subject_type))
            .size(13)
            .font(iced::Font::MONOSPACE)
            .color(Color::from_rgb(0.6, 0.64, 0.83));

        let mut list = column![text("What do you want to do?").size(14)].spacing(4);
        for name in st.verb_menu(&self.recent) {
            let (confirm, destructive) = st.verb_flags(&name);
            let chip = chip_for(confirm, destructive);
            let recent_mark = if self.recent.iter().any(|r| *r == name) { "  ·recent" } else { "" };
            let label = format!("{name}{chip}{recent_mark}");
            let b = button(text(label).size(14).font(iced::Font::MONOSPACE))
                .on_press(Message::VerbPicked(name.clone()))
                .padding([5u16, 10])
                .width(Length::Fill)
                .style(if destructive { button::danger } else { button::text });
            list = list.push(b);
        }

        column![
            header,
            Space::new().height(Length::Fixed(8.0)),
            scrollable(list).height(Length::Fill),
            nav_row(Message::Back, "‹ subject"),
        ]
        .spacing(8)
        .into()
    }

    /// Stage 3 — review the assembled sentence and run it.
    fn view_review<'a>(&'a self, st: &'a ComposeState) -> Element<'a, Message> {
        // The live CLI-equivalent ("speak it back"): exactly what will run.
        let preview = container(
            text(st.preview()).size(15).font(iced::Font::MONOSPACE).color(Color::from_rgb(0.55, 0.85, 0.6)),
        )
        .padding(10)
        .width(Length::Fill)
        .style(panel_style);

        let mut col = column![text("Run this command:").size(14), preview].spacing(8);

        if st.needs_object() {
            // Object pane is increment 2 — never emit a broken command.
            col = col.push(
                text(format!(
                    "‘{}’ needs an object ({}) — the object picker arrives in increment 2.",
                    st.verb.as_deref().unwrap_or(""),
                    st.object_type().unwrap_or(""),
                ))
                .size(13)
                .color(Color::from_rgb(0.95, 0.7, 0.35)),
            );
        } else if st.needs_confirm() || st.is_destructive() {
            // The GUI's own confirm gate: a spawned `goo` has no stdin for the
            // y/N prompt, so clicking Run here IS the confirmation (the verb is
            // then run with --confirm-dangerous=<verb>). Gates on destructive too,
            // so a `[!!]` verb always warns even if it didn't set `confirm`.
            let warn = if st.is_destructive() {
                "⚠ destructive — this cannot be undone. Click Run to confirm."
            } else {
                "⚠ this verb asks for confirmation. Click Run to confirm."
            };
            col = col.push(text(warn).size(13).color(Color::from_rgb(0.95, 0.5, 0.4)));
        }

        // Run is enabled only when the sentence is complete (inc 1: no object gap).
        let run = button(text(if st.is_destructive() { "Run  [!!]" } else { "Run" }).size(14))
            .on_press_maybe((!st.needs_object()).then_some(Message::Run))
            .padding([6u16, 16])
            .style(if st.is_destructive() { button::danger } else { button::primary });

        let nav = row![
            run,
            Space::new().width(Length::Fixed(8.0)),
            button(text("‹ verb").size(13)).on_press(Message::Back).padding([6u16, 12]).style(button::text),
            Space::new().width(Length::Fill),
            button(text("Cancel").size(13)).on_press(Message::Cancel).padding([6u16, 12]).style(button::text),
        ]
        .align_y(iced::Alignment::Center);

        col.push(Space::new().height(Length::Fixed(8.0))).push(nav).spacing(8).into()
    }
}

// ============================================================================
// Helpers (the I/O the pure core excludes)
// ============================================================================

/// Spawn `goo` with the sentence's argv and return its exit code. A
/// confirm/destructive verb gets `--confirm-dangerous=<verb>` appended (NOT part
/// of `argv()`/the preview — it's the gate bypass the GUI's confirm pane earns).
fn run_sentence(st: &ComposeState) -> i32 {
    let mut argv = st.argv();
    // Append the gate bypass for ANY gated verb. The CLI prompt currently keys on
    // `confirm` only, but OPTIONS surfaces `confirm`/`destructive` independently,
    // so a future `destructive:true, confirm:false` verb would also need it —
    // including `|| is_destructive()` is harmless today (the flag is a no-op when
    // the CLI wouldn't prompt) and closes that latent gap.
    if st.needs_confirm() || st.is_destructive() {
        if let Some(v) = st.verb.as_deref() {
            argv.push(format!("--confirm-dangerous={v}"));
        }
    }
    let goo = std::env::var("GOO_BIN").unwrap_or_else(|_| "goo".to_string());
    match std::process::Command::new(&goo).args(&argv).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("goo-compose-gui: failed to exec {goo}: {e}");
            1
        }
    }
}

/// The confirm/destructive chip (the `completion-polish.md` §2 vocabulary).
fn chip_for(confirm: bool, destructive: bool) -> &'static str {
    if destructive {
        "  [!!]"
    } else if confirm {
        "  [!]"
    } else {
        ""
    }
}

/// A `‹ back`-style nav button row.
fn nav_row(msg: Message, label: &str) -> Element<'_, Message> {
    row![
        button(text(label.to_string()).size(13)).on_press(msg).padding([6u16, 12]).style(button::text),
        Space::new().width(Length::Fill),
        button(text("Cancel").size(13)).on_press(Message::Cancel).padding([6u16, 12]).style(button::text),
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.weak.color.into()),
        border: iced::Border { radius: 8.0.into(), width: 1.0, color: palette.background.strong.color },
        ..Default::default()
    }
}

/// Subject candidates as canonical `goo://<domain>/<id>` addresses + labels: the
/// implicit selection / clipboard first, then items from enumerable prefixed
/// sources. Mirrors the v0 dmenu `goo-compose` candidate set.
fn subject_candidates(reg: &Value) -> Vec<Candidate> {
    let mut out = Vec::new();
    let trunc = |s: &str| s.chars().take(60).collect::<String>();

    let sel = selection::primary();
    if !sel.is_empty() {
        out.push(Candidate { addr: "goo://sel/".into(), label: format!("selection: {}", trunc(&sel)) });
    }
    let clip = selection::clipboard();
    if !clip.is_empty() {
        out.push(Candidate { addr: "goo://clip/".into(), label: format!("clipboard: {}", trunc(&clip)) });
    }

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
                out.push(Candidate {
                    addr: format!("goo://{prefix}/{id}"),
                    label: format!("{title} ({name})"),
                });
            }
        }
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
