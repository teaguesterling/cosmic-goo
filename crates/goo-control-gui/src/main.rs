//! goo-control-gui (v1) — Control Center for cosmic-goo (iced).
//!
//! Registry browser + entity surface + plugin list, read-only, in-process —
//! the implementation of the design in `doc/control-center.html`. Reads the
//! engine via `goo_engine::registry::load_all()`; entity-peek runs source
//! `list_cmd`s via `goo_engine::shell::bash_capture`. No mutation in v1.
//!
//! The headline UI pattern is the multi-contributor view: a polymorphic verb
//! (`connect`/`stop`/`info`/`logs`/`show`/`status`/`disconnect`) shows all its
//! impls *stacked*, specificity-sorted, with the per-contributor `accepts`,
//! `default_for`, and `cmd` preview visible. The user can SEE the layered
//! registry instead of one collapsed row.
//!
//! Three panes implemented now (per the user's v1 pick):
//!   📚 Vocabulary  — verbs, types, domains
//!   🌐 Entities    — live per-source peek
//!   🔌 Plugins     — package list + per-plugin contribution summary
//!
//! Stubs for the remaining tabs (Channels / Modifiers / Tools / Diagnostics)
//! point to the design doc. MVU bones port mechanically to libcosmic later.
//!
//! Built on demand: `cargo build -p goo-control-gui`. Not a default-member.

use goo_engine::{registry, shell::bash_capture};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Color, Element, Length, Task, Theme};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn main() -> iced::Result {
    iced::application(App::default, App::update, App::view)
        .title(|_: &App| "cosmic-goo · Control Center".to_string())
        .theme(|_: &App| Theme::Dark)
        .window_size((1280.0, 820.0))
        .run()
}

// ============================================================================
// State
// ============================================================================

struct App {
    reg: Value,                   // the merged registry (load_all)
    tab: Tab,
    vsub: VocabSubtab,
    sel_verb: Option<String>,
    sel_type: Option<String>,
    sel_source: Option<String>,   // domain by source name
    sel_plugin: Option<String>,
    // Per-source live peek cache (filled on RefreshEntities). Empty Vec = "tried, no items".
    entity_cache: HashMap<String, Vec<Value>>,
}

impl Default for App {
    fn default() -> Self {
        App {
            reg: registry::load_all(),
            tab: Tab::default(),
            vsub: VocabSubtab::default(),
            sel_verb: None,
            sel_type: None,
            sel_source: None,
            sel_plugin: None,
            entity_cache: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Vocabulary,
    Entities,
    Channels,
    Modifiers,
    Plugins,
    Tools,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum VocabSubtab {
    #[default]
    Verbs,
    Types,
    Domains,
}

#[derive(Debug, Clone)]
enum Message {
    TabChanged(Tab),
    VocabSubChanged(VocabSubtab),
    SelectVerb(String),
    SelectType(String),
    SelectSource(String),
    SelectPlugin(String),
    PeekSource(String),
    Reload,
}

// ============================================================================
// Update
// ============================================================================

impl App {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TabChanged(t) => self.tab = t,
            Message::VocabSubChanged(s) => self.vsub = s,
            Message::SelectVerb(n) => self.sel_verb = Some(n),
            Message::SelectType(n) => self.sel_type = Some(n),
            Message::SelectSource(n) => self.sel_source = Some(n),
            Message::SelectPlugin(n) => self.sel_plugin = Some(n),
            Message::PeekSource(n) => {
                // Run list_cmd; cache result (empty Vec if it errors or returns nothing).
                let items = peek_source(&self.reg, &n);
                self.entity_cache.insert(n, items);
            }
            Message::Reload => self.reg = registry::load_all(),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let body: Element<_> = match self.tab {
            Tab::Vocabulary => self.view_vocabulary(),
            Tab::Entities => self.view_entities(),
            Tab::Plugins => self.view_plugins(),
            Tab::Channels => stub("Channels", "Coercion-graph inspector (live `--paths --format mermaid`) — design draft: doc/control-center.html"),
            Tab::Modifiers => stub("Modifiers", "Adverbs · sigils · aliases — the composition layer; v2 will edit defaults inline."),
            Tab::Tools => stub("Tools", "Shell-dep dashboard (which `tool` deps are on PATH, install hints) — v2/v3."),
            Tab::Diagnostics => stub("Diagnostics", "`goo validate` status, env, dispatch trace probe — v3."),
        };
        column![self.view_chrome(), container(body).padding(14)]
            .into()
    }
}

// ============================================================================
// Chrome (title + tabs)
// ============================================================================

impl App {
    fn view_chrome(&self) -> Element<'_, Message> {
        let title = text("cosmic-goo · Control Center")
            .size(20)
            .color(Color::from_rgb(0.91, 0.92, 1.0));
        let subtitle = text("registry browser · entity surface · plugins — v1 read-only")
            .size(12)
            .color(Color::from_rgb(0.6, 0.64, 0.83));
        let titlebar = column![
            row![title, Space::new().width(Length::Fill), subtitle].align_y(iced::Alignment::End),
        ]
        .padding([8u16, 18])
        .spacing(2);

        let mk = |label: &'static str, t: Tab, count: Option<String>| -> Element<'_, Message> {
            let on = self.tab == t;
            let label_txt = match count {
                Some(c) => format!("{label}  {c}"),
                None => label.to_string(),
            };
            let mut b = button(text(label_txt).size(13)).on_press(Message::TabChanged(t)).padding([6u16, 12]);
            if on {
                b = b.style(button::primary);
            } else {
                b = b.style(button::text);
            }
            b.into()
        };
        let n_verbs = unique_verb_names(&self.reg).len();
        let n_plugins = self.reg.get("plugins").and_then(Value::as_array).map_or(0, |a| a.len());
        let n_channels = self.reg.get("channels").and_then(Value::as_array).map_or(0, |a| a.len());
        let tabs = row![
            mk("📚 Vocabulary", Tab::Vocabulary, Some(format!("{n_verbs}"))),
            mk("🌐 Entities", Tab::Entities, None),
            mk("🔄 Channels", Tab::Channels, Some(format!("{n_channels}"))),
            mk("⚙️ Modifiers", Tab::Modifiers, None),
            mk("🔌 Plugins", Tab::Plugins, Some(format!("{n_plugins}"))),
            mk("🛠️ Tools", Tab::Tools, None),
            mk("🔍 Diagnostics", Tab::Diagnostics, None),
            Space::new().width(Length::Fill),
            button(text("⟳ reload").size(12)).on_press(Message::Reload).padding([6u16, 12]).style(button::text),
        ]
        .spacing(4)
        .padding(iced::Padding::default().top(4.0).bottom(8.0).horizontal(14.0));

        column![titlebar, tabs].into()
    }
}

// ============================================================================
// Vocabulary
// ============================================================================

impl App {
    fn view_vocabulary(&self) -> Element<'_, Message> {
        let mk_sub = |label: &'static str, s: VocabSubtab, count: usize| -> Element<'_, Message> {
            let on = self.vsub == s;
            let mut b = button(text(format!("{label} ({count})")).size(12))
                .on_press(Message::VocabSubChanged(s))
                .padding([4u16, 10]);
            if on {
                b = b.style(button::primary);
            } else {
                b = b.style(button::text);
            }
            b.into()
        };
        let n_verbs = unique_verb_names(&self.reg).len();
        let n_types = self.reg.get("types").and_then(Value::as_array).map_or(0, |a| a.len());
        let n_sources = self.reg.get("sources").and_then(Value::as_array).map_or(0, |a| a.len());

        let subtabs = row![
            mk_sub("verbs", VocabSubtab::Verbs, n_verbs),
            mk_sub("types", VocabSubtab::Types, n_types),
            mk_sub("domains", VocabSubtab::Domains, n_sources),
        ]
        .spacing(4)
        .padding(iced::Padding::default().bottom(10.0));

        let body = match self.vsub {
            VocabSubtab::Verbs => self.view_verbs(),
            VocabSubtab::Types => self.view_types(),
            VocabSubtab::Domains => self.view_domains(),
        };

        column![subtabs, body].spacing(6).into()
    }

    // ---- Verbs sub-tab ----
    fn view_verbs(&self) -> Element<'_, Message> {
        // List pane: every unique verb name with × count badge for polymorphic ones.
        let names = unique_verb_names(&self.reg);
        let contributors_by_name = verb_contributors(&self.reg);
        let mut list_col = column![].spacing(2);
        for name in &names {
            let contributors = contributors_by_name.get(name).map_or(0, Vec::len);
            let on = self.sel_verb.as_deref() == Some(name.as_str());
            let badge = if contributors > 1 {
                format!("  ×{contributors}")
            } else {
                String::new()
            };
            let label = format!("{name}{badge}");
            let mut b = button(text(label).size(13).font(iced::Font::MONOSPACE))
                .on_press(Message::SelectVerb(name.clone()))
                .padding([4u16, 10])
                .width(Length::Fill);
            b = if on { b.style(button::primary) } else { b.style(button::text) };
            list_col = list_col.push(b);
        }
        let list = container(scrollable(list_col).height(Length::Fill))
            .width(Length::Fixed(280.0))
            .padding(6)
            .style(panel_style);

        // Detail pane.
        let detail: Element<_> = if let Some(name) = &self.sel_verb {
            let contributors = contributors_by_name.get(name).cloned().unwrap_or_default();
            self.view_verb_detail(name.clone(), contributors)
        } else {
            placeholder("Pick a verb on the left.")
        };

        row![list, container(detail).padding(8).style(panel_style).width(Length::Fill).height(Length::Fill)]
            .spacing(10)
            .height(Length::Fill)
            .into()
    }

    fn view_verb_detail(&self, name: String, contributors: Vec<Value>) -> Element<'static, Message> {
        let header = text(format!("{name}"))
            .size(20)
            .font(iced::Font::MONOSPACE);
        let n = contributors.len();
        let summary = if n > 1 {
            format!("polymorphic — {n} contributors, dispatched by subject type via specificity")
        } else {
            "1 contributor".to_string()
        };
        let mut col = column![
            header,
            text(summary).size(12).color(Color::from_rgb(0.6, 0.64, 0.83)),
            Space::new().height(Length::Fixed(10.0)),
        ]
        .spacing(4);

        for v in &contributors {
            col = col.push(view_contributor_card(v));
        }
        scrollable(col).into()
    }

    // ---- Types sub-tab ----
    fn view_types(&self) -> Element<'_, Message> {
        let mut names: Vec<String> = self
            .reg
            .get("types")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str).map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();

        let mut list_col = column![].spacing(2);
        for n in &names {
            let on = self.sel_type.as_deref() == Some(n.as_str());
            let mut b = button(text(n.clone()).size(12).font(iced::Font::MONOSPACE))
                .on_press(Message::SelectType(n.clone()))
                .padding([4u16, 10])
                .width(Length::Fill);
            b = if on { b.style(button::primary) } else { b.style(button::text) };
            list_col = list_col.push(b);
        }
        let list = container(scrollable(list_col).height(Length::Fill))
            .width(Length::Fixed(320.0))
            .padding(6)
            .style(panel_style);

        let detail: Element<_> = if let Some(t) = &self.sel_type {
            self.view_type_detail(t)
        } else {
            placeholder("Pick a type on the left.")
        };

        row![list, container(detail).padding(8).style(panel_style).width(Length::Fill).height(Length::Fill)]
            .spacing(10)
            .height(Length::Fill)
            .into()
    }

    fn view_type_detail(&self, t: &str) -> Element<'_, Message> {
        let type_decls = find_type_decls(&self.reg, t);
        let is_a_chain = walk_is_a(&self.reg, t);
        let accepted_by = verbs_accepting(&self.reg, t);
        let emitted_by = sources_emitting(&self.reg, t);

        let mut col = column![
            text(t.to_string()).size(18).font(iced::Font::MONOSPACE),
            text(format!(
                "declared by {} plugin(s) · is_a chain: {}",
                type_decls.len(),
                if is_a_chain.len() > 1 {
                    is_a_chain.join(" ⊑ ")
                } else {
                    "(none)".into()
                }
            ))
            .size(12)
            .color(Color::from_rgb(0.6, 0.64, 0.83)),
            Space::new().height(Length::Fixed(10.0)),
        ]
        .spacing(4);

        col = col.push(section_header("Emitted by"));
        if emitted_by.is_empty() {
            col = col.push(text("— no sources emit this type").size(12).color(Color::from_rgb(0.4, 0.43, 0.69)));
        } else {
            for s in &emitted_by {
                col = col.push(text(format!("  • :{s}")).size(13).font(iced::Font::MONOSPACE));
            }
        }

        col = col.push(Space::new().height(Length::Fixed(8.0)));
        col = col.push(section_header(&format!("Accepted by ({} verb(s))", accepted_by.len())));
        let mut verb_line = String::new();
        for (i, v) in accepted_by.iter().enumerate() {
            if i > 0 {
                verb_line.push_str(" · ");
            }
            verb_line.push_str(v);
        }
        col = col.push(text(verb_line).size(13).font(iced::Font::MONOSPACE));

        col = col.push(Space::new().height(Length::Fixed(8.0)));
        col = col.push(section_header(&format!("Declared in ({})", type_decls.len())));
        for d in &type_decls {
            let kv = format!(
                "  · {plugin} — kind={kind}{is_a}",
                plugin = d.plugin,
                kind = d.kind.as_deref().unwrap_or("?"),
                is_a = if d.is_a.is_empty() {
                    String::new()
                } else {
                    format!(", is_a=[{}]", d.is_a.join(", "))
                }
            );
            col = col.push(text(kv).size(12).font(iced::Font::MONOSPACE));
        }

        scrollable(col).into()
    }

    // ---- Domains sub-tab ----
    fn view_domains(&self) -> Element<'_, Message> {
        let sources = self.reg.get("sources").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut by_name: BTreeMap<String, Value> = BTreeMap::new();
        for s in sources {
            if let Some(n) = s.get("name").and_then(Value::as_str) {
                by_name.insert(n.to_string(), s);
            }
        }

        let mut list_col = column![].spacing(2);
        for (name, s) in &by_name {
            let on = self.sel_source.as_deref() == Some(name.as_str());
            let prefix = s
                .get("prefix")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(|p| format!(":{p}"))
                .unwrap_or_else(|| "—".to_string());
            let label = format!("{prefix:>9}  {name}");
            let mut b = button(text(label).size(12).font(iced::Font::MONOSPACE))
                .on_press(Message::SelectSource(name.clone()))
                .padding([4u16, 10])
                .width(Length::Fill);
            b = if on { b.style(button::primary) } else { b.style(button::text) };
            list_col = list_col.push(b);
        }
        let list = container(scrollable(list_col).height(Length::Fill))
            .width(Length::Fixed(280.0))
            .padding(6)
            .style(panel_style);

        let detail: Element<_> = if let Some(n) = &self.sel_source {
            let s = by_name.get(n).cloned().unwrap_or(Value::Null);
            self.view_domain_detail(n.clone(), s)
        } else {
            placeholder("Pick a domain on the left.")
        };

        row![list, container(detail).padding(8).style(panel_style).width(Length::Fill).height(Length::Fill)]
            .spacing(10)
            .height(Length::Fill)
            .into()
    }

    fn view_domain_detail(&self, name: String, s: Value) -> Element<'static, Message> {
        let prefix = s.get("prefix").and_then(Value::as_str).unwrap_or("—");
        let emits = s.get("emits").and_then(Value::as_str).unwrap_or("(none)");
        let enumerate = s.get("enumerate").and_then(Value::as_bool).unwrap_or(true);
        let implicit = s.get("implicit").and_then(Value::as_bool).unwrap_or(false);
        let plugin = s.get("plugin").and_then(Value::as_str).unwrap_or("?");
        let lc = s.get("list_cmd").and_then(Value::as_str).unwrap_or("(none)");

        let mut col = column![
            text(format!(":{prefix}  ({name})")).size(18).font(iced::Font::MONOSPACE),
            text(format!(
                "from plugins/{plugin}.toml · emits {emits} · enumerate={enumerate} · implicit={implicit}"
            ))
            .size(12)
            .color(Color::from_rgb(0.6, 0.64, 0.83)),
            Space::new().height(Length::Fixed(10.0)),
            section_header("list_cmd"),
            text(lc.to_string()).size(11).font(iced::Font::MONOSPACE).color(Color::from_rgb(0.84, 0.86, 1.0)),
            Space::new().height(Length::Fixed(10.0)),
        ]
        .spacing(4);

        // Peek button
        col = col.push(
            button(text("Peek items (run list_cmd)").size(12))
                .on_press(Message::PeekSource(name.to_string()))
                .padding([4u16, 12])
                .style(button::primary),
        );

        if let Some(items) = self.entity_cache.get(&name) {
            col = col.push(Space::new().height(Length::Fixed(10.0)));
            col = col.push(section_header(&format!("Items ({})", items.len())));
            if items.is_empty() {
                col = col
                    .push(text("(empty — list_cmd returned no items or errored)").size(12).color(Color::from_rgb(0.4, 0.43, 0.69)));
            } else {
                for it in items.iter().take(50) {
                    let title = it.get("title").and_then(Value::as_str).unwrap_or("");
                    let id = it.get("id").and_then(Value::as_str).unwrap_or("");
                    let sub = it.get("subtitle").and_then(Value::as_str).unwrap_or("");
                    col = col.push(text(format!("  • {title}  ({id})  {sub}")).size(12).font(iced::Font::MONOSPACE));
                }
                if items.len() > 50 {
                    col = col.push(text(format!("  …and {} more", items.len() - 50)).size(11).color(Color::from_rgb(0.4, 0.43, 0.69)));
                }
            }
        }

        scrollable(col).into()
    }
}

// ============================================================================
// Entities (per-source live peek, grouped)
// ============================================================================

impl App {
    fn view_entities(&self) -> Element<'_, Message> {
        let mut col = column![
            text("Entities").size(20),
            text("Live per-source peek — the canonical singular pane. Click any source to fetch its items.")
                .size(12)
                .color(Color::from_rgb(0.6, 0.64, 0.83)),
            Space::new().height(Length::Fixed(10.0)),
        ]
        .spacing(4);

        let sources = self.reg.get("sources").and_then(Value::as_array).cloned().unwrap_or_default();
        for s in sources {
            let name = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let prefix = s.get("prefix").and_then(Value::as_str).unwrap_or("—");
            let emits = s.get("emits").and_then(Value::as_str).unwrap_or("?");
            let cache = self.entity_cache.get(&name);
            let count_str = match cache {
                Some(items) => format!("{} item(s)", items.len()),
                None => "click peek".to_string(),
            };

            let head = row![
                text(format!(":{prefix}  ")).size(14).font(iced::Font::MONOSPACE),
                text(name.clone()).size(13).font(iced::Font::MONOSPACE).color(Color::from_rgb(0.6, 0.64, 0.83)),
                Space::new().width(Length::Fill),
                text(format!("emits {emits}  ·  {count_str}")).size(11).color(Color::from_rgb(0.4, 0.43, 0.69)),
                button(text(if cache.is_some() { "refresh" } else { "peek" }).size(11))
                    .on_press(Message::PeekSource(name.clone()))
                    .padding([3u16, 9])
                    .style(button::text),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);

            let mut group = column![head].spacing(4).padding(10);
            if let Some(items) = cache {
                for it in items.iter().take(8) {
                    let title = it.get("title").and_then(Value::as_str).unwrap_or("");
                    let id = it.get("id").and_then(Value::as_str).unwrap_or("");
                    group = group.push(
                        text(format!("    • {title}  ({id})")).size(12).font(iced::Font::MONOSPACE),
                    );
                }
                if items.len() > 8 {
                    group = group.push(text(format!("    …and {} more", items.len() - 8)).size(11).color(Color::from_rgb(0.4, 0.43, 0.69)));
                }
            }

            col = col.push(container(group).padding(2).style(panel_style));
        }

        scrollable(col).into()
    }
}

// ============================================================================
// Plugins
// ============================================================================

impl App {
    fn view_plugins(&self) -> Element<'_, Message> {
        let plugins = self.reg.get("plugins").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut by_name: BTreeMap<String, Value> = BTreeMap::new();
        for p in plugins {
            if let Some(n) = p.get("name").and_then(Value::as_str) {
                by_name.insert(n.to_string(), p);
            }
        }

        let mut list_col = column![].spacing(2);
        for (name, p) in &by_name {
            let on = self.sel_plugin.as_deref() == Some(name.as_str());
            let tier = p.get("tier").and_then(Value::as_str).unwrap_or("desktop");
            let label = format!("{name}  ({tier})");
            let mut b = button(text(label).size(12).font(iced::Font::MONOSPACE))
                .on_press(Message::SelectPlugin(name.clone()))
                .padding([4u16, 10])
                .width(Length::Fill);
            b = if on { b.style(button::primary) } else { b.style(button::text) };
            list_col = list_col.push(b);
        }
        let list = container(scrollable(list_col).height(Length::Fill))
            .width(Length::Fixed(300.0))
            .padding(6)
            .style(panel_style);

        let detail: Element<_> = if let Some(n) = &self.sel_plugin {
            let p = by_name.get(n).cloned().unwrap_or(Value::Null);
            self.view_plugin_detail(n, &p)
        } else {
            placeholder("Pick a plugin on the left.")
        };

        row![list, container(detail).padding(8).style(panel_style).width(Length::Fill).height(Length::Fill)]
            .spacing(10)
            .height(Length::Fill)
            .into()
    }

    fn view_plugin_detail(&self, name: &str, p: &Value) -> Element<'_, Message> {
        let tier = p.get("tier").and_then(Value::as_str).unwrap_or("desktop");
        let desc = p.get("description").and_then(Value::as_str).unwrap_or("");

        // Count contributions in the merged registry (filtered by plugin field, which
        // `registry::merge` sets via `with_provenance`).
        let count = |k: &str| -> usize {
            self.reg
                .get(k)
                .and_then(Value::as_array)
                .map_or(0, |a| a.iter().filter(|e| e.get("plugin").and_then(Value::as_str) == Some(name)).count())
        };

        let mut col = column![
            text(name.to_string()).size(18).font(iced::Font::MONOSPACE),
            text(format!("tier: {tier}")).size(12).color(Color::from_rgb(0.6, 0.64, 0.83)),
            text(desc.to_string()).size(13),
            Space::new().height(Length::Fixed(10.0)),
            section_header("Contributions"),
        ]
        .spacing(4);

        for (label, k) in [
            ("verbs", "verbs"),
            ("sources", "sources"),
            ("types", "types"),
            ("channels", "channels"),
            ("adverbs", "adverbs"),
            ("sigils", "sigils"),
            ("aliases", "aliases"),
            ("checkers", "checkers"),
            ("detectors", "detectors"),
        ] {
            let n = count(k);
            if n > 0 {
                col = col.push(text(format!("  · {label}: {n}")).size(13).font(iced::Font::MONOSPACE));
            }
        }

        scrollable(col).into()
    }
}

// ============================================================================
// Helpers — projections over the registry
// ============================================================================

/// Sorted, deduped verb names across all (polymorphic) entries.
fn unique_verb_names(reg: &Value) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    if let Some(arr) = reg.get("verbs").and_then(Value::as_array) {
        for v in arr {
            if let Some(n) = v.get("name").and_then(Value::as_str) {
                set.insert(n.to_string());
            }
        }
    }
    set.into_iter().collect()
}

/// Map verb-name → all contributing entries (in registry order, which is also
/// dispatch tie-break order — later registered wins ties).
fn verb_contributors(reg: &Value) -> HashMap<String, Vec<Value>> {
    let mut by_name: HashMap<String, Vec<Value>> = HashMap::new();
    if let Some(arr) = reg.get("verbs").and_then(Value::as_array) {
        for v in arr {
            if let Some(n) = v.get("name").and_then(Value::as_str) {
                by_name.entry(n.to_string()).or_default().push(v.clone());
            }
        }
    }
    by_name
}

struct TypeDecl {
    plugin: String,
    kind: Option<String>,
    is_a: Vec<String>,
}

fn find_type_decls(reg: &Value, type_name: &str) -> Vec<TypeDecl> {
    let mut out = Vec::new();
    if let Some(arr) = reg.get("types").and_then(Value::as_array) {
        for t in arr {
            if t.get("name").and_then(Value::as_str) == Some(type_name) {
                let is_a: Vec<String> = t
                    .get("is_a")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                out.push(TypeDecl {
                    plugin: t.get("plugin").and_then(Value::as_str).unwrap_or("?").to_string(),
                    kind: t.get("kind").and_then(Value::as_str).map(String::from),
                    is_a,
                });
            }
        }
    }
    out
}

/// Walk `is_a` to surface the supertype chain (first match per step; for cycles,
/// stop after a few hops). Used for the type-detail "lattice" line.
fn walk_is_a(reg: &Value, start: &str) -> Vec<String> {
    let mut chain = vec![start.to_string()];
    let mut current = start.to_string();
    for _ in 0..6 {
        let decls = find_type_decls(reg, &current);
        let Some(next) = decls.iter().find_map(|d| d.is_a.first().cloned()) else { break };
        if chain.contains(&next) {
            break;
        }
        chain.push(next.clone());
        current = next;
    }
    chain
}

/// All verb names whose `accepts` patterns match `type_name` exactly OR
/// structurally — used for the "Accepted by" list. (Structural matching defers
/// to `mime::is_subtype` for accuracy, but for the v1 display we use a cheaper
/// substring/glob check; the OPTIONS surface is the authoritative dispatcher.)
fn verbs_accepting(reg: &Value, type_name: &str) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    if let Some(arr) = reg.get("verbs").and_then(Value::as_array) {
        for v in arr {
            let Some(accepts) = v.get("accepts").and_then(Value::as_array) else { continue };
            let matches = accepts.iter().filter_map(|p| p.as_str()).any(|p| {
                p == type_name
                    || p == "*/*"
                    || (p.ends_with("/*") && type_name.starts_with(&p[..p.len() - 1]))
            });
            if matches {
                if let Some(n) = v.get("name").and_then(Value::as_str) {
                    set.insert(n.to_string());
                }
            }
        }
    }
    set.into_iter().collect()
}

fn sources_emitting(reg: &Value, type_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = reg.get("sources").and_then(Value::as_array) {
        for s in arr {
            if s.get("emits").and_then(Value::as_str) == Some(type_name) {
                if let Some(n) = s.get("name").and_then(Value::as_str) {
                    out.push(n.to_string());
                }
            }
        }
    }
    out
}

/// Run the source's `list_cmd` (via bash_capture) and parse the JSON. Empty
/// vec on any failure — the entity pane shows "no items" without crashing.
fn peek_source(reg: &Value, name: &str) -> Vec<Value> {
    let Some(sources) = reg.get("sources").and_then(Value::as_array) else { return vec![] };
    let Some(src) = sources.iter().find(|s| s.get("name").and_then(Value::as_str) == Some(name)) else {
        return vec![];
    };
    let Some(lc) = src.get("list_cmd").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return vec![];
    };
    let out = bash_capture(lc);
    serde_json::from_str::<Vec<Value>>(out.trim()).unwrap_or_default()
}

// ============================================================================
// View helpers (small components)
// ============================================================================

fn view_contributor_card(v: &Value) -> Element<'static, Message> {
    let plugin = v.get("plugin").and_then(Value::as_str).unwrap_or("?");
    let accepts: Vec<String> = v
        .get("accepts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let default_for = v
        .get("default_for")
        .and_then(|d| match d {
            Value::String(s) => Some(s.clone()),
            Value::Array(arr) => Some(arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
            _ => None,
        })
        .unwrap_or_else(|| "—".into());
    let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("(no cmd)");
    let confirm = v.get("confirm").and_then(Value::as_bool).unwrap_or(false);
    let object_type = v.get("object_type").and_then(Value::as_str).filter(|s| !s.is_empty());

    let head = row![
        text("from ").size(11).color(Color::from_rgb(0.4, 0.43, 0.69)),
        text(format!("plugins/{plugin}.toml"))
            .size(12)
            .font(iced::Font::MONOSPACE)
            .color(Color::from_rgb(0.21, 0.84, 0.76)),
        Space::new().width(Length::Fill),
        if confirm {
            text("⚠ confirm").size(11).color(Color::from_rgb(1.0, 0.71, 0.33))
        } else {
            text("").size(11)
        },
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let mut body = column![
        head,
        Space::new().height(Length::Fixed(4.0)),
        text(format!("accepts: {}", accepts.join(", "))).size(12).font(iced::Font::MONOSPACE),
        text(format!("default_for: {default_for}")).size(12).font(iced::Font::MONOSPACE),
    ]
    .spacing(3)
    .padding(10);

    if let Some(ot) = object_type {
        body = body.push(text(format!("object_type: {ot}")).size(12).font(iced::Font::MONOSPACE));
    }
    body = body.push(Space::new().height(Length::Fixed(4.0)));
    body = body.push(text(cmd.to_string()).size(12).font(iced::Font::MONOSPACE).color(Color::from_rgb(0.84, 0.86, 1.0)));

    container(body).style(card_style).padding(2).into()
}

fn section_header(label: impl Into<String>) -> Element<'static, Message> {
    text(label.into().to_uppercase())
        .size(10)
        .color(Color::from_rgb(0.4, 0.43, 0.69))
        .into()
}

fn placeholder(msg: &str) -> Element<'_, Message> {
    container(text(msg).size(13).color(Color::from_rgb(0.6, 0.64, 0.83)))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn stub<'a>(title: &str, blurb: &str) -> Element<'a, Message> {
    container(
        column![
            text(title.to_string()).size(22),
            text(blurb.to_string()).size(13).color(Color::from_rgb(0.6, 0.64, 0.83)),
            Space::new().height(Length::Fixed(8.0)),
            text("Designed in doc/control-center.html. Implementation: v2/v3.")
                .size(11)
                .color(Color::from_rgb(0.4, 0.43, 0.69)),
        ]
        .spacing(4)
        .padding(20),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

// Container styles
fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.weak.color.into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
        ..Default::default()
    }
}

fn card_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
        ..Default::default()
    }
}
