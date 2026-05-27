//! The negotiation engine — the planner (slice 1).
//!
//! Given a subject type, a verb (its candidate instruments as [`VerbEdge`]s), a
//! set of [`Converter`]s, and a preference-ordered Accept profile, find the
//! minimum-cost pipeline that runs the verb and lands in Accept: inserting type
//! conversions *before* the verb (input coercion) and *after* it (output
//! negotiation), and choosing the instrument whose route is cheapest (`Using:`
//! selection). All three fall out of one two-layer Dijkstra. See
//! `doc/design/negotiation.md`.
//!
//! Slice 1 is the planner only — an in-memory converter set, no `[[channels]]`
//! schema parsing (slice 2) and no execution/materialization (slice 4). The
//! graph is **virtual**: a node's successors are computed on the fly by
//! lattice-matching (`is_subtype`), never stored as an adjacency structure.
//!
//! `plan` returns the single minimum-cost route. `300`-style enumeration of
//! equal-cost *alternatives* (for the picker / `--explain`) is a separate entry
//! point in a later slice (`pathfinding::astar_bag` or `dijkstra_all`), not a
//! retrofit of `plan`.

use crate::mime::is_subtype;
use pathfinding::prelude::dijkstra;
use serde_json::Value;

/// Declared cost semantics, mapped to a numeric weight in one place (§4). The
/// tier is deliberately coarse: it collapses several real axes (fidelity loss,
/// latency, heaviness) — a tuple-valued cost is a v2 refinement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Free,    // identity / no-op
    Cheap,   // lossless, local, fast (json pretty, base64)
    Normal,  // ordinary local transform (incl. a GUI launch — full fidelity, just heavy)
    Lossy,   // fidelity loss (image→ansi, transcode-down)
    Network, // remote round-trip
}

impl Tier {
    fn weight(self) -> u32 {
        match self {
            Tier::Free => 0,
            Tier::Cheap => 1,
            Tier::Normal => 4,
            Tier::Lossy => 16,
            Tier::Network => 32,
        }
    }

    /// Parse a `[[channels]]` `cost` string; unknown → `None` (validation flags it).
    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "free" => Some(Tier::Free),
            "cheap" => Some(Tier::Cheap),
            "normal" => Some(Tier::Normal),
            "lossy" => Some(Tier::Lossy),
            "network" => Some(Tier::Network),
            _ => None,
        }
    }
}

/// How a transformer consumes its input. Carried for the executor (slice 4),
/// where it drives buffer insertion; unused by slice-1 planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stream,
    Path,
    Bytes,
}

impl Mode {
    /// Parse a `[[channels]]` `consumes` string; unknown → `None`.
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "stream" => Some(Mode::Stream),
            "path" => Some(Mode::Path),
            "bytes" => Some(Mode::Bytes),
            _ => None,
        }
    }
}

/// A type→type transformer: a coercion channel (and the same entity a `Using:`
/// instrument is). `emits` is a concrete type, never a pattern (schema rule §2.5).
#[derive(Clone, Debug)]
pub struct Converter {
    pub name: String,
    pub accepts: Vec<String>, // lattice patterns
    // SCHEMA RULE (§2.5): `emits` must be a concrete type, never a pattern —
    // Dijkstra needs a node to land on. Slice 2's TOML validation enforces it.
    pub emits: String,
    pub cost: Tier,
    pub requires: Vec<String>, // env capabilities that gate usability
    pub consumes: Mode,
    pub cmd: String, // how it runs (executor, slice 4); empty in pure planner tests
}

/// A candidate (verb, instrument): the mandatory A→B transition. `emits == None`
/// is an identity edge — a `kind="present"` verb, where the subject *is* the
/// result and all the work is the output route.
#[derive(Clone, Debug)]
pub struct VerbEdge {
    // `""` = no named instrument (a plain or `kind="present"` verb).
    pub instrument: String,
    pub accepts: Vec<String>,
    pub emits: Option<String>,
    pub cost: Tier,
    pub requires: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepKind {
    Convert(String), // a converter, by name
    Verb(String),    // the verb, by instrument name ("" = no named instrument)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub from: String,
    pub to: String,
}

/// A planned pipeline: ordered steps (input coercions, the verb, output
/// coercions), the representation that matched Accept, and the route cost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub delivered: String,
    pub cost: u32, // route cost only; the preference-rank penalty is stripped
}

impl Plan {
    /// A stable, flat JSON shape for the plan — the contract the wasm/JS
    /// simulator mirrors and the conformance fixtures are dumped in.
    pub fn to_json(&self) -> Value {
        let steps: Vec<Value> = self
            .steps
            .iter()
            .map(|s| {
                let (kind, name) = match &s.kind {
                    StepKind::Convert(n) => ("convert", n.as_str()),
                    StepKind::Verb(n) => ("verb", n.as_str()),
                };
                serde_json::json!({ "kind": kind, "name": name, "from": s.from, "to": s.to })
            })
            .collect();
        serde_json::json!({ "steps": steps, "delivered": self.delivered, "cost": self.cost })
    }
}

// A Dijkstra node: a type in a layer (`false` = pre-verb A, `true` = post-verb
// B), or the synthetic Goal sink. Delivery edges (B→Goal) carry the
// preference-rank penalty, so preference is the primary sort and cost the
// tiebreaker.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Node {
    Ty(String, bool),
    Goal,
}

// Dominates any realistic route cost (weights are ≤ 32 over a handful of hops),
// so a lower-preference representation is chosen only when a higher-preferred one
// is unreachable.
const RANK_PENALTY: u32 = 1_000_000;

fn cap_ok(requires: &[String], env: &[String]) -> bool {
    requires.iter().all(|r| env.iter().any(|e| e == r))
}

/// Plan the cheapest pipeline, or `None` if no route reaches Accept (a `415`).
/// `accept` is preference-ordered (most-preferred first).
pub fn plan(
    subject_type: &str,
    verb_edges: &[VerbEdge],
    converters: &[Converter],
    accept: &[String],
    env_caps: &[String],
    reg: &Value,
) -> Option<Plan> {
    // Prune transformers whose env requirements aren't met (not a runtime fail).
    let usable: Vec<&Converter> = converters.iter().filter(|c| cap_ok(&c.requires, env_caps)).collect();
    let verbs: Vec<&VerbEdge> = verb_edges.iter().filter(|v| cap_ok(&v.requires, env_caps)).collect();

    let start = Node::Ty(subject_type.to_string(), false);
    let (path, total) = dijkstra(
        &start,
        |node| successors(node, &usable, &verbs, accept, reg),
        |node| *node == Node::Goal,
    )?;
    Some(reconstruct(path, total, &usable, &verbs, accept, reg))
}

/// A resolved presentation context: the preference-ordered Accept profile
/// (most-preferred first) and the available environment capabilities (which gate
/// converter `requires`). The `--as` / `--to` overrides and the env heuristic
/// both produce one of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub accept: Vec<String>,
    pub env_caps: Vec<String>,
}

fn owned(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The §12 thin heuristic: synthesize the default Target from environment
/// signals. `tty` = stdout is a terminal; `display` = a Wayland/X display is
/// present. A tty prefers inline ANSI; a display offers a surface; a
/// piped/redirected (non-tty) stdout is a plain byte sink.
pub fn target_from_env(tty: bool, display: bool) -> Target {
    match (tty, display) {
        // cosmic-terminal: both available, ANSI preferred over a popped window.
        (true, true) => Target {
            accept: owned(&["text/x-ansi", "text/plain", "application/vnd.goo.surface"]),
            env_caps: owned(&["pty", "display"]),
        },
        (true, false) => Target {
            accept: owned(&["text/x-ansi", "text/plain"]),
            env_caps: owned(&["pty"]),
        },
        // launcher-ish: a surface consumer with no inherited tty.
        (false, true) => Target {
            accept: owned(&["application/vnd.goo.surface", "*/*"]),
            env_caps: owned(&["display"]),
        },
        // piped / redirected: a byte sink takes anything.
        (false, false) => Target {
            accept: owned(&["*/*"]),
            env_caps: vec![],
        },
    }
}

impl Target {
    /// `--as <type>`: pin the Accept to exactly this representation (env_caps
    /// unchanged — the override says *what*, not *where*).
    pub fn with_accept(mut self, as_type: &str) -> Target {
        self.accept = vec![as_type.to_string()];
        self
    }
}

/// Build the verb's candidate edges (the mandatory A→B transition). A
/// `kind="present"` verb is an identity edge (the subject is the result). A verb
/// may declare `usage = [<channel name>, …]` — each names a channel (in
/// `[[channels]]`) that carries the verb out; the planner chooses among them
/// (filling the `Using:` slot), taking `emits`/`cost`/`requires` from the
/// channel. A plain verb (no `usage`) is carried out by its own `cmd`: one edge
/// whose `emits` is the verb's declared `emits` (default `text/plain`). See
/// goo-protocol §3 *Terminology* (channel + the use-axis; the chosen channel is
/// the "instrument").
pub fn verb_edges(verb: &Value, reg: &Value) -> Vec<VerbEdge> {
    let strvec = |v: &Value, k: &str| -> Vec<String> {
        v.get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let accepts = strvec(verb, "accepts");

    if verb.get("kind").and_then(Value::as_str) == Some("present") {
        return vec![VerbEdge { instrument: String::new(), accepts, emits: None, cost: Tier::Free, requires: vec![] }];
    }
    if let Some(usage) = verb.get("usage").and_then(Value::as_array) {
        return usage
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|name| {
                // Resolve the implementing channel; emits/cost/requires live there.
                let ch = reg
                    .get("channels")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|c| c.get("name").and_then(Value::as_str) == Some(name))?;
                Some(VerbEdge {
                    instrument: name.to_string(),
                    accepts: accepts.clone(),
                    emits: Some(ch.get("emits")?.as_str()?.to_string()),
                    cost: ch.get("cost").and_then(Value::as_str).and_then(Tier::parse).unwrap_or(Tier::Normal),
                    requires: strvec(ch, "requires"),
                })
            })
            .collect();
    }
    let emits = verb.get("emits").and_then(Value::as_str).unwrap_or("text/plain").to_string();
    vec![VerbEdge { instrument: String::new(), accepts, emits: Some(emits), cost: Tier::Normal, requires: vec![] }]
}

/// Top-level planning entry: pull converters from the registry, build the verb's
/// edges, and plan a route to the target's Accept. Pure — the surface the CLI
/// (`--explain`) and the wasm simulator both call.
pub fn plan_request(subject_type: &str, verb: &Value, target: &Target, reg: &Value) -> Option<Plan> {
    let convs = converters_from_registry(reg);
    let edges = verb_edges(verb, reg);
    plan(subject_type, &edges, &convs, &target.accept, &target.env_caps, reg)
}

/// Build the converter set from a registry's `[[channels]]` (slice 2). Skips
/// entries missing the essentials (`validate_channels` reports those); applies
/// defaults (`cost=normal`, `consumes=path`) for omitted fields.
pub fn converters_from_registry(reg: &Value) -> Vec<Converter> {
    reg.get("channels")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(channel_to_converter).collect())
        .unwrap_or_default()
}

fn channel_to_converter(ch: &Value) -> Option<Converter> {
    let name = ch.get("name")?.as_str()?.to_string();
    let emits = ch.get("emits")?.as_str()?.to_string();
    let accepts: Vec<String> = ch
        .get("accepts")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    if accepts.is_empty() {
        return None;
    }
    let str_vec = |k: &str| -> Vec<String> {
        ch.get(k)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    Some(Converter {
        name,
        accepts,
        emits,
        cost: ch.get("cost").and_then(Value::as_str).and_then(Tier::parse).unwrap_or(Tier::Normal),
        requires: str_vec("requires"),
        consumes: ch.get("consumes").and_then(Value::as_str).and_then(Mode::parse).unwrap_or(Mode::Path),
        cmd: ch.get("cmd").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

/// Validate `[[channels]]` entries (slice 2). Enforces the §2.5 schema rules so
/// the graph stays well-defined: `emits` concrete (no `*`), `accepts` non-empty,
/// and known `cost`/`consumes` vocab. Returns one message per problem.
pub fn validate_channels(reg: &Value) -> Vec<String> {
    let mut errs = Vec::new();
    let Some(arr) = reg.get("channels").and_then(Value::as_array) else { return errs };
    for ch in arr {
        let name = ch.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
        match ch.get("emits").and_then(Value::as_str) {
            None => errs.push(format!("channel \"{name}\" has no emits")),
            Some(e) if e.contains('*') => {
                errs.push(format!("channel \"{name}\" emits \"{e}\" — must be a concrete type, not a pattern"))
            }
            Some(_) => {}
        }
        let accepts_ok = ch.get("accepts").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
        if !accepts_ok {
            errs.push(format!("channel \"{name}\" needs a non-empty accepts list"));
        }
        if let Some(c) = ch.get("cost").and_then(Value::as_str) {
            if Tier::parse(c).is_none() {
                errs.push(format!("channel \"{name}\" has unknown cost tier \"{c}\" (free|cheap|normal|lossy|network)"));
            }
        }
        if let Some(m) = ch.get("consumes").and_then(Value::as_str) {
            if Mode::parse(m).is_none() {
                errs.push(format!("channel \"{name}\" has unknown consumes mode \"{m}\" (stream|path|bytes)"));
            }
        }
    }
    errs
}

fn successors(
    node: &Node,
    converters: &[&Converter],
    verbs: &[&VerbEdge],
    accept: &[String],
    reg: &Value,
) -> Vec<(Node, u32)> {
    let mut out = Vec::new();
    let Node::Ty(t, layer) = node else { return out };

    // Within-layer converter edges (both A and B).
    for c in converters {
        if c.accepts.iter().any(|p| is_subtype(t, p, reg)) {
            out.push((Node::Ty(c.emits.clone(), *layer), c.cost.weight()));
        }
    }
    if !*layer {
        // Verb edges A→B (the mandatory action; identity for `present`).
        for v in verbs {
            if v.accepts.iter().any(|p| is_subtype(t, p, reg)) {
                let to = v.emits.clone().unwrap_or_else(|| t.clone());
                out.push((Node::Ty(to, true), v.cost.weight()));
            }
        }
    } else if let Some(rank) = accept.iter().position(|p| is_subtype(t, p, reg)) {
        // Delivery edge B→Goal; the best (lowest) matching preference rank.
        out.push((Node::Goal, rank as u32 * RANK_PENALTY));
    }
    out
}

fn reconstruct(
    path: Vec<Node>,
    total: u32,
    converters: &[&Converter],
    verbs: &[&VerbEdge],
    accept: &[String],
    reg: &Value,
) -> Plan {
    let mut steps = Vec::new();
    let mut delivered = String::new();
    for win in path.windows(2) {
        match (&win[0], &win[1]) {
            // Same-layer hop = a converter.
            (Node::Ty(from, a), Node::Ty(to, b)) if a == b => {
                steps.push(Step {
                    kind: StepKind::Convert(pick_converter(from, to, converters, reg)),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            // A→B = the verb.
            (Node::Ty(from, false), Node::Ty(to, true)) => {
                steps.push(Step {
                    kind: StepKind::Verb(pick_verb(from, to, verbs, reg)),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            // B→Goal = delivery.
            (Node::Ty(from, true), Node::Goal) => delivered = from.clone(),
            _ => {}
        }
    }
    // Strip the rank penalty so `cost` is the route cost alone. The invariant
    // (route cost < RANK_PENALTY) holds for sane tier weights; saturating_sub +
    // debug_assert guard against a future tier inflated past the penalty.
    let rank = accept.iter().position(|p| is_subtype(&delivered, p, reg)).unwrap_or(0) as u32;
    let penalty = rank * RANK_PENALTY;
    debug_assert!(penalty <= total, "route cost must stay below RANK_PENALTY");
    Plan { steps, delivered, cost: total.saturating_sub(penalty) }
}

// Reconstruction re-derives which edge Dijkstra used by matching (from, to) and
// taking the cheapest candidate — consistent with what the search relaxed.
// Tiebreaker when two edges share endpoints *and* cost: the first in slice order
// (Vec iteration). Deterministic, and the choice the slice-4 executor will run —
// so converter/instrument ordering in the registry is the authority on ties.
fn pick_converter(from: &str, to: &str, converters: &[&Converter], reg: &Value) -> String {
    converters
        .iter()
        .filter(|c| c.emits == to && c.accepts.iter().any(|p| is_subtype(from, p, reg)))
        .min_by_key(|c| c.cost.weight())
        .map(|c| c.name.clone())
        .unwrap_or_default()
}

fn pick_verb(from: &str, to: &str, verbs: &[&VerbEdge], reg: &Value) -> String {
    verbs
        .iter()
        .filter(|v| {
            v.accepts.iter().any(|p| is_subtype(from, p, reg))
                && match &v.emits {
                    Some(e) => e == to,
                    None => from == to, // identity (present)
                }
        })
        .min_by_key(|v| v.cost.weight())
        .map(|v| v.instrument.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn conv(name: &str, accepts: &[&str], emits: &str, cost: Tier) -> Converter {
        Converter {
            name: name.into(),
            accepts: accepts.iter().map(|s| s.to_string()).collect(),
            emits: emits.into(),
            cost,
            requires: vec![],
            consumes: Mode::Path,
            cmd: String::new(),
        }
    }
    fn conv_req(name: &str, accepts: &[&str], emits: &str, cost: Tier, requires: &[&str]) -> Converter {
        let mut c = conv(name, accepts, emits, cost);
        c.requires = requires.iter().map(|s| s.to_string()).collect();
        c
    }
    fn verb(instrument: &str, accepts: &[&str], emits: Option<&str>, cost: Tier) -> VerbEdge {
        VerbEdge {
            instrument: instrument.into(),
            accepts: accepts.iter().map(|s| s.to_string()).collect(),
            emits: emits.map(|s| s.to_string()),
            cost,
            requires: vec![],
        }
    }
    fn present(accepts: &[&str]) -> VerbEdge {
        verb("", accepts, None, Tier::Free)
    }
    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // Input coercion: csv→json before a json-accepting verb. The converter lands
    // in layer A so the verb edge unlocks.
    #[test]
    fn input_coercion() {
        let reg = j!({});
        let verbs = [verb("jq", &["application/json"], Some("application/json"), Tier::Normal)];
        let convs = [conv("csv2json", &["text/csv"], "application/json", Tier::Cheap)];
        let p = plan("text/csv", &verbs, &convs, &strs(&["application/json"]), &[], &reg).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Convert("csv2json".into()));
        assert_eq!(p.steps[1].kind, StepKind::Verb("jq".into()));
        assert_eq!(p.delivered, "application/json");
    }

    // Multi-hop coercion: the algorithm actually *chains* converters
    // (csv→tsv→json), not just direct edges.
    #[test]
    fn multi_hop_coercion_chains() {
        let reg = j!({});
        let verbs = [verb("jq", &["application/json"], Some("application/json"), Tier::Normal)];
        let convs = [
            conv("csv2tsv", &["text/csv"], "text/tab-separated-values", Tier::Cheap),
            conv("tsv2json", &["text/tab-separated-values"], "application/json", Tier::Cheap),
        ];
        let p = plan("text/csv", &verbs, &convs, &strs(&["application/json"]), &[], &reg).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Convert("csv2tsv".into()));
        assert_eq!(p.steps[1].kind, StepKind::Convert("tsv2json".into()));
        assert_eq!(p.steps[2].kind, StepKind::Verb("jq".into()));
    }

    // Minimum-cost, not just *a* path: a direct converter beats a two-hop route.
    #[test]
    fn cheapest_route_wins() {
        let reg = j!({});
        let verbs = [verb("jq", &["application/json"], Some("application/json"), Tier::Normal)];
        let convs = [
            conv("csv2tsv", &["text/csv"], "text/tab-separated-values", Tier::Cheap),
            conv("tsv2json", &["text/tab-separated-values"], "application/json", Tier::Cheap),
            conv("csv2json", &["text/csv"], "application/json", Tier::Cheap), // 1 hop vs 2
        ];
        let p = plan("text/csv", &verbs, &convs, &strs(&["application/json"]), &[], &reg).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Convert("csv2json".into()));
        assert_eq!(p.steps.len(), 2); // direct convert + verb
    }

    // Output negotiation: image→ansi after an identity (present) verb. Same
    // algorithm, the other side of the verb edge.
    #[test]
    fn output_negotiation() {
        let reg = j!({});
        let verbs = [present(&["image/*"])];
        let convs = [conv("chafa", &["image/*"], "text/x-ansi", Tier::Lossy)];
        let p = plan("image/png", &verbs, &convs, &strs(&["text/x-ansi"]), &[], &reg).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Verb("".into()));
        assert_eq!(p.steps[1].kind, StepKind::Convert("chafa".into()));
        assert_eq!(p.delivered, "text/x-ansi");
    }

    // Using: selection — the planner picks the instrument whose route reaches
    // Accept; `assemble`'s prompt output is a dead end for a text/plain Accept.
    #[test]
    fn instrument_selection() {
        let reg = j!({});
        let verbs = [
            verb("fabric/inference", &["text/*"], Some("text/plain"), Tier::Normal),
            verb("fabric/assemble", &["text/*"], Some("application/vnd.goo.prompt"), Tier::Normal),
        ];
        let p = plan("text/plain", &verbs, &[], &strs(&["text/plain"]), &[], &reg).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Verb("fabric/inference".into()));
        assert_eq!(p.delivered, "text/plain");
    }

    // Preference: a cosmic-terminal accepts both ansi and a surface, ranks ansi
    // first → chafa (lossy) wins over eog (cheaper) because preference dominates.
    #[test]
    fn preference_orders_above_cost() {
        let reg = j!({});
        let verbs = [present(&["image/*"])];
        let convs = [
            conv("chafa", &["image/*"], "text/x-ansi", Tier::Lossy),
            conv_req("eog", &["image/*"], "application/vnd.wayland.surface", Tier::Normal, &["display"]),
        ];
        let accept = strs(&["text/x-ansi", "application/vnd.wayland.surface"]);
        let p = plan("image/png", &verbs, &convs, &accept, &strs(&["display", "pty"]), &reg).unwrap();
        assert_eq!(p.delivered, "text/x-ansi");
        assert_eq!(p.steps[1].kind, StepKind::Convert("chafa".into()));
    }

    // Bare desktop: only a surface is accepted → the GUI converter route.
    #[test]
    fn desktop_routes_to_surface() {
        let reg = j!({});
        let verbs = [present(&["image/*"])];
        let convs = [
            conv("chafa", &["image/*"], "text/x-ansi", Tier::Lossy),
            conv_req("eog", &["image/*"], "application/vnd.wayland.surface", Tier::Normal, &["display"]),
        ];
        let accept = strs(&["application/vnd.wayland.surface"]);
        let p = plan("image/png", &verbs, &convs, &accept, &strs(&["display"]), &reg).unwrap();
        assert_eq!(p.delivered, "application/vnd.wayland.surface");
        assert_eq!(p.steps[1].kind, StepKind::Convert("eog".into()));
    }

    // `requires` gates a converter: no display ⇒ eog is pruned ⇒ no route.
    #[test]
    fn requires_gates_converter() {
        let reg = j!({});
        let verbs = [present(&["image/*"])];
        let convs = [conv_req("eog", &["image/*"], "application/vnd.wayland.surface", Tier::Normal, &["display"])];
        let accept = strs(&["application/vnd.wayland.surface"]);
        assert!(plan("image/png", &verbs, &convs, &accept, &[], &reg).is_none());
    }

    // No converter bridges the gap ⇒ None (a 415).
    #[test]
    fn no_route_is_none() {
        let reg = j!({});
        let verbs = [present(&["image/*"])];
        assert!(plan("image/png", &verbs, &[], &strs(&["text/plain"]), &[], &reg).is_none());
    }

    // The common case: subject already deliverable post-verb — just the verb, no
    // converters, zero cost.
    #[test]
    fn identity_no_converters() {
        let reg = j!({});
        let verbs = [present(&["text/*"])];
        let p = plan("text/plain", &verbs, &[], &strs(&["text/plain"]), &[], &reg).unwrap();
        assert_eq!(p.steps.len(), 1);
        assert_eq!(p.steps[0].kind, StepKind::Verb("".into()));
        assert_eq!(p.cost, 0);
    }

    // ---- slice 2: [[channels]] schema → converter set ----

    #[test]
    fn converters_parse_from_registry() {
        let reg = j!({ "channels": [
            { "name": "chafa", "accepts": ["image/*"], "emits": "text/x-ansi",
              "cost": "lossy", "consumes": "path", "cmd": "chafa {in.path|q}" },
            { "name": "eog", "accepts": ["image/*"], "emits": "application/vnd.wayland.surface",
              "requires": ["display"] },  // defaults: cost=normal, consumes=path
        ]});
        let cs = converters_from_registry(&reg);
        assert_eq!(cs.len(), 2);
        let chafa = &cs[0];
        assert_eq!(chafa.name, "chafa");
        assert_eq!(chafa.emits, "text/x-ansi");
        assert_eq!(chafa.cost, Tier::Lossy);
        assert_eq!(chafa.consumes, Mode::Path);
        assert_eq!(chafa.cmd, "chafa {in.path|q}");
        let eog = &cs[1];
        assert_eq!(eog.cost, Tier::Normal); // default
        assert_eq!(eog.requires, vec!["display".to_string()]);
    }

    // The parsed converters drive the planner end-to-end.
    #[test]
    fn parsed_converters_feed_the_planner() {
        let reg = j!({ "channels": [
            { "name": "chafa", "accepts": ["image/*"], "emits": "text/x-ansi", "cost": "lossy" }
        ]});
        let cs = converters_from_registry(&reg);
        let verbs = [present(&["image/*"])];
        let p = plan("image/png", &verbs, &cs, &strs(&["text/x-ansi"]), &[], &reg).unwrap();
        assert_eq!(p.steps[1].kind, StepKind::Convert("chafa".into()));
    }

    #[test]
    fn validate_flags_pattern_emits_and_empty_accepts() {
        let reg = j!({ "channels": [
            { "name": "bad-emit", "accepts": ["image/*"], "emits": "text/*" },   // pattern emit
            { "name": "no-accept", "accepts": [], "emits": "text/plain" },        // empty accepts
            { "name": "bad-cost", "accepts": ["text/*"], "emits": "text/plain", "cost": "huge" },
            { "name": "ok", "accepts": ["image/*"], "emits": "text/x-ansi", "cost": "lossy" },
        ]});
        let errs = validate_channels(&reg);
        assert!(errs.iter().any(|e| e.contains("bad-emit") && e.contains("concrete")));
        assert!(errs.iter().any(|e| e.contains("no-accept") && e.contains("non-empty")));
        assert!(errs.iter().any(|e| e.contains("bad-cost") && e.contains("unknown cost")));
        assert!(!errs.iter().any(|e| e.contains("\"ok\"")));
    }

    #[test]
    fn validate_clean_when_no_channels() {
        assert!(validate_channels(&j!({})).is_empty());
    }

    // ---- slice 3: accept derivation, verb edges, plan_request ----

    #[test]
    fn env_synthesizes_accept_profiles() {
        assert_eq!(target_from_env(true, false).accept, owned(&["text/x-ansi", "text/plain"]));
        // cosmic-terminal: ansi preferred over surface.
        let ct = target_from_env(true, true);
        assert_eq!(ct.accept[0], "text/x-ansi");
        assert!(ct.accept.contains(&"application/vnd.goo.surface".to_string()));
        assert_eq!(ct.env_caps, owned(&["pty", "display"]));
        // piped/redirected: a byte sink.
        assert_eq!(target_from_env(false, false).accept, owned(&["*/*"]));
    }

    #[test]
    fn as_override_pins_accept() {
        let t = target_from_env(true, false).with_accept("application/json");
        assert_eq!(t.accept, vec!["application/json".to_string()]);
        assert_eq!(t.env_caps, owned(&["pty"])); // override says what, not where
    }

    #[test]
    fn verb_edges_present_is_identity() {
        let v = j!({ "name": "view", "kind": "present", "accepts": ["image/*"] });
        let e = verb_edges(&v, &j!({}));
        assert_eq!(e.len(), 1);
        assert!(e[0].emits.is_none());
        assert_eq!(e[0].cost, Tier::Free);
    }

    // A verb's `usage` names channels; emits/cost come from the channel.
    #[test]
    fn verb_edges_resolves_usage_channels() {
        let reg = j!({ "channels": [
            { "name": "fabric/inference", "accepts": ["text/*"], "emits": "text/plain", "cost": "network", "cmd": "fabric ..." },
            { "name": "fabric/assemble", "accepts": ["text/*"], "emits": "application/vnd.goo.prompt", "cost": "cheap", "cmd": "cat ..." },
        ]});
        let v = j!({ "name": "summarize", "accepts": ["text/*"],
            "usage": ["fabric/inference", "fabric/assemble"] });
        let e = verb_edges(&v, &reg);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].instrument, "fabric/inference");
        assert_eq!(e[0].emits.as_deref(), Some("text/plain")); // from the channel
        assert_eq!(e[1].cost, Tier::Cheap); // from the channel
    }

    // A plain verb (no usage) is carried out by its own cmd: one edge.
    #[test]
    fn verb_edges_plain_verb_single_edge() {
        let v = j!({ "name": "json-keys", "accepts": ["application/json"], "emits": "text/plain" });
        let e = verb_edges(&v, &j!({}));
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].instrument, "");
        assert_eq!(e[0].emits.as_deref(), Some("text/plain"));
    }

    // A curated demo registry: the surface type hierarchy + image converters +
    // a present `view` verb. The SAME request plans differently by environment.
    fn demo_reg() -> Value {
        j!({
            "types": [
                { "name": "application/vnd.wayland.surface", "is_a": ["application/vnd.goo.surface"] }
            ],
            "channels": [
                { "name": "chafa", "accepts": ["image/*"], "emits": "text/x-ansi", "cost": "lossy" },
                { "name": "eog", "accepts": ["image/*"], "emits": "application/vnd.wayland.surface",
                  "cost": "normal", "requires": ["display"] }
            ]
        })
    }

    // Conformance contract for site/goo-simulator.html: the dumped golden plans
    // (site/demo-plans.json) must match what plan_request produces *now*. A Rust
    // change that alters planning without regenerating the fixtures fails here —
    // and the simulator's JS planner is checked against the same fixtures on load.
    #[test]
    fn demo_plans_match_fixtures() {
        use std::path::PathBuf;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../simulator");
        let read = |f: &str| -> Value {
            serde_json::from_str(&std::fs::read_to_string(dir.join(f)).unwrap()).unwrap()
        };
        let reg = read("demo-registry.json");
        let scenarios = read("scenarios.json");
        let golden = read("demo-plans.json");
        let scs = scenarios["scenarios"].as_array().unwrap();
        for sc in scs {
            let id = sc["id"].as_str().unwrap();
            let verb = reg["verbs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"].as_str() == sc["verb"].as_str())
                .unwrap();
            let mut target = target_from_env(
                sc["env"]["tty"].as_bool().unwrap_or(false),
                sc["env"]["display"].as_bool().unwrap_or(false),
            );
            if let Some(a) = sc.get("as").and_then(Value::as_str) {
                target = target.with_accept(a);
            }
            let got = plan_request(sc["subject"].as_str().unwrap(), verb, &target, &reg)
                .map(|p| p.to_json())
                .unwrap_or(Value::Null);
            assert_eq!(
                &got, &golden[id],
                "scenario '{id}' drifted — regenerate: cargo run --example dump_plans -p goo-engine"
            );
        }
        assert_eq!(golden.as_object().unwrap().len(), scs.len(), "stale ids in demo-plans.json");
    }

    #[test]
    fn plan_request_routes_by_environment() {
        let reg = demo_reg();
        let view = j!({ "name": "view", "kind": "present", "accepts": ["image/*"] });

        // bare tty → chafa → ansi
        let tty = plan_request("image/png", &view, &target_from_env(true, false), &reg).unwrap();
        assert_eq!(tty.delivered, "text/x-ansi");
        assert_eq!(tty.steps[1].kind, StepKind::Convert("chafa".into()));

        // bare desktop → eog → surface (the lattice resolves wayland.surface ⊑ goo.surface)
        let desktop = plan_request("image/png", &view, &target_from_env(false, true), &reg).unwrap();
        assert_eq!(desktop.delivered, "application/vnd.wayland.surface");
        assert_eq!(desktop.steps[1].kind, StepKind::Convert("eog".into()));

        // cosmic-terminal (pty+display) → ansi preferred over the cheaper surface
        let ct = plan_request("image/png", &view, &target_from_env(true, true), &reg).unwrap();
        assert_eq!(ct.delivered, "text/x-ansi");
    }
}
