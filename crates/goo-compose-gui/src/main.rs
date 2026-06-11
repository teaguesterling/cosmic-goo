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

use goo_engine::compose::{ComposeState, ComposeUi, Item, KeyInput, RunResult, Stage, UiAction};
use goo_engine::{address, history, mime, registry, selection};
use iced::event::{self, Event, Status};
use iced::keyboard::{key::Named, Key};
use iced::widget::{column, container, image, mouse_area, row, scrollable, svg, text, Space};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use serde_json::Value;
use std::collections::HashMap;

const ICON_PX: f32 = 20.0;

/// A loaded themed icon — PNG (`image`) or SVG (`svg`); both handles are cheap
/// (Arc-backed) to clone per frame.
#[derive(Clone)]
enum IconKind {
    Raster(image::Handle),
    Vector(svg::Handle),
}

const MONO: iced::Font = iced::Font::MONOSPACE;
const RECENT_N: usize = 16;

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title(|_: &App| "cosmic-goo · compose".to_string())
        .theme(|_: &App| Theme::Dark)
        .subscription(App::subscription)
        // A frameless, centered floating launcher (gnome-do idiom) — no titlebar
        // chrome; Esc is the close affordance.
        .decorations(false)
        .position(iced::window::Position::Centered)
        .window_size((940.0, 560.0))
        .run()
}

struct App {
    reg: Value,
    ui: ComposeUi,
    /// Resolved icon cache, keyed by icon name. `None` = looked up, not found
    /// (cached so a miss isn't re-resolved every render).
    icons: HashMap<String, Option<IconKind>>,
}

impl Default for App {
    fn default() -> Self {
        let reg = registry::load_all();
        let subjects = subject_candidates(&reg);
        let mut icons = HashMap::new();
        load_icons(&subjects, &mut icons);
        App { reg, ui: ComposeUi::new(subjects), icons }
    }
}

#[derive(Debug, Clone)]
enum Message {
    /// A decoded keypress (from the global `event::listen_with` subscription).
    Key(KeyInput),
    /// A mouse click on row `usize` of the active pane (sets the selection, commits).
    Click(usize),
    /// The off-thread verb run finished with this captured output.
    RunComplete(RunResult),
}

impl App {
    fn subscription(&self) -> Subscription<Message> {
        event::listen_with(on_event)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Key(k) => {
                let action = self.ui.apply(&k);
                self.perform(action)
            }
            Message::Click(i) => {
                self.ui.selected = i;
                let action = self.ui.apply(&KeyInput::Enter);
                self.perform(action)
            }
            // The off-thread run finished — feed its output back to the reducer,
            // which flips to the Result stage (success view or error recovery).
            Message::RunComplete(r) => {
                self.ui.on_run_result(r.stdout, r.stderr, r.code);
                Task::none()
            }
        }
    }

    /// Perform the I/O a key produced — the ONLY side-effecting code; everything
    /// else is the pure reducer. Returns a `Task` so a slow verb (an LLM call) can
    /// run off the UI thread without freezing the window.
    fn perform(&mut self, action: Option<UiAction>) -> Task<Message> {
        let Some(action) = action else { return Task::none() };
        match action {
            UiAction::ResolveSubject(addr) => {
                match address::resolve(&addr, &self.reg, None) {
                    Ok(subject) => {
                        let st = ComposeState::from_subject(&self.reg, &subject, addr);
                        let recent = history::recent_verbs_for_type(&st.subject_type, RECENT_N);
                        self.ui.on_subject_resolved(st, recent);
                    }
                    Err(e) => self.ui.set_error(format!("could not resolve {addr}: {e}")),
                }
                Task::none()
            }
            UiAction::LoadObjects(object_type) => {
                let objects = enumerate_objects(&self.reg, &object_type);
                load_icons(&objects, &mut self.icons);
                self.ui.set_objects(objects);
                Task::none()
            }
            UiAction::Run => {
                let Some(st) = self.ui.state.as_ref() else { return Task::none() };
                let argv = run_argv(st);
                self.ui.on_run_started(); // → Running stage; the view shows "running…"
                // Run the (blocking) child on a worker thread; await its result via
                // a oneshot so the iced executor — and the UI — stay responsive.
                Task::perform(
                    async move {
                        let (tx, rx) = iced::futures::channel::oneshot::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(run_capture(argv));
                        });
                        rx.await.unwrap_or_else(|_| RunResult {
                            stdout: String::new(),
                            stderr: "compose-gui: run thread died".into(),
                            code: 1,
                        })
                    },
                    Message::RunComplete,
                )
            }
            UiAction::CopyResult(s) => {
                copy_to_clipboard(&s);
                Task::none()
            }
            // Dismiss: propagate the run's exit code (130 if nothing ran, 0 on a
            // successful result, the verb's code on a failed one).
            UiAction::Cancel => {
                let code = self.ui.run_output().map(|r| r.code).unwrap_or(130);
                std::process::exit(code);
            }
        }
    }

    // ========================================================================
    // View
    // ========================================================================

    fn view(&self) -> Element<'_, Message> {
        let ui = &self.ui;

        // Post-run: a full-pane result/error view instead of the slot panes (#12).
        if matches!(ui.stage, Stage::Running | Stage::Result) {
            return self.result_view();
        }

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
        let mut footer = column![].spacing(6);
        // The adverb panel (Ready stage, selector verbs) sits above the preview so
        // a change is reflected immediately in the speak-it-back line below it.
        if let Some(panel) = self.adverb_panel() {
            footer = footer.push(panel);
        }
        footer = footer.push(preview).push(text(status).size(12).color(Color::from_rgb(0.6, 0.64, 0.83)));
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

    /// The post-run view (#12): the sentence that ran, then either "running…", the
    /// captured output (success), or stderr + retry/edit/cancel (failure). The
    /// sentence preview stays on screen throughout (§6.7 preserves the state).
    fn result_view(&self) -> Element<'_, Message> {
        let ui = &self.ui;
        let running = ui.stage == Stage::Running;
        let r = ui.run_output().cloned().unwrap_or_default();
        let failed = !running && r.code != 0;

        let preview = ui.state.as_ref().map(|s| s.preview()).unwrap_or_else(|| "goo".into());
        let header = container(text(preview).size(14).font(MONO).color(Color::from_rgb(0.55, 0.85, 0.6)))
            .padding(10)
            .width(Length::Fill)
            .style(panel_style);

        let body: Element<Message> = if running {
            text("running… ⟳").size(16).color(Color::from_rgb(0.85, 0.82, 0.5)).into()
        } else if failed {
            let msg = if r.stderr.trim().is_empty() {
                format!("verb failed (exit {})", r.code)
            } else {
                r.stderr.clone()
            };
            scrollable(text(msg).size(13).font(MONO).color(Color::from_rgb(0.95, 0.5, 0.4)))
                .height(Length::Fill)
                .into()
        } else {
            let out = if r.stdout.trim().is_empty() { "(no output)".to_string() } else { r.stdout.clone() };
            scrollable(text(out).size(14).font(MONO).color(Color::from_rgb(0.85, 0.88, 0.92)))
                .height(Length::Fill)
                .into()
        };
        let body_pane = container(body).padding(12).width(Length::Fill).height(Length::Fill).style(panel_style);

        let (status, status_color) = if running {
            ("running… (esc can't interrupt yet)".to_string(), Color::from_rgb(0.6, 0.64, 0.83))
        } else if failed {
            (format!("[r]etry · [e]dit · [Esc] cancel   (exit {})", r.code), Color::from_rgb(0.95, 0.5, 0.4))
        } else {
            ("Enter/Esc to close · c to copy".to_string(), Color::from_rgb(0.6, 0.84, 0.6))
        };

        column![
            text(if failed { "goo-compose · failed" } else { "goo-compose · result" }).size(20),
            Space::new().height(Length::Fixed(8.0)),
            header,
            Space::new().height(Length::Fixed(8.0)),
            body_pane,
            Space::new().height(Length::Fixed(8.0)),
            text(status).size(13).color(status_color),
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
            let visible = self.ui.visible();
            if visible.is_empty() {
                // Never a silent empty pane. On the verb stage with nothing applicable
                // the usual cause is an unloaded plugin set, so say so.
                let hint = match (stage, self.ui.query.is_empty()) {
                    (Stage::Verb, true) => "no verbs for this subject\n(plugins loaded? COSMIC_GOO_BUILTIN_PLUGINS_DIR)",
                    (_, true) => "no candidates",
                    (_, false) => "no matches",
                };
                col = col.push(text(hint.to_string()).size(12).color(Color::from_rgb(0.7, 0.55, 0.4)));
            }
            let mut list = column![].spacing(1);
            for (i, it) in visible.into_iter().enumerate() {
                let selected = i == self.ui.selected;
                let icon = self.icon_widget(it.icon.as_deref());
                let cell = container(
                    row![icon, text(it.label).size(13).font(MONO)].spacing(8).align_y(iced::Alignment::Center),
                )
                .padding([3, 8])
                .width(Length::Fill);
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

    /// The icon widget for a row: the cached themed image (PNG or SVG), or a
    /// fixed-size empty placeholder so every row's text stays left-aligned.
    fn icon_widget(&self, name: Option<&str>) -> Element<'_, Message> {
        let handle = name.and_then(|n| self.icons.get(n)).and_then(|o| o.as_ref());
        match handle {
            Some(IconKind::Raster(h)) => {
                image(h.clone()).width(Length::Fixed(ICON_PX)).height(Length::Fixed(ICON_PX)).into()
            }
            Some(IconKind::Vector(h)) => {
                svg(h.clone()).width(Length::Fixed(ICON_PX)).height(Length::Fixed(ICON_PX)).into()
            }
            None => Space::new().width(Length::Fixed(ICON_PX)).height(Length::Fixed(ICON_PX)).into(),
        }
    }

    /// The Ready-stage adverb panel (only for a selector verb with slots): one row
    /// per adverb — `name:` then its choices, the current value highlighted and the
    /// focused slot marked. `↑`/`↓` move slots, `←`/`→` cycle the focused one.
    fn adverb_panel(&self) -> Option<Element<'_, Message>> {
        if self.ui.stage != Stage::Ready {
            return None;
        }
        let slots = self.ui.adverb_slots();
        let st = self.ui.state.as_ref()?;
        if slots.is_empty() {
            return None;
        }
        let mut col = column![text("Options").size(12).color(Color::from_rgb(0.6, 0.64, 0.83))].spacing(3);
        for (i, slot) in slots.iter().enumerate() {
            let focused = i == self.ui.adverb_sel;
            let current = st.adverb_value(&slot.name, slot.default.as_deref());
            let marker = if focused { "› " } else { "  " };
            let name_color = if focused { Color::from_rgb(0.85, 0.85, 0.9) } else { Color::from_rgb(0.6, 0.64, 0.83) };
            let mut line = row![text(format!("{marker}{}:", slot.name))
                .size(13)
                .font(MONO)
                .width(Length::Fixed(96.0))
                .color(name_color)]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            for v in &slot.values {
                let is_cur = Some(v.as_str()) == current;
                let cell = container(text(v.clone()).size(12).font(MONO)).padding([2, 8]);
                let cell = if is_cur { cell.style(sel_style) } else { cell };
                line = line.push(cell);
            }
            col = col.push(line);
        }
        Some(container(col).padding(8).width(Length::Fill).style(panel_style).into())
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
            Stage::Ready => {
                let opts = if self.ui.adverb_slots().is_empty() { "" } else { " · ↑/↓ option · ←/→ change" };
                format!("Enter to run{opts} · Esc to step back")
            }
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
        Key::Named(Named::ArrowLeft) => KeyInput::Left,
        Key::Named(Named::ArrowRight) => KeyInput::Right,
        Key::Named(Named::Space) => KeyInput::Char(" ".into()),
        Key::Character(c) => KeyInput::Char(c.to_string()),
        _ => return None,
    };
    Some(Message::Key(ki))
}

// ============================================================================
// Helpers (the I/O the pure reducer excludes)
// ============================================================================

/// The sentence's argv for `goo`. A confirm/destructive verb gets
/// `--confirm-dangerous=<verb>` appended (NOT part of `argv()`/the preview — the
/// GUI's confirm beat earns it; a spawned `goo` has no stdin for the y/N gate).
fn run_argv(st: &ComposeState) -> Vec<String> {
    let mut argv = st.argv();
    if st.needs_confirm() || st.is_destructive() {
        if let Some(v) = st.verb.as_deref() {
            argv.push(format!("--confirm-dangerous={v}"));
        }
    }
    argv
}

/// Run `goo <argv>` and **capture** stdout/stderr + exit code (`.output()`, not
/// `.status()`), so the result can be shown in the GUI instead of vanishing to an
/// inherited fd. Runs on a worker thread (it can block for a slow LLM call).
fn run_capture(argv: Vec<String>) -> RunResult {
    let goo = std::env::var("GOO_BIN").unwrap_or_else(|_| "goo".to_string());
    match std::process::Command::new(&goo).args(&argv).output() {
        Ok(o) => RunResult {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            code: o.status.code().unwrap_or(1),
        },
        Err(e) => RunResult {
            stdout: String::new(),
            stderr: format!("compose-gui: failed to exec {goo}: {e}"),
            code: 1,
        },
    }
}

/// Put a successful run's reply on the Wayland clipboard (`wl-copy`).
fn copy_to_clipboard(s: &str) {
    use std::io::Write;
    if let Ok(mut child) = std::process::Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(s.as_bytes());
        }
        let _ = child.wait();
    }
}

/// Subject candidates as canonical `goo://…` addresses + labels: implicit
/// selection / clipboard first, then items from enumerable prefixed sources.
fn subject_candidates(reg: &Value) -> Vec<Item> {
    let mut out = Vec::new();
    let sel = selection::primary();
    if !sel.is_empty() {
        out.push(Item::new("goo://sel/", format!("selection: {}", clean_label(&sel))));
    }
    let clip = selection::clipboard();
    if !clip.is_empty() {
        out.push(Item::new("goo://clip/", format!("clipboard: {}", clean_label(&clip))));
    }
    out.extend(source_items(reg, |_emits| true, true));
    out
}

/// A one-line candidate label: collapse all whitespace (newlines, tabs, runs) to
/// single spaces and cap the length, so a long/multi-line selection or title
/// can't wrap across the pane.
fn clean_label(s: &str) -> String {
    const MAX: usize = 52;
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX {
        let kept: String = collapsed.chars().take(MAX - 1).collect();
        format!("{kept}…")
    } else {
        collapsed
    }
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
            let label = if tag_source { format!("{} ({name})", clean_label(title)) } else { clean_label(title) };
            // Per-item icon NAME (a freedesktop name, e.g. app_id); the shell
            // resolves it to a themed image. Falls back to the source-level icon.
            let icon = it
                .get("icon")
                .and_then(|v| v.as_str())
                .or_else(|| source.get("icon").and_then(|v| v.as_str()))
                .filter(|s| !s.is_empty())
                .map(String::from);
            out.push(Item::new(format!("goo://{prefix}/{id}"), label).with_icon(icon));
        }
    }
    out
}

/// Resolve & load every not-yet-cached icon name in `items` into `cache`. A miss
/// is cached as `None` so it isn't re-resolved on every render.
fn load_icons(items: &[Item], cache: &mut HashMap<String, Option<IconKind>>) {
    for it in items {
        let Some(name) = it.icon.as_deref() else { continue };
        if cache.contains_key(name) {
            continue;
        }
        cache.insert(name.to_string(), resolve_icon(name));
    }
}

/// Look up a freedesktop icon name in the current theme and load it — SVG via the
/// `svg` handle, anything else (PNG/XPM) via the `image` handle. `None` if the
/// theme has no such icon (the row then shows a blank placeholder).
fn resolve_icon(name: &str) -> Option<IconKind> {
    let path = freedesktop_icons::lookup(name).with_size(24).with_scale(1).find()?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => Some(IconKind::Vector(svg::Handle::from_path(path))),
        _ => Some(IconKind::Raster(image::Handle::from_path(path))),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the freedesktop-icons CRATE actually resolves a common name to a
    // loadable handle end-to-end (lookup + extension branch + handle build) — not
    // merely that a file exists on disk. If this is None, themed lookup needs
    // `.with_theme(<current theme>)` before the feature is anything but blank
    // gutters. Skips gracefully on a headless box with no icon themes installed.
    #[test]
    fn resolves_a_common_app_icon() {
        let has_themes = std::path::Path::new("/usr/share/icons/hicolor").is_dir();
        if !has_themes {
            eprintln!("no icon themes installed — skipping icon-resolution check");
            return;
        }
        assert!(
            resolve_icon("firefox").is_some() || resolve_icon("folder").is_some(),
            "freedesktop_icons::lookup returned None for both 'firefox' and 'folder' \
             despite hicolor being present — themed lookup likely needs .with_theme()"
        );
    }
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
