//! goo-compose-gui — the native **noun-first** compose dialog over the `goo` CLI
//! (iced), in a gnome-do / Kupfer idiom: pick a subject, then a verb, then (when
//! the verb needs one) an object, type-to-filter at every step, and run the exact
//! `goo …` command shown live at the bottom.
//!
//! **Thin shell.** Every bit of interaction logic — the pane state machine, the
//! query editing, the selection movement, the commit/advance/back transitions,
//! and the sentence itself — lives in the pure, unit-tested
//! [`goo_engine::compose`] reducer ([`ComposeUi`] over [`ComposeState`]). This
//! file is the iced `view`/`update`/`subscription` plus the *only* I/O the
//! reducer hands back as [`UiAction`]s: resolving the picked address, enumerating
//! object candidates, and spawning the command. So the whole keyboard flow is
//! tested headlessly in `compose.rs` — no display required.
//!
//! **Input model (gnome-do):** nothing is focused; every keypress is captured
//! globally via `event::listen_with` and fed to `ComposeUi::apply`. Type to
//! filter, ↑/↓ to move, Enter/Tab to pick + advance, Esc to clear/step-back/
//! cancel. The keypress that *completes* the sentence never also runs it (it
//! advances to a Ready pane); a gated (`confirm`/`destructive`) verb needs an
//! extra armed beat, so a reflex double-Enter can't fire it.
//!
//! Built on demand (`make build-gui` / `cargo build -p goo-compose-gui`). Stay on
//! iced 0.14; the libcosmic swap is a separate cross-cutting arc. Deferred: the
//! adverb/slot panel (key-value widgets; verbs run on registry defaults until
//! then) and §6.6/§6.7 late-binding + error-recovery (#12).

use goo_engine::compose::{ComposeState, ComposeUi, Item, KeyInput, Stage, UiAction};
use goo_engine::{address, history, mime, registry, selection};
use iced::event::{self, Event, Status};
use iced::keyboard::{key::Named, Key};
use iced::widget::{column, container, mouse_area, row, scrollable, text, Space};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use serde_json::Value;

const MONO: iced::Font = iced::Font::MONOSPACE;
const RECENT_N: usize = 16;

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title(|_: &App| "cosmic-goo · compose".to_string())
        .theme(|_: &App| Theme::Dark)
        .subscription(App::subscription)
        .window_size((940.0, 560.0))
        .run()
}

struct App {
    reg: Value,
    ui: ComposeUi,
}

impl Default for App {
    fn default() -> Self {
        let reg = registry::load_all();
        let subjects = subject_candidates(&reg);
        App { reg, ui: ComposeUi::new(subjects) }
    }
}

#[derive(Debug, Clone)]
enum Message {
    /// A decoded keypress (from the global `event::listen_with` subscription).
    Key(KeyInput),
    /// A mouse click on row `usize` of the active pane (sets the selection, commits).
    Click(usize),
}

impl App {
    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(on_event)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let action = match message {
            Message::Key(k) => self.ui.apply(&k),
            Message::Click(i) => {
                self.ui.selected = i;
                self.ui.apply(&KeyInput::Enter)
            }
        };
        if let Some(action) = action {
            self.perform(action);
        }
        Task::none()
    }

    /// Perform the I/O a key produced — the ONLY side-effecting code; everything
    /// else is the pure reducer.
    fn perform(&mut self, action: UiAction) {
        match action {
            UiAction::ResolveSubject(addr) => match address::resolve(&addr, &self.reg, None) {
                Ok(subject) => {
                    let st = ComposeState::from_subject(&self.reg, &subject, addr);
                    let recent = history::recent_verbs_for_type(&st.subject_type, RECENT_N);
                    self.ui.on_subject_resolved(st, recent);
                }
                Err(e) => self.ui.set_error(format!("could not resolve {addr}: {e}")),
            },
            UiAction::LoadObjects(object_type) => {
                self.ui.set_objects(enumerate_objects(&self.reg, &object_type));
            }
            UiAction::Run => {
                if let Some(st) = self.ui.state.as_ref() {
                    std::process::exit(run_sentence(st));
                }
            }
            UiAction::Cancel => std::process::exit(130),
        }
    }

    // ========================================================================
    // View
    // ========================================================================

    fn view(&self) -> Element<'_, Message> {
        let ui = &self.ui;

        // The three panes, side by side. Each is active (query + filtered list),
        // committed (the chosen value), or pending (—).
        let subject_pane = self.pane("Subject", Stage::Subject, ui.state.as_ref().map(|s| s.subject_addr.clone()));
        let verb_pane = self.pane("Verb", Stage::Verb, ui.state.as_ref().and_then(|s| s.verb.clone()));

        let mut panes = row![subject_pane, verb_pane].spacing(10).height(Length::Fill);
        // The object pane appears only for a two-step verb.
        if ui.state.as_ref().map(|s| s.needs_object()).unwrap_or(false) {
            let obj = ui.state.as_ref().and_then(|s| s.object_addr.clone());
            panes = panes.push(self.pane("Object", Stage::Object, obj));
        }

        // Speak-it-back: the live CLI-equivalent, pinned at the bottom.
        let preview_text = ui.state.as_ref().map(|s| s.preview()).unwrap_or_else(|| "goo".into());
        let preview = container(text(preview_text).size(15).font(MONO).color(Color::from_rgb(0.55, 0.85, 0.6)))
            .padding(10)
            .width(Length::Fill)
            .style(panel_style);

        let status = self.status_line();
        let mut footer = column![preview, text(status).size(12).color(Color::from_rgb(0.6, 0.64, 0.83))].spacing(6);
        if let Some(e) = &ui.error {
            footer = footer.push(text(format!("⚠ {e}")).size(12).color(Color::from_rgb(0.95, 0.5, 0.4)));
        }

        column![
            text("goo-compose · noun-first").size(20),
            Space::new().height(Length::Fixed(8.0)),
            panes,
            Space::new().height(Length::Fixed(8.0)),
            footer,
        ]
        .padding(16)
        .into()
    }

    /// One pane: active (query + filtered list with the selection highlighted),
    /// committed (the chosen value), or pending.
    fn pane<'a>(&'a self, title: &str, stage: Stage, committed: Option<String>) -> Element<'a, Message> {
        let active = self.ui.stage == stage;
        let mut col = column![text(title.to_string()).size(12).color(Color::from_rgb(0.6, 0.64, 0.83))]
            .spacing(6)
            .width(Length::FillPortion(1));

        if active {
            col = col.push(text(format!("› {}", self.ui.query)).size(15).font(MONO).color(Color::from_rgb(0.85, 0.85, 0.9)));
            let mut list = column![].spacing(1);
            for (i, it) in self.ui.visible().into_iter().enumerate() {
                let selected = i == self.ui.selected;
                let cell = container(text(it.label).size(13).font(MONO)).padding([3, 8]).width(Length::Fill);
                let cell = if selected { cell.style(sel_style) } else { cell };
                // Click-to-commit (mouse_area doesn't take focus, so it can't steal keys).
                list = list.push(mouse_area(cell).on_press(Message::Click(i)));
            }
            col = col.push(scrollable(list).height(Length::Fill));
        } else if let Some(v) = committed {
            col = col.push(text(v).size(13).font(MONO).color(Color::from_rgb(0.55, 0.85, 0.6)));
        } else {
            col = col.push(text("—").size(13).color(Color::from_rgb(0.45, 0.48, 0.6)));
        }

        container(col).padding(8).height(Length::Fill).style(panel_style).into()
    }

    /// The footer hint line — context-sensitive, and the confirm prompt in Ready.
    fn status_line(&self) -> String {
        match self.ui.stage {
            Stage::Ready if self.ui.gated() && !self.ui.armed() => {
                let what = if self.ui.state.as_ref().map(|s| s.is_destructive()).unwrap_or(false) {
                    "destructive [!!]"
                } else {
                    "needs confirm [!]"
                };
                format!("⚠ {what} — Enter to confirm · Esc to step back")
            }
            Stage::Ready if self.ui.gated() => "⚠ Enter again to RUN · Esc to step back".to_string(),
            Stage::Ready => "Enter to run · Esc to step back".to_string(),
            _ => "type to filter · ↑/↓ move · Enter or Tab pick · Esc clear/back".to_string(),
        }
    }
}

/// Decode a global keyboard event into the reducer's iced-free [`KeyInput`].
/// Named (nav/control) keys are matched FIRST; only a genuine character key
/// edits the query, so Tab/Enter/Backspace never leak a control char into it.
fn on_event(event: Event, _status: Status, _id: iced::window::Id) -> Option<Message> {
    let Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event else {
        return None;
    };
    let ki = match key.as_ref() {
        Key::Named(Named::Enter) => KeyInput::Enter,
        Key::Named(Named::Tab) => KeyInput::Tab,
        Key::Named(Named::Escape) => KeyInput::Escape,
        Key::Named(Named::Backspace) => KeyInput::Backspace,
        Key::Named(Named::ArrowUp) => KeyInput::Up,
        Key::Named(Named::ArrowDown) => KeyInput::Down,
        Key::Named(Named::Space) => KeyInput::Char(" ".into()),
        Key::Character(c) => KeyInput::Char(c.to_string()),
        _ => return None,
    };
    Some(Message::Key(ki))
}

// ============================================================================
// Helpers (the I/O the pure reducer excludes)
// ============================================================================

/// Spawn `goo` with the sentence's argv and return its exit code. A
/// confirm/destructive verb gets `--confirm-dangerous=<verb>` appended (NOT part
/// of `argv()`/the preview — the GUI's confirm beat earns it; a spawned `goo`
/// has no stdin for the y/N gate).
fn run_sentence(st: &ComposeState) -> i32 {
    let mut argv = st.argv();
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

/// Subject candidates as canonical `goo://…` addresses + labels: implicit
/// selection / clipboard first, then items from enumerable prefixed sources.
fn subject_candidates(reg: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    let trunc = |s: &str| s.chars().take(60).collect::<String>();

    let sel = selection::primary();
    if !sel.is_empty() {
        out.push(Item::new("goo://sel/", format!("selection: {}", trunc(&sel))));
    }
    let clip = selection::clipboard();
    if !clip.is_empty() {
        out.push(Item::new("goo://clip/", format!("clipboard: {}", trunc(&clip))));
    }
    out.extend(source_items(reg, |_emits| true, true));
    out
}

/// Object candidates of `object_type`: items from every source whose `emits` is a
/// subtype of the wanted type (`mime::is_subtype`, NOT exact `==`, so polymorphic
/// object types still match).
fn enumerate_objects(reg: &Value, object_type: &str) -> Vec<Item> {
    source_items(reg, |emits| mime::is_subtype(emits, object_type, reg), false)
}

/// Shared source-item enumeration: run each qualifying source's `list_cmd` and
/// build `goo://<prefix>/<id>` items. `keep` filters by the source's `emits`;
/// `tag_source` appends the source name to the label (subjects show it).
fn source_items(reg: &Value, keep: impl Fn(&str) -> bool, tag_source: bool) -> Vec<Item> {
    let mut out = Vec::new();
    let Some(sources) = reg.get("sources").and_then(|s| s.as_array()) else { return out };
    for source in sources {
        if source.get("enumerate").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = source.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name == "selection" || name == "clipboard" {
            continue;
        }
        let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
        if !keep(emits) {
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
            let label = if tag_source { format!("{title} ({name})") } else { title.to_string() };
            out.push(Item::new(format!("goo://{prefix}/{id}"), label));
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

fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.weak.color.into()),
        border: iced::Border { radius: 8.0.into(), width: 1.0, color: palette.background.strong.color },
        ..Default::default()
    }
}

/// The highlighted-row background (the current selection).
fn sel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.primary.weak.color.into()),
        text_color: Some(palette.primary.weak.text),
        border: iced::Border { radius: 4.0.into(), ..Default::default() },
        ..Default::default()
    }
}
