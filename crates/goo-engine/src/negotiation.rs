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
use pathfinding::prelude::{dijkstra, yen};
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
    // The binary the `cmd` needs (`mlr`, `chafa`). `None` = no PATH dependency
    // (always usable). Orthogonal to `requires` (env capability vs binary
    // presence): the planner prunes a channel whose `tool` isn't on PATH, so a
    // route around an uninstalled tool is found, or a 415 names what's missing.
    pub tool: Option<String>,
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
    // type, layer (false = pre-verb A, true = post-verb B), and converter hops
    // taken *within this layer* (for the earned-hops bound, §4.1). The hop count
    // is part of node identity so Dijkstra prunes per-layer depth.
    Ty(String, bool, u8),
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
/// `accept` is preference-ordered (most-preferred first). This is the unbounded
/// convenience entry (no per-layer hop cap) — it delegates to [`plan_bounded`]
/// with `u8::MAX` on both layers. The earned-hops caps (§4.1) enter via
/// `plan_bounded`; this wrapper keeps the pure-planner tests' intent ("any depth")
/// explicit.
pub fn plan(
    subject_type: &str,
    verb_edges: &[VerbEdge],
    converters: &[Converter],
    accept: &[String],
    env_caps: &[String],
    reg: &Value,
) -> Option<Plan> {
    plan_bounded(subject_type, verb_edges, converters, accept, env_caps, reg, u8::MAX, u8::MAX)
}

/// As [`plan`], but bounding the number of *converter* hops taken within each
/// layer: at most `max_hops_a` before the verb (input coercion) and `max_hops_b`
/// after it (output negotiation). The verb edge itself is never a converter hop.
/// This is the earned-hops model (§4.1): a caller grants depth per axis from the
/// explicit slots the user supplied (`--hops`, `--as`/`--to`); the default is
/// tight and deeper routes must be earned.
pub fn plan_bounded(
    subject_type: &str,
    verb_edges: &[VerbEdge],
    converters: &[Converter],
    accept: &[String],
    env_caps: &[String],
    reg: &Value,
    max_hops_a: u8,
    max_hops_b: u8,
) -> Option<Plan> {
    // Prune transformers whose env requirements aren't met (not a runtime fail).
    let usable: Vec<&Converter> = converters.iter().filter(|c| cap_ok(&c.requires, env_caps)).collect();
    let verbs: Vec<&VerbEdge> = verb_edges.iter().filter(|v| cap_ok(&v.requires, env_caps)).collect();

    let start = Node::Ty(subject_type.to_string(), false, 0);
    let (path, total) = dijkstra(
        &start,
        |node| successors(node, &usable, &verbs, accept, reg, max_hops_a, max_hops_b),
        |node| *node == Node::Goal,
    )?;
    Some(reconstruct(path, total, &usable, &verbs, accept, reg))
}

/// Per-layer converter-hop budget — the earned-hops model (§4.1). `a` bounds
/// input coercion (pre-verb, layer A); `b` bounds output negotiation (post-verb,
/// layer B). The default is the tight `(1, 1)`: one *implicit* hop on each side, so
/// a deeper route is **earned, not free**. `--hops N` raises layer A; `--force`
/// makes both unbounded. The verb edge and the delivery edge are never hops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hops {
    pub a: u8,
    pub b: u8,
}

impl Default for Hops {
    fn default() -> Self {
        Hops { a: 1, b: 1 }
    }
}

impl Hops {
    /// No bound on either layer — `--force`, and the high bound stage 3's teaching
    /// 415 re-searches at to discover the route it should suggest.
    pub fn unbounded() -> Self {
        Hops { a: u8::MAX, b: u8::MAX }
    }

    /// `--hops N`: raise the input-coercion (layer A) budget. Layer B stays at the
    /// default ≤1 — "more hops" means a longer *input* chain, never extra output
    /// negotiation (that one case wants `--force`; §4.1).
    pub fn with_layer_a(self, n: u8) -> Self {
        Hops { a: n, ..self }
    }
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
/// (`--explain`) and the wasm simulator both call. `available_tools` is the set
/// of channel `tool` binaries present on PATH (the bin probes it; the planner
/// prunes channels whose tool is declared but absent — routing around an
/// uninstalled tool, or to a 415).
pub fn plan_request(subject_type: &str, verb: &Value, target: &Target, reg: &Value, available_tools: &[String]) -> Option<Plan> {
    plan_request_using(subject_type, verb, target, reg, available_tools, None)
}

/// As [`plan_request`], but with an optional `--using` pin: when `Some(channel)`,
/// the verb is forced through that one `usage` channel (the planner's other
/// candidate channels are dropped). A *constraint*, not a hint — if the pinned
/// channel has no route, the result is `None` (a 415), never a fallback. (The
/// CLI validates that the channel is actually one of the verb's `usage` before
/// calling, for a clean error.)
pub fn plan_request_using(
    subject_type: &str,
    verb: &Value,
    target: &Target,
    reg: &Value,
    available_tools: &[String],
    using: Option<&str>,
) -> Option<Plan> {
    plan_request_using_bounded(subject_type, verb, target, reg, available_tools, using, Hops::default())
}

/// As [`plan_request_using`], but with an explicit per-layer hop budget (§4.1).
/// The CLI calls this with caps derived from the user's flags — default
/// `Hops::default()` = (1,1), `--hops N` raises layer A, `--force` is
/// `Hops::unbounded()`. The two thinner entries above delegate here with the
/// default so existing callers (and the pure-planner tests) keep the earned-hops
/// default without churn.
pub fn plan_request_using_bounded(
    subject_type: &str,
    verb: &Value,
    target: &Target,
    reg: &Value,
    available_tools: &[String],
    using: Option<&str>,
    hops: Hops,
) -> Option<Plan> {
    let convs: Vec<Converter> = converters_from_registry(reg)
        .into_iter()
        .filter(|c| tool_present(&c.tool, available_tools))
        .collect();
    let edges: Vec<VerbEdge> = verb_edges(verb, reg)
        .into_iter()
        .filter(|e| usage_tool_present(e, reg, available_tools))
        .filter(|e| using.is_none_or(|u| e.instrument == u))
        .collect();
    plan_bounded(subject_type, &edges, &convs, &target.accept, &target.env_caps, reg, hops.a, hops.b)
}

/// Plan the cheapest route over ALL of a subject's membership types — its content
/// `type` plus any provenance facets (a file is also `inode/file`; see
/// `verbs::subject_types`). Each membership is a valid "the subject is this type"
/// starting point; a verb that directly accepts a facet (`open` on a file) yields a
/// cost-0 route, while a content verb routes from the content type as before. No
/// single route mixes memberships — converters operate on content types, the facet
/// only adds a direct verb match — so planning per start type and taking the minimum
/// is complete. With one membership this is exactly [`plan_request_using_bounded`].
pub fn plan_request_over(
    subject_types: &[&str],
    verb: &Value,
    target: &Target,
    reg: &Value,
    available_tools: &[String],
    using: Option<&str>,
    hops: Hops,
) -> Option<Plan> {
    subject_types
        .iter()
        .filter_map(|t| plan_request_using_bounded(t, verb, target, reg, available_tools, using, hops))
        .min_by_key(|p| p.cost)
}

fn tool_present(tool: &Option<String>, available: &[String]) -> bool {
    tool.as_ref().is_none_or(|t| available.iter().any(|a| a == t))
}

/// A verb edge's instrument is a `usage` channel name; prune the edge if that
/// channel declares a `tool` that isn't present. Plain/present edges (no
/// instrument) have no tool dependency.
fn usage_tool_present(edge: &VerbEdge, reg: &Value, available: &[String]) -> bool {
    if edge.instrument.is_empty() {
        return true;
    }
    let tool = reg
        .get("channels")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|c| c.get("name").and_then(Value::as_str) == Some(edge.instrument.as_str())))
        .and_then(|c| c.get("tool").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(String::from);
    tool_present(&tool, available)
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
        tool: ch.get("tool").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from),
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
        // An empty `tool` is author confusion — omit the field for "no tool".
        if ch.get("tool").and_then(Value::as_str) == Some("") {
            errs.push(format!("channel \"{name}\" has an empty tool (omit `tool` for no PATH dependency)"));
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
    max_hops_a: u8,
    max_hops_b: u8,
) -> Vec<(Node, u32)> {
    let mut out = Vec::new();
    let Node::Ty(t, layer, hops) = node else { return out };

    // Within-layer converter edges (both A and B), bounded by the earned-hops cap
    // for this layer (§4.1). Each converter step increments the layer's hop count;
    // when it reaches the cap, no further converter edges are offered (but the verb
    // edge and delivery below are unaffected — the cap limits *coercion* depth).
    let cap = if *layer { max_hops_b } else { max_hops_a };
    if *hops < cap {
        for c in converters {
            if c.accepts.iter().any(|p| is_subtype(t, p, reg)) {
                out.push((Node::Ty(c.emits.clone(), *layer, hops + 1), c.cost.weight()));
            }
        }
    }
    if !*layer {
        // Verb edges A→B (the mandatory action; identity for `present`). Crossing
        // the verb resets the hop count: layer B earns its own coercion budget.
        for v in verbs {
            if v.accepts.iter().any(|p| is_subtype(t, p, reg)) {
                let to = v.emits.clone().unwrap_or_else(|| t.clone());
                out.push((Node::Ty(to, true, 0), v.cost.weight()));
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
            // Same-layer hop = a converter (layers equal; hop counts ignored).
            (Node::Ty(from, a, _), Node::Ty(to, b, _)) if a == b => {
                steps.push(Step {
                    kind: StepKind::Convert(pick_converter(from, to, converters, reg)),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            // A→B = the verb (layer false → true).
            (Node::Ty(from, false, _), Node::Ty(to, true, _)) => {
                steps.push(Step {
                    kind: StepKind::Verb(pick_verb(from, to, verbs, reg)),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
            // B→Goal = delivery.
            (Node::Ty(from, true, _), Node::Goal) => delivered = from.clone(),
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

// ---- route enumeration (§4.2): "all the ways A→B" for `--explain --paths` ----

// A *thin* enumeration node — type + layer, NO hop counter. The planner's `Node`
// carries hops so Dijkstra prunes per-layer depth; but for `yen` k-shortest
// enumeration, a hop counter in identity would split one type-sequence into many
// "paths" that differ only by internal count (advisor trap #1). So enumerate on
// this hopless node — every distinct path is a distinct type sequence — and bound
// depth by counting converter hops *after* the fact.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum EnumNode {
    Ty(String, bool),
    Goal,
}

fn successors_enum(
    node: &EnumNode,
    converters: &[&Converter],
    verbs: &[&VerbEdge],
    accept: &[String],
    reg: &Value,
) -> Vec<(EnumNode, u32)> {
    let mut out = Vec::new();
    let EnumNode::Ty(t, layer) = node else { return out };
    for c in converters {
        if c.accepts.iter().any(|p| is_subtype(t, p, reg)) {
            out.push((EnumNode::Ty(c.emits.clone(), *layer), c.cost.weight()));
        }
    }
    if !*layer {
        for v in verbs {
            if v.accepts.iter().any(|p| is_subtype(t, p, reg)) {
                let to = v.emits.clone().unwrap_or_else(|| t.clone());
                out.push((EnumNode::Ty(to, true), v.cost.weight()));
            }
        }
    } else if let Some(rank) = accept.iter().position(|p| is_subtype(t, p, reg)) {
        out.push((EnumNode::Goal, rank as u32 * RANK_PENALTY));
    }
    out
}

fn plan_from_enum_path(path: &[EnumNode], total: u32, converters: &[&Converter], verbs: &[&VerbEdge], accept: &[String], reg: &Value) -> Plan {
    let mut steps = Vec::new();
    let mut delivered = String::new();
    for win in path.windows(2) {
        match (&win[0], &win[1]) {
            (EnumNode::Ty(from, a), EnumNode::Ty(to, b)) if a == b => {
                steps.push(Step { kind: StepKind::Convert(pick_converter(from, to, converters, reg)), from: from.clone(), to: to.clone() });
            }
            (EnumNode::Ty(from, false), EnumNode::Ty(to, true)) => {
                steps.push(Step { kind: StepKind::Verb(pick_verb(from, to, verbs, reg)), from: from.clone(), to: to.clone() });
            }
            (EnumNode::Ty(from, true), EnumNode::Goal) => delivered = from.clone(),
            _ => {}
        }
    }
    let rank = accept.iter().position(|p| is_subtype(&delivered, p, reg)).unwrap_or(0) as u32;
    Plan { steps, delivered, cost: total.saturating_sub(rank * RANK_PENALTY) }
}

/// Per-layer converter-hop counts of a plan: `(input, output)` — converters before
/// the verb edge, and after it.
fn layer_hops(plan: &Plan) -> (u8, u8) {
    let (mut a, mut b, mut seen_verb) = (0u8, 0u8, false);
    for s in &plan.steps {
        match &s.kind {
            StepKind::Verb(_) => seen_verb = true,
            StepKind::Convert(_) if !seen_verb => a = a.saturating_add(1),
            StepKind::Convert(_) => b = b.saturating_add(1),
        }
    }
    (a, b)
}

/// Enumerate the distinct routes (cost-ranked, up to `k`) from `subject_type`
/// through the verb to a satisfiable `Accept`, keeping those within `max_hops`
/// converter hops *per layer* (§4.2). The route-graph debugger behind
/// `goo --explain <verb> <subj> --paths`. Each result is a [`Plan`] (same shape
/// the planner emits); the first is the route `plan` itself would pick.
pub fn enumerate(
    subject_type: &str,
    verb_edges: &[VerbEdge],
    converters: &[Converter],
    accept: &[String],
    env_caps: &[String],
    reg: &Value,
    max_hops: u8,
    k: usize,
) -> Vec<Plan> {
    let usable: Vec<&Converter> = converters.iter().filter(|c| cap_ok(&c.requires, env_caps)).collect();
    let verbs: Vec<&VerbEdge> = verb_edges.iter().filter(|v| cap_ok(&v.requires, env_caps)).collect();
    let start = EnumNode::Ty(subject_type.to_string(), false);
    yen(
        &start,
        |node| successors_enum(node, &usable, &verbs, accept, reg),
        |node| *node == EnumNode::Goal,
        k,
    )
    .into_iter()
    .map(|(path, total)| plan_from_enum_path(&path, total, &usable, &verbs, accept, reg))
    .filter(|plan| {
        let (a, b) = layer_hops(plan);
        a <= max_hops && b <= max_hops
    })
    .collect()
}

/// As [`enumerate`], but pulling the converter set / verb edges from the registry
/// (the surface the CLI's `--paths` calls). Prunes channels whose `tool` is absent.
pub fn enumerate_request(
    subject_type: &str,
    verb: &Value,
    target: &Target,
    reg: &Value,
    available_tools: &[String],
    max_hops: u8,
    k: usize,
) -> Vec<Plan> {
    let convs: Vec<Converter> = converters_from_registry(reg)
        .into_iter()
        .filter(|c| tool_present(&c.tool, available_tools))
        .collect();
    let edges: Vec<VerbEdge> = verb_edges(verb, reg)
        .into_iter()
        .filter(|e| usage_tool_present(e, reg, available_tools))
        .collect();
    enumerate(subject_type, &edges, &convs, &target.accept, &target.env_caps, reg, max_hops, k)
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
            tool: None,
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

    // Membership multi-start (file-vs-data): a provenance facet (a file is also an
    // inode/file) is a valid alternative start type, so a verb accepting inode/file
    // matches a text/csv *file* directly — without the facet there's no route (415).
    #[test]
    fn plan_request_over_uses_a_membership_facet_for_a_direct_match() {
        let reg = j!({});
        let verb = j!({ "name": "open", "accepts": ["inode/file"], "emits": "application/x-opened" });
        let target = Target { accept: strs(&["application/x-opened"]), env_caps: vec![] };
        assert!(
            plan_request_over(&["text/csv"], &verb, &target, &reg, &[], None, Hops::default()).is_none(),
            "content type alone should not reach an inode/file verb"
        );
        assert!(
            plan_request_over(&["text/csv", "inode/file"], &verb, &target, &reg, &[], None, Hops::default()).is_some(),
            "the inode/file membership should give a direct match"
        );
    }

    // When BOTH memberships yield a route, the cheapest wins — the key positive that
    // the existing test (facet-where-content-has-none) doesn't cover. Here the verb
    // accepts json (reachable from text/csv only via a converter hop) AND inode/file
    // (direct), so the facet route is cheaper and must be the one chosen.
    #[test]
    fn plan_request_over_takes_the_cheaper_of_two_membership_routes() {
        let reg = j!({ "channels": [
            { "name": "csv2json", "accepts": ["text/csv"], "emits": "application/json", "cost": "cheap", "cmd": "x" }
        ]});
        let verb = j!({ "name": "v", "accepts": ["application/json", "inode/file"], "emits": "application/x-out" });
        let target = Target { accept: strs(&["application/x-out"]), env_caps: vec![] };
        let p = plan_request_over(&["text/csv", "inode/file"], &verb, &target, &reg, &[], None, Hops::default())
            .expect("both memberships route; should plan");
        assert!(
            !p.steps.iter().any(|s| matches!(&s.kind, StepKind::Convert(_))),
            "should take the cheaper DIRECT facet route, not the csv→json converter route: {:?}",
            p.steps.iter().map(|s| &s.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn plan_request_over_with_one_membership_equals_the_base_planner() {
        let reg = j!({});
        let verb = j!({ "name": "v", "accepts": ["text/plain"], "emits": "text/plain" });
        let target = Target { accept: strs(&["text/plain"]), env_caps: vec![] };
        let over = plan_request_over(&["text/plain"], &verb, &target, &reg, &[], None, Hops::default());
        let base = plan_request_using_bounded("text/plain", &verb, &target, &reg, &[], None, Hops::default());
        assert!(over.is_some());
        assert_eq!(over.as_ref().map(|p| p.cost), base.as_ref().map(|p| p.cost));
    }

    #[test]
    fn plan_request_over_empty_memberships_is_none() {
        let reg = j!({});
        let verb = j!({ "name": "v", "accepts": ["text/plain"], "emits": "text/plain" });
        let target = Target { accept: strs(&["text/plain"]), env_caps: vec![] };
        assert!(plan_request_over(&[], &verb, &target, &reg, &[], None, Hops::default()).is_none());
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
            let got = plan_request(sc["subject"].as_str().unwrap(), verb, &target, &reg, &[])
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
        let tty = plan_request("image/png", &view, &target_from_env(true, false), &reg, &[]).unwrap();
        assert_eq!(tty.delivered, "text/x-ansi");
        assert_eq!(tty.steps[1].kind, StepKind::Convert("chafa".into()));

        // bare desktop → eog → surface (the lattice resolves wayland.surface ⊑ goo.surface)
        let desktop = plan_request("image/png", &view, &target_from_env(false, true), &reg, &[]).unwrap();
        assert_eq!(desktop.delivered, "application/vnd.wayland.surface");
        assert_eq!(desktop.steps[1].kind, StepKind::Convert("eog".into()));

        // cosmic-terminal (pty+display) → ansi preferred over the cheaper surface
        let ct = plan_request("image/png", &view, &target_from_env(true, true), &reg, &[]).unwrap();
        assert_eq!(ct.delivered, "text/x-ansi");
    }

    // #2: a channel declaring a `tool` is pruned when the tool isn't available —
    // routing around it, or to a 415 when it's the only path.
    #[test]
    fn plan_request_prunes_channels_with_missing_tools() {
        let reg = j!({ "channels": [
            { "name": "needs-mlr", "accepts": ["text/csv"], "emits": "application/json",
              "cost": "cheap", "cmd": "mlr …", "tool": "mlr" }
        ]});
        let verb = j!({ "name": "keys", "accepts": ["application/json"], "emits": "text/plain" });
        let target = target_from_env(true, false); // accepts text/plain
        // mlr present → csv →[needs-mlr]→ json →(keys)→ text/plain
        assert!(plan_request("text/csv", &verb, &target, &reg, &["mlr".into()]).is_some());
        // mlr absent → the only csv→json channel is pruned → no route (415).
        assert!(plan_request("text/csv", &verb, &target, &reg, &[]).is_none());
    }

    // #1: --using pins the verb's usage channel, overriding the planner's pick.
    #[test]
    fn plan_request_using_pins_the_channel() {
        let reg = j!({ "channels": [
            { "name": "a", "accepts": ["text/*"], "emits": "text/x-a", "cost": "cheap", "cmd": "x" },
            { "name": "b", "accepts": ["text/*"], "emits": "text/x-b", "cost": "normal", "cmd": "y" },
        ]});
        let verb = j!({ "name": "say", "accepts": ["text/*"], "usage": ["a", "b"] });
        let target = Target { accept: owned(&["*/*"]), env_caps: vec![] };
        // unpinned → cheapest channel (a)
        let p = plan_request_using("text/plain", &verb, &target, &reg, &[], None).unwrap();
        assert_eq!(p.steps[0].kind, StepKind::Verb("a".into()));
        // pin b → b wins despite being costlier (constraint, not a hint)
        let pb = plan_request_using("text/plain", &verb, &target, &reg, &[], Some("b")).unwrap();
        assert_eq!(pb.steps[0].kind, StepKind::Verb("b".into()));
        // pin a channel not in `usage` → no edge → no route (the CLI pre-validates
        // for a friendlier message; the planner just yields None).
        assert!(plan_request_using("text/plain", &verb, &target, &reg, &[], Some("z")).is_none());
    }

    // Earned-hops (§4.1): the default (1,1) bounds input coercion to one hop, so a
    // 2-hop chain (csv→tsv→json) is unreachable; raising layer A — or `--force`
    // (unbounded) — restores it. The verb edge itself is never a hop.
    #[test]
    fn earned_hops_bounds_input_coercion() {
        let reg = j!({ "channels": [
            { "name": "csv2tsv", "accepts": ["text/csv"], "emits": "text/tab-separated-values", "cost": "cheap", "cmd": "x" },
            { "name": "tsv2json", "accepts": ["text/tab-separated-values"], "emits": "application/json", "cost": "cheap", "cmd": "y" },
        ]});
        let verb = j!({ "name": "keys", "accepts": ["application/json"], "emits": "text/plain" });
        let target = Target { accept: owned(&["*/*"]), env_caps: vec![] };

        // default (1,1): csv→tsv→json is 2 layer-A hops → no route.
        assert!(plan_request_using_bounded("text/csv", &verb, &target, &reg, &[], None, Hops::default()).is_none());
        // the default-delegating entry agrees (proves the flip is wired through).
        assert!(plan_request("text/csv", &verb, &target, &reg, &[]).is_none());
        // raise layer A to 2 → the chain unlocks (csv2tsv + tsv2json + verb).
        let p = plan_request_using_bounded("text/csv", &verb, &target, &reg, &[], None, Hops::default().with_layer_a(2)).unwrap();
        assert_eq!(p.steps.len(), 3);
        // --force (unbounded) reaches it too.
        assert!(plan_request_using_bounded("text/csv", &verb, &target, &reg, &[], None, Hops::unbounded()).is_some());

        // Regression: a *single* input hop is still allowed by the default (1 ≤ 1).
        let one = j!({ "channels": [
            { "name": "csv2json", "accepts": ["text/csv"], "emits": "application/json", "cost": "cheap", "cmd": "z" }
        ]});
        assert!(plan_request("text/csv", &verb, &target, &one, &[]).is_some());
    }

    // Route enumeration (§4.2): all the ways csv→…→json, cost-ranked. A direct
    // converter and a two-hop chain both reach json; enumerate returns both, the
    // cheaper (direct) first, and the hop bound prunes the long one.
    #[test]
    fn enumerate_lists_ranked_routes_and_bounds_depth() {
        let reg = j!({});
        let verbs = [verb("jq", &["application/json"], Some("application/json"), Tier::Normal)];
        let convs = [
            conv("csv2json", &["text/csv"], "application/json", Tier::Cheap), // direct (1 hop)
            conv("csv2tsv", &["text/csv"], "text/tab-separated-values", Tier::Cheap),
            conv("tsv2json", &["text/tab-separated-values"], "application/json", Tier::Cheap), // 2-hop
        ];
        let accept = strs(&["application/json"]);

        // depth 3, k=10: both routes appear, cheapest (direct) first.
        let routes = enumerate("text/csv", &verbs, &convs, &accept, &[], &reg, 3, 10);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].steps[0].kind, StepKind::Convert("csv2json".into())); // 1-hop wins
        assert!(routes[0].cost <= routes[1].cost); // cost-ranked
        assert!(routes[1].steps.iter().any(|s| s.kind == StepKind::Convert("csv2tsv".into())));

        // max_hops=1 prunes the 2-hop chain, leaving only the direct route.
        let shallow = enumerate("text/csv", &verbs, &convs, &accept, &[], &reg, 1, 10);
        assert_eq!(shallow.len(), 1);
        assert_eq!(shallow[0].steps[0].kind, StepKind::Convert("csv2json".into()));
    }
}
