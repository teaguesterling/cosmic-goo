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
}

/// How a transformer consumes its input. Carried for the executor (slice 4),
/// where it drives buffer insertion; unused by slice-1 planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stream,
    Path,
    Bytes,
}

/// A type→type transformer: a coercion channel (and the same entity a `Using:`
/// instrument is). `emits` is a concrete type, never a pattern (schema rule §2.5).
#[derive(Clone, Debug)]
pub struct Converter {
    pub name: String,
    pub accepts: Vec<String>, // lattice patterns
    pub emits: String,        // concrete type
    pub cost: Tier,
    pub requires: Vec<String>, // env capabilities that gate usability
    pub consumes: Mode,
}

/// A candidate (verb, instrument): the mandatory A→B transition. `emits == None`
/// is an identity edge — a `kind="present"` verb, where the subject *is* the
/// result and all the work is the output route.
#[derive(Clone, Debug)]
pub struct VerbEdge {
    pub instrument: String, // "" for a plain/present verb with no named instrument
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
    // Strip the rank penalty so `cost` is the route cost alone.
    let rank = accept.iter().position(|p| is_subtype(&delivered, p, reg)).unwrap_or(0) as u32;
    Plan { steps, delivered, cost: total - rank * RANK_PENALTY }
}

// Reconstruction re-derives which edge Dijkstra used by matching (from, to) and
// taking the cheapest candidate — consistent with what the search relaxed.
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
}
