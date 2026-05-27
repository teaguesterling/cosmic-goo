//! Dump the negotiation simulator's golden plans.
//!
//! Reads `simulator/demo-registry.json` + `simulator/scenarios.json`, runs
//! `plan_request` over every scenario, and writes `simulator/demo-plans.json`
//! (`{id → plan|null}`). The conformance contract for `simulator/goo-simulator.html`: its JS
//! planner must reproduce these exactly (it self-checks on load). The Rust test
//! `negotiation::demo_plans_match_fixtures` keeps the file in sync with the
//! engine — regenerate with:
//!
//!     cargo run --example dump_plans -p goo-engine

use goo_engine::negotiation::{plan_request, target_from_env, Target};
use serde_json::Value;
use std::path::PathBuf;

fn site_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../simulator")
}

/// Compute the `{id → plan|null}` map for every scenario. Shared with the
/// conformance test so both run the exact same path.
pub fn compute(registry: &Value, scenarios: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for sc in scenarios["scenarios"].as_array().unwrap() {
        let id = sc["id"].as_str().unwrap();
        let plan = plan_for(registry, sc).map(|p| p.to_json()).unwrap_or(Value::Null);
        out.insert(id.to_string(), plan);
    }
    Value::Object(out)
}

fn plan_for(reg: &Value, sc: &Value) -> Option<goo_engine::negotiation::Plan> {
    let subject = sc["subject"].as_str()?;
    let verb_name = sc["verb"].as_str()?;
    let verb = reg["verbs"].as_array()?.iter().find(|v| v["name"].as_str() == Some(verb_name))?;
    let mut target: Target =
        target_from_env(sc["env"]["tty"].as_bool().unwrap_or(false), sc["env"]["display"].as_bool().unwrap_or(false));
    if let Some(as_type) = sc.get("as").and_then(Value::as_str) {
        target = target.with_accept(as_type);
    }
    plan_request(subject, verb, &target, reg)
}

fn main() {
    let dir = site_dir();
    let reg: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("demo-registry.json")).unwrap()).unwrap();
    let scenarios: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("scenarios.json")).unwrap()).unwrap();
    let plans = compute(&reg, &scenarios);
    let pretty = serde_json::to_string_pretty(&plans).unwrap();
    std::fs::write(dir.join("demo-plans.json"), format!("{pretty}\n")).unwrap();
    println!("wrote {}", dir.join("demo-plans.json").display());
}
