//! `goo` — the cosmic-goo CLI. A thin orchestration layer over `goo-engine`,
//! ported from `bin/goo` (which stays canonical until this passes the bats
//! conformance suite). Subcommands and verb invocation assemble a subject /
//! object / adverbs, hand them to the engine to render a command, and exec it
//! via `bash -c` — exactly as the shell does.
//!
//! Plugins are found via env only (`COSMIC_GOO_BUILTIN_PLUGINS_DIR` and the
//! XDG dirs `registry::dirs()` reads); no path magic. Exit codes: 0 / 1
//! (catch-all) / 130 (cancel).

use goo_engine::{address, dispatch as disp, exec, inference, mime, negotiation, options, registry, selection, verbs};
use serde_json::{json, Map, Value};
use std::io::IsTerminal;

fn main() {
    reset_sigpipe();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // Global `-c <path>` / `--config <path>` (repeatable): extra plugin files/dirs
    // merged LAST (highest precedence). Threaded to registry::load_all via env so
    // every entry point (verb run, --explain, list, …) sees the same config.
    let (args, configs) = extract_config_flags(raw);
    if !configs.is_empty() {
        std::env::set_var("COSMIC_GOO_EXTRA_CONFIG", configs.join(":"));
    }
    std::process::exit(dispatch(&args, 0));
}

/// Pull global `-c`/`--config` out of the arg list, returning `(remaining, configs)`.
fn extract_config_flags(args: Vec<String>) -> (Vec<String>, Vec<String>) {
    let (mut out, mut configs) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "-c" || a == "--config" {
            if let Some(p) = args.get(i + 1) {
                configs.push(p.clone());
                i += 2;
                continue;
            }
            i += 1;
        } else if let Some(p) = a.strip_prefix("--config=").or_else(|| a.strip_prefix("-c=")) {
            configs.push(p.to_string());
            i += 1;
        } else {
            out.push(a.clone());
            i += 1;
        }
    }
    (out, configs)
}

/// Restore the default SIGPIPE disposition. Rust ignores SIGPIPE at startup, so
/// writing to a closed pipe (e.g. `goo plugins | head`) returns EPIPE and
/// `println!` panics — unlike bash and every other Unix tool, which die quietly
/// on the signal. Resetting to SIG_DFL restores that parity. (Linux/Unix ABI:
/// SIGPIPE = 13, SIG_DFL = 0.)
#[cfg(unix)]
fn reset_sigpipe() {
    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    unsafe {
        signal(13, 0);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

/// `goo: <msg>` to stderr; returns exit code 1.
fn die(msg: impl AsRef<str>) -> i32 {
    eprintln!("goo: {}", msg.as_ref());
    1
}

// ---------------- top-level dispatch ----------------

fn dispatch(args: &[String], alias_depth: u32) -> i32 {
    match args.first().map(String::as_str) {
        None => {
            // A true CLI: no args prints usage rather than launching anything.
            print_usage();
            0
        }
        Some("compose") => cmd_compose(),
        Some("list") => cmd_list(args.get(1).map(String::as_str)),
        Some("describe") => cmd_describe(args.get(1).map(String::as_str)),
        Some("plugins") => cmd_plugins(),
        Some("validate") => cmd_validate(),
        Some("--explain") => cmd_explain(&args[1..]),
        Some("options") => cmd_options(&args[1..]),
        // `goo what <addr>` — applicable-verbs listing for a subject. Same OPTIONS
        // projection `cmd_goo`'s error path consumes (SSOT). See completion-polish.md.
        Some("what") => match args.get(1) {
            Some(addr) if !addr.is_empty() => cmd_what(&registry::load_all(), addr),
            _ => die("what: usage: goo what <subject>"),
        },
        Some("dispatch") => cmd_dispatch(args.get(1).map(String::as_str)),
        Some("reload") => cmd_reload(),
        Some("__complete") => cmd_complete(&args[1..]),
        Some("-h") | Some("--help") | Some("help") => {
            print_usage();
            0
        }
        Some(first) => {
            // A command alias rewrites the leading word into its `expands`
            // tokens, then we re-dispatch (the expansion may itself be a
            // subcommand or another alias). A depth guard breaks cycles.
            let reg = registry::load_all();
            if let Some(exp) = alias_expansion(&reg, first) {
                let depth = alias_depth + 1;
                if depth > 16 {
                    return die(format!("alias expansion too deep (cycle?): {first}"));
                }
                // TODO: shell-quote-aware tokenization; whitespace-split covers
                // all current alias fixtures.
                let mut new_args: Vec<String> = exp.split_whitespace().map(str::to_string).collect();
                new_args.extend_from_slice(&args[1..]);
                return dispatch(&new_args, depth);
            }
            // `GOO` (the doc's default verb): a leading explicit address — a
            // goo:// URL, a sigil/native shape — with no verb means "resolve
            // this subject and run its type's default_for verb". A bare word
            // that isn't an address stays a verb lookup (→ "unknown verb").
            // Rust-only extension beyond the bash reference; see goo-protocol.md.
            if address::is_explicit(first, &reg) {
                return cmd_goo(&reg, first, &args[1..]);
            }
            // Entity-name inference (slice #7, data-entry-ux.md §3.2). Fires
            // only when `first` is an inferable SHAPE and NOT a known verb —
            // the noun-first dispatch path. Per §3.2.3 the (band, context) →
            // action mapping happens here in the bin layer; the engine returns
            // the same band regardless of context.
            if inference::is_inferable_shape(first) && verbs::lookup(&reg, first, None).is_none() {
                let ctx = detect_inference_context();
                if let Ok((subject, band, reason)) = inference::infer_entity(first, &reg, ctx) {
                    match act_on_inference(&reg, first, &subject, band, &reason, ctx, &args[1..]) {
                        InferAction::Resolved(code) => return code,
                        InferAction::FallThrough => {}
                    }
                }
            }
            cmd_verb(&reg, args)
        }
    }
}

/// What the dispatch layer did with an inference result. `Resolved` means we
/// either ran the verb or printed a handled message (caller returns the code);
/// `FallThrough` means inference declined to act (Low band, or HIGH-in-script
/// without an override — fall to today's "unknown verb" path).
enum InferAction {
    Resolved(i32),
    FallThrough,
}

/// Outcome of resolving a verb's subject. `Value` is the resolved subject;
/// `Exit` means a message was already printed (the MEDIUM-band inference
/// picker) and `cmd_verb` should return that code instead of running the verb.
enum Subject {
    Value(Value),
    Exit(i32),
}

/// What verb-aware inference (slice 8 / §3.4) decided for a bare subject token.
enum VerbInfer {
    /// Resolve the verb's subject to this value.
    Use(Value),
    /// A picker was printed — `resolve_subject` should signal `Subject::Exit(2)`.
    Picker,
    /// Inference declined (LOW, HIGH-in-script, or no match) — continue the
    /// resolution chain (text content, then the `handle_search` fallback).
    FallThrough,
}

/// Map (band, context) → action per §3.2.3 + §3.5.
///
/// **Safety property** (§3.6): only DEFINITIVE fires silently in script
/// context. Everything fuzzier degrades to a nudge log or falls through, so
/// pipes / CI / cron cannot be surprised by a fuzzy match.
fn act_on_inference(
    reg: &Value,
    raw: &str,
    subject: &Value,
    band: inference::Band,
    reason: &inference::Reason,
    ctx: inference::Context,
    rest: &[String],
) -> InferAction {
    use inference::{Band, Context};
    let interactive = matches!(ctx, Context::Interactive | Context::Gui);
    let nudge_suppressed = std::env::var("GOO_INFER_NO_NUDGE").is_ok();

    match band {
        Band::Definitive => {
            // Safe in all contexts (exact match, unique). Resolve silently.
            InferAction::Resolved(run_inferred(reg, subject, rest))
        }
        Band::High if interactive => {
            // Resolve + one-line nudge so the user learns the explicit form.
            if !nudge_suppressed {
                emit_nudge(raw, subject, reason);
            }
            InferAction::Resolved(run_inferred(reg, subject, rest))
        }
        Band::High => {
            // Script context (no override): emit "would have resolved" log to
            // stderr and fall through. Caller will hit the "unknown verb"
            // path — that's the safe, explicit failure mode.
            if !nudge_suppressed {
                emit_script_nudge(raw, subject, reason);
            }
            InferAction::FallThrough
        }
        Band::Medium if interactive => {
            // Numbered candidate list — the user-visible part of the
            // §3.5 picker. Real interactive picker (read a number, dispatch
            // accordingly) is post-v1; for now, the user re-runs with the
            // explicit form they see in the list.
            if !nudge_suppressed {
                emit_medium_picker(raw, reason);
            }
            InferAction::Resolved(2)
        }
        Band::Medium | Band::Low => InferAction::FallThrough,
    }
}

/// One-line stderr nudge for HIGH-band interactive resolutions (§3.5).
fn emit_nudge(raw: &str, subject: &Value, reason: &inference::Reason) {
    let id = subject.get("id").and_then(Value::as_str).unwrap_or("");
    eprintln!(
        "goo: inferred '{raw}' → :{}/{id}  (band: HIGH; use the explicit form to suppress)",
        reason.winner_source
    );
}

/// Script-context nudge-then-fallback log for HIGH band (§3.5).
fn emit_script_nudge(raw: &str, subject: &Value, reason: &inference::Reason) {
    let id = subject.get("id").and_then(Value::as_str).unwrap_or("");
    eprintln!(
        "goo: would have inferred '{raw}' → :{}/{id} (HIGH band) — not auto-resolving",
        reason.winner_source
    );
    eprintln!("     in script context. Use the explicit form, or GOO_INFER_STRICTNESS=tty.");
}

/// MEDIUM-band picker render (§3.5 inline picker format). Numbered list of
/// the top alternatives; user re-runs with the explicit address they see.
/// Real interactive number-pick is post-v1 (would need stdin handling +
/// a stdout-vs-stdin separation we don't have today).
fn emit_medium_picker(raw: &str, reason: &inference::Reason) {
    eprintln!("goo: '{raw}' is ambiguous — pick one:");
    for (i, (src, id, label)) in reason.alternatives.iter().enumerate() {
        // 1-indexed for human readability; pad address column for alignment.
        let addr = format!(":{src}/{id}");
        eprintln!("  {n}) {addr:<32}  {label}", n = i + 1, addr = addr, label = label);
    }
    let extra = reason.candidate_count.saturating_sub(reason.alternatives.len());
    if extra > 0 {
        eprintln!("  … {extra} more (top {} of {})", reason.alternatives.len(), reason.candidate_count);
    }
    eprintln!("re-run with the explicit address; suppress this list with GOO_INFER_NO_NUDGE=1.");
}

/// Hand the inferred subject to GOO default-verb dispatch by rebuilding it as
/// the canonical `:<prefix>/<id>` form and calling `cmd_goo`. That path
/// re-resolves through `address::resolve` — small extra cost; cache layer
/// (slice 7b) will eliminate the redundant source enumeration.
fn run_inferred(reg: &Value, subject: &Value, rest: &[String]) -> i32 {
    let prefix = subject.get("type").and_then(Value::as_str).unwrap_or("");
    let _ = prefix; // type isn't the addressing prefix; reconstruct from reason instead
    // We have the subject already; build a canonical goo:// URI by looking up
    // the source whose `emits` matches the subject's type. Avoids encoding the
    // prefix in the subject and matches cmd_goo's existing entry point.
    let subj_type = subject.get("type").and_then(Value::as_str).unwrap_or("");
    let id = subject.get("id").and_then(Value::as_str).unwrap_or("");
    let prefix = reg
        .get("sources")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter().find_map(|s| {
                if s.get("emits").and_then(Value::as_str) == Some(subj_type) {
                    s.get("prefix").and_then(Value::as_str).or_else(|| s.get("name").and_then(Value::as_str))
                } else {
                    None
                }
            })
        })
        .unwrap_or("");
    if prefix.is_empty() || id.is_empty() {
        return die(format!("inference: cannot rebuild address for type '{subj_type}'"));
    }
    let addr = format!(":{prefix}/{id}");
    cmd_goo(reg, &addr, rest)
}

/// Detect the inference Context for this invocation (§3.2.3): env override
/// first (`GOO_INFER_STRICTNESS=script|tty|gui`), then `isatty(stdout)`. CLI
/// flag (`--infer-strictness`) is deferred — env covers automation cleanly.
fn detect_inference_context() -> inference::Context {
    if let Ok(v) = std::env::var("GOO_INFER_STRICTNESS") {
        return match v.as_str() {
            "script" => inference::Context::Script,
            "tty" | "interactive" => inference::Context::Interactive,
            "gui" => inference::Context::Gui,
            _ => detect_inference_context_from_isatty(), // unknown value → fall back
        };
    }
    detect_inference_context_from_isatty()
}

fn detect_inference_context_from_isatty() -> inference::Context {
    if std::io::stdout().is_terminal() {
        inference::Context::Interactive
    } else {
        inference::Context::Script
    }
}

/// The `GOO` default verb: resolve `addr` to a subject, look up its type's
/// `default_for` verb, and run it (with any trailing `--adverbs` / object).
/// No applicable default → a helpful error listing the top-5 applicable verbs
/// (the CLI analog of the protocol's 415/300). The top-5 are pulled from
/// `options::options_for(reg, &subject).allow`, the SAME projection
/// `goo what` and `goo options` consume — divergence between surfaces would
/// be a projection bug, not a UI preference. See completion-polish.md §6
/// slice 3 + Gate 4 (the triple-equality test that locks this property).
fn cmd_goo(reg: &Value, addr: &str, rest: &[String]) -> i32 {
    let subject = match address::resolve(addr, reg, None) {
        Ok(s) => s,
        Err(e) => return die(e.replace("address: ", "")),
    };
    let type_ = subject.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain");
    let verb = match verbs::default_for(reg, type_) {
        Some(v) => v,
        None => {
            let mut msg = format!("no default verb for type '{type_}'");
            let (listing, total) = applicable_verbs_listing(reg, &subject, Some(5));
            if !listing.is_empty() {
                // Header is count-aware: show "top 5" only when truncation actually
                // hides additional verbs. With ≤5 applicable, "top 5 applicable
                // verbs" alongside 1-2 entries reads as misleading.
                let header = if total > 5 {
                    "  top 5 applicable verbs:".to_string()
                } else {
                    "  applicable verbs:".to_string()
                };
                msg.push_str(&format!("\n{header}\n"));
                msg.push_str(&listing);
                // Only point to `goo what` when there's actually more to see.
                if total > 5 {
                    msg.push_str(&format!("\n  full list:  goo what {addr}"));
                }
            }
            return die(msg);
        }
    };
    let (positionals, adverbs) = parse_args(rest);
    let object_arg = positionals.first().cloned().unwrap_or_default();
    let has_object_type = verb.get("object_type").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).is_some();
    let object = if !object_arg.is_empty() || has_object_type {
        match verbs::resolve_object(reg, &verb, &object_arg, &subject) {
            Ok(o) => o,
            Err(e) => return die(e),
        }
    } else {
        Value::Null
    };
    exec_verb(reg, &verb, &subject, &object, &adverbs)
}

/// `goo what <addr>` — informational: print the applicable-verbs list for a
/// subject, with chips and descriptions. The same projection (`options::options_for`)
/// the dispatch-error path consumes — divergence is a bug. Single-source-of-truth
/// surface for "what can I do with this thing" in the CLI; per `OPTIONS.allow`
/// `goo reload` — drop the entity-list cache so the next inference re-reads
/// every source fresh. The watch-validated cache never serves stale data on its
/// own (it invalidates on a source's watch-file mtime); this is the manual
/// override for the cases watch can't see yet (a source whose data changed with
/// no file to watch, before the `good` daemon's dbus/inotify hooks exist).
fn cmd_reload() -> i32 {
    let n = inference::clear_entity_cache();
    eprintln!("goo: reload: cleared {n} cached source list(s)");
    0
}

/// ordering. See completion-polish.md §6 slice 3.
fn cmd_what(reg: &Value, addr: &str) -> i32 {
    let subject = match address::resolve(addr, reg, None) {
        Ok(s) => s,
        Err(e) => return die(e.replace("address: ", "")),
    };
    let type_ = subject.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let (listing, _total) = applicable_verbs_listing(reg, &subject, None);
    if listing.is_empty() {
        println!("no applicable verbs for {addr}  (type: {type_})");
        return 0;
    }
    println!("applicable verbs for {addr}  (type: {type_})");
    println!("{listing}");
    0
}

/// Build the printable per-verb listing for a subject: chips + name + description,
/// in `OPTIONS.allow` order, optionally truncated to a max count. Reused by
/// `cmd_what` and `cmd_goo`'s "no default verb" error path so the two surfaces
/// can NEVER drift in ordering — the SSOT property Gate 4 verifies with the
/// triple-equality test (slice 3 error top-5 == `goo what` first-5 ==
/// `OPTIONS.allow` first-5). See completion-polish.md §6 slice 3.
///
/// Returns `(listing_string, total_applicable)` so callers can adapt the header
/// to whether truncation actually hid anything ("top 5 of 12" vs "applicable
/// verbs"). The listing reflects `max`; the total reflects the full count.
///
/// Format mirrors the chip vocabulary in completion-polish.md §2:
/// `    <name>  <chips>    <description>` (4-space indent, chips after name,
/// chips suffix in `goo describe` style so future zsh/fish ports can match
/// glyphs verbatim).
fn applicable_verbs_listing(reg: &Value, subject: &Value, max: Option<usize>) -> (String, usize) {
    let view = options::options_for(reg, subject);
    let names: Vec<&str> = view
        .get("allow")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let total = names.len();
    let verbs_map = view.get("verbs").and_then(Value::as_object);
    // Description isn't projected through OPTIONS (it's a verb internal — kept
    // out by the projection-leak test). Look it up from the registry instead.
    // For polymorphic verbs we pick the first contributor's description as a
    // representative — `goo describe <name>` is the right surface for
    // per-impl detail.
    let registry_verbs = reg.get("verbs").and_then(Value::as_array);
    let description_for = |name: &str| -> Option<String> {
        registry_verbs?
            .iter()
            .find(|v| v.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|v| v.get("description").and_then(Value::as_str))
            .map(String::from)
    };

    let mut lines: Vec<String> = Vec::new();
    let iter: Box<dyn Iterator<Item = &&str>> = match max {
        Some(n) => Box::new(names.iter().take(n)),
        None => Box::new(names.iter()),
    };
    for name in iter {
        let v = verbs_map.and_then(|m| m.get(*name));
        let confirm = v.and_then(|v| v.get("confirm")).and_then(Value::as_bool).unwrap_or(false);
        let destructive = v.and_then(|v| v.get("destructive")).and_then(Value::as_bool).unwrap_or(false);
        // Chip rule matches `cmd_describe`: destructive subsumes confirm.
        let chip = if destructive {
            "  [!!]"
        } else if confirm {
            "  [!]"
        } else {
            ""
        };
        let desc = description_for(name).unwrap_or_default();
        let sep = if desc.is_empty() { "" } else { "    " };
        lines.push(format!("    {name}{chip}{sep}{desc}"));
    }
    (lines.join("\n"), total)
}

/// Split a verb/GOO argument tail into positionals and an adverbs object:
/// `--flag=val` / `--flag val` / `--flag` (=true) / bare positionals.
fn parse_args(rest: &[String]) -> (Vec<String>, Value) {
    let mut positionals: Vec<String> = Vec::new();
    let mut adverbs = Map::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        // `-o FILE` / `-o=FILE` — sugar for `--to <file>` (route the result to a file).
        if a == "-o" {
            if let Some(v) = rest.get(i + 1) {
                adverbs.insert("to".into(), json!(format!("goo://file/{v}")));
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if let Some(v) = a.strip_prefix("-o=") {
            adverbs.insert("to".into(), json!(format!("goo://file/{v}")));
            i += 1;
            continue;
        }
        if let Some(kv) = a.strip_prefix("--") {
            if let Some((name, value)) = kv.split_once('=') {
                adverbs.insert(name.to_string(), json!(value));
                i += 1;
            } else {
                match rest.get(i + 1) {
                    Some(v) if !v.starts_with("--") => {
                        adverbs.insert(kv.to_string(), json!(v));
                        i += 2;
                    }
                    _ => {
                        adverbs.insert(kv.to_string(), json!(true));
                        i += 1;
                    }
                }
            }
        } else {
            positionals.push(a.clone());
            i += 1;
        }
    }
    (positionals, Value::Object(adverbs))
}

/// Derive the earned-hops budget (§4.1) from the run's adverbs: `--force` lifts
/// the bound entirely; `--hops N` raises input-coercion depth (layer A); else the
/// tight default `(1, 1)`. `--hops`/`--force` ride in the adverb map like
/// `--as`/`--to`/`--using` — no verb template references them, so they're inert as
/// substitutions and read here only as planner controls.
fn hops_from_adverbs(adverbs: &Value) -> negotiation::Hops {
    if adverbs.get("force").and_then(|v| v.as_bool()).unwrap_or(false) {
        return negotiation::Hops::unbounded();
    }
    match adverbs.get("hops").and_then(|v| v.as_str()).and_then(|s| s.parse::<u8>().ok()) {
        Some(n) => negotiation::Hops::default().with_layer_a(n),
        None => negotiation::Hops::default(),
    }
}

fn print_usage() {
    print!(
        "goo — Grammar Of Operations CLI

USAGE
    goo <verb> [POSITIONAL...] [--FLAG=VALUE]
    goo <address>                        Resolve the address, run its type's default verb (GOO)
    goo list <source>                    Emit source items as JSON
    goo describe <verb>                  Show verb details
    goo dispatch <input>                 Classify content and route to a verb
    goo options <subject|=TYPE>          Applicable verbs + their slots, as JSON (discovery; unstable v1)
    goo compose                          Build a sentence (scripted via GOO_COMPOSE_ANSWERS)
    goo what <subject|=TYPE>             List the verbs applicable to a subject (with chips)
    goo plugins                          List loaded plugins
    goo validate                         Validate all loaded plugins
    goo reload                           Drop the entity-list cache (force a fresh re-read)
    goo <verb> … [--using CHANNEL]       --using pins the channel that performs a verb
    goo <verb> … [--to DEST | -o FILE]   route the result to a file / clipboard (^) instead of stdout
    goo <verb> … [--hops N | --force]    allow deeper auto-coercion (default: 1 hop in, 1 out)
    goo <verb> … [--confirm-dangerous=V] pre-approve the y/N gate for the named confirm/destructive verb(s)
    goo --explain <verb> [=TYPE|subj]    Show the negotiation plan (route/415) — read-only
                                         [--as TYPE] [--using CHANNEL] [--explain-env tty|cosmic|desktop|piped]
                                         [--explain-with route|steps|shell] (default: adaptive)
                                         [--paths [--max-hops C] [--format text|mermaid]]  all routes A→B

GLOBAL
    -c, --config <file|dir>              merge an extra plugin config (repeatable; highest precedence)

SUBJECT INFERENCE
    If no positional is given, the subject falls back in order:
      1. PRIMARY selection (wl-paste --primary)
      2. Clipboard (wl-paste)
      3. Focused app (cos-cli) when the verb accepts an app type

EXAMPLES
    goo critique \"some text to review\"
    goo critique --via=clipboard          # operate on the current selection
    goo plugins
    goo describe critique
"
    );
}

// ---------------- helpers ----------------

use goo_engine::shell::{bash_capture, bash_capture_bytes, bash_exec};

/// `goo --explain VERB [SUBJECT|=TYPE] [--as TYPE] [--explain-env ENV]
/// [--explain-with MODE]` — the negotiation plan explainer (goo-debug). Read-only:
/// shows the Accept profile and the planned route (or a 415), never runs anything.
/// `=<mime>` asserts the subject type virtually (no file needed), via the built-in
/// `=` sigil shipped in `core.toml`; `--explain-env
/// tty|cosmic|desktop|piped` overrides the detected environment (default: isatty +
/// $WAYLAND_DISPLAY). The route line is richly rendered on a TTY (cost by color;
/// lossy/network edges marked). `--explain-with route|steps|shell` picks the detail
/// view (default: adaptive — `shell` commands for a ≤2-hop route, annotated `steps`
/// beyond). `--paths [--max-hops C] [--format text|mermaid]` enumerates *all* routes
/// A→B (the route-graph debugger) instead of the single chosen plan.
fn cmd_explain(args: &[String]) -> i32 {
    let reg = registry::load_all();
    let (mut verb_name, mut subj, mut as_type, mut env_ovr, mut using): (Option<&str>, Option<&str>, Option<&str>, Option<&str>, Option<&str>) =
        (None, None, None, None, None);
    let (mut hops_flag, mut force): (Option<&str>, bool) = (None, false);
    let mut explain_with: Option<&str> = None;
    let (mut paths, mut max_hops_flag, mut format_flag): (bool, Option<&str>, Option<&str>) = (false, None, None);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if let Some(v) = a.strip_prefix("--as=") {
            as_type = Some(v);
        } else if a == "--as" {
            as_type = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(v) = a.strip_prefix("--using=") {
            using = Some(v);
        } else if a == "--using" {
            using = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(v) = a.strip_prefix("--explain-with=") {
            explain_with = Some(v);
        } else if a == "--explain-with" {
            explain_with = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if a == "--paths" {
            paths = true;
        } else if let Some(v) = a.strip_prefix("--max-hops=") {
            max_hops_flag = Some(v);
        } else if a == "--max-hops" {
            max_hops_flag = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(v) = a.strip_prefix("--format=") {
            format_flag = Some(v);
        } else if a == "--format" {
            format_flag = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(v) = a.strip_prefix("--hops=") {
            hops_flag = Some(v);
        } else if a == "--hops" {
            hops_flag = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if a == "--force" {
            force = true;
        } else if let Some(v) = a.strip_prefix("--explain-env=") {
            env_ovr = Some(v);
        } else if a == "--explain-env" {
            env_ovr = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if verb_name.is_none() {
            verb_name = Some(a);
        } else if subj.is_none() {
            subj = Some(a);
        }
        i += 1;
    }

    let verb_name = match verb_name {
        Some(v) => v,
        None => return die("explain: usage: goo --explain VERB [=TYPE|subject] [--as TYPE] [--explain-env tty|cosmic|desktop|piped]"),
    };
    let verb = match reg["verbs"].as_array().and_then(|a| a.iter().find(|v| v["name"].as_str() == Some(verb_name))) {
        Some(v) => v.clone(),
        None => return die(format!("explain: unknown verb '{verb_name}'")),
    };

    // Type the subject the SAME WAY the run does — via the detection signals — and
    // record which fired, so the annotation is truthful. The narrow exception:
    // when the subject is a *type-domain* address (`=<mime>` / `:type/<mime>` /
    // `goo://type/<mime>`), the user has *explicitly asserted* the type — route
    // through `address::resolve` and label `explicit`. Every other shape (file
    // paths, URLs, sigil-+/^, bare text) keeps its provenance: extension, libmagic,
    // checker, content.
    let (subject_type, type_source): (String, &str) = if let Some(s) = subj {
        if address::canonicalize(s, &reg).starts_with("goo://type/") {
            match address::resolve(s, &reg, None) {
                Ok(subject) => (
                    subject.get("type").and_then(Value::as_str).unwrap_or("application/octet-stream").to_string(),
                    "explicit",
                ),
                Err(e) => return die(e.replace("address: ", "")),
            }
        } else if std::path::Path::new(s).exists() {
            address::type_for_path(s, &reg)
                .unwrap_or_else(|_| ("application/octet-stream".into(), "libmagic"))
        } else if let Some((t, src)) = mime::infer_for_with_source(s, &verb, &reg) {
            (t, src)
        } else {
            (mime::detect_content(s), "content")
        }
    } else {
        return die("explain: needs a subject — e.g. `=image/png` or a file path");
    };

    let (tty, display) = match env_ovr {
        Some("tty") => (true, false),
        Some("cosmic") | Some("cosmic-term") => (true, true),
        Some("desktop") => (false, true),
        Some("piped") => (false, false),
        Some(other) => return die(format!("explain: unknown --explain-env '{other}' (tty|cosmic|desktop|piped)")),
        None => {
            use std::io::IsTerminal;
            let disp = std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
                || std::env::var("DISPLAY").is_ok_and(|v| !v.is_empty());
            (std::io::stdout().is_terminal(), disp)
        }
    };

    let mut target = negotiation::target_from_env(tty, display);
    if let Some(t) = as_type {
        target = target.with_accept(t);
    }

    let caps = if target.env_caps.is_empty() {
        String::new()
    } else {
        format!("   · caps {{{}}}", target.env_caps.join(", "))
    };
    println!("Accept: {}{}", target.accept.join("  "), caps);
    println!("subject: {subject_type} (via {type_source})");

    // --explain is tool-AGNOSTIC: it shows the planned route regardless of which
    // converter tools are installed locally (a planning/debug view, not a
    // local-reality check). Pass every declared tool so nothing is pruned;
    // execution (exec_negotiated) prunes by real availability.
    let (avail, missing) = channel_tools(&reg);
    let all_tools: Vec<String> = avail.into_iter().chain(missing).collect();

    // `--paths`: enumerate ALL routes to a satisfiable Accept (the route-graph
    // debugger), instead of the single chosen plan. `--max-hops C` bounds depth
    // (default 3); `--format text|mermaid` picks the drawing.
    if paths {
        let max_hops: u8 = max_hops_flag.and_then(|s| s.parse().ok()).unwrap_or(3);
        let routes = negotiation::enumerate_request(&subject_type, &verb, &target, &reg, &all_tools, max_hops, 12);
        if routes.is_empty() {
            println!("415 · no route within {max_hops} hop(s) — {subject_type} → {verb_name} (nothing reaches the Accept)");
            return 1;
        }
        let out = match format_flag {
            Some("mermaid") => render_paths_mermaid(&routes, verb_name, &reg),
            Some("text") | None => render_paths_text(&routes, &subject_type, verb_name, &reg, max_hops, use_color()),
            Some(other) => return die(format!("explain: unknown --format '{other}' (text|mermaid)")),
        };
        println!("{out}");
        return 0;
    }

    // Same earned-hops budget the run would use, so --explain is an honest preview.
    let hops = if force {
        negotiation::Hops::unbounded()
    } else {
        match hops_flag.and_then(|s| s.parse::<u8>().ok()) {
            Some(n) => negotiation::Hops::default().with_layer_a(n),
            None => negotiation::Hops::default(),
        }
    };
    match negotiation::plan_request_using_bounded(&subject_type, &verb, &target, &reg, &all_tools, using, hops) {
        None => {
            println!("415 · no route — {subject_type} can't be presented here (verb: {verb_name})");
            1
        }
        Some(plan) => {
            println!("{}   (cost {})", render_route(&plan, &subject_type, verb_name, &reg, use_color()), plan.cost);
            // Detail view (§ user request): `--explain-with steps|shell|route`, with
            // an adaptive default — a runnable-ish `shell` block for simple routes
            // (≤2 converter hops), annotated `steps` beyond. Understanding scales
            // with route length; a full command for a deep route is less useful.
            let conv_hops = plan.steps.iter().filter(|s| matches!(s.kind, negotiation::StepKind::Convert(_))).count();
            let subj_path = subj.filter(|s| std::path::Path::new(s).exists());
            let block = match explain_with {
                Some("route") => String::new(),
                Some("steps") => render_steps(&plan, &verb, verb_name, &reg),
                Some("shell") => render_shell(&plan, &verb, &reg, subj_path),
                Some(other) => return die(format!("explain: unknown --explain-with '{other}' (route|steps|shell)")),
                None if conv_hops <= 2 => render_shell(&plan, &verb, &reg, subj_path),
                None => render_steps(&plan, &verb, verb_name, &reg),
            };
            if !block.is_empty() {
                println!("{block}");
            }
            0
        }
    }
}

/// `goo options <subject | =TYPE>` — the OPTIONS discovery surface (goo-protocol
/// §7): the verbs applicable to the subject and, per verb, the slots a caller can
/// fill (`Using:` instruments, `With:` adverbs + their choices, `object_type`).
/// Emits JSON — the single composable surface the compose-gui's verb-pick,
/// completion, and (later) the `good` daemon all consume. Read-only. The shape is
/// **unstable through v1** (`schema_version`/`stable`). Rust-only: it exposes
/// `Using:` channels, which the bash reference has no concept of.
fn cmd_options(args: &[String]) -> i32 {
    let reg = registry::load_all();
    let Some(arg) = args.iter().find(|a| !a.starts_with('-')) else {
        return die("options: usage: goo options <subject | =TYPE>");
    };
    // Everything goes through the addressing layer — explicit forms (`=<mime>`,
    // `:type/<mime>`, `goo://…`, sigils, native shapes) and bare content alike. The
    // `=` sigil (`core.toml`) rewrites `=text/markdown` → `goo://type/text/markdown`,
    // which resolves to a virtual subject `{type: "text/markdown"}` — same shape
    // the old `@<mime>` produced, now canonical.
    let subject = match address::resolve(arg, &reg, None) {
        Ok(s) => s,
        Err(e) => return die(e.replace("address: ", "")),
    };
    let view = options::options_for(&reg, &subject);
    match serde_json::to_string_pretty(&view) {
        Ok(s) => {
            println!("{s}");
            0
        }
        Err(e) => die(format!("options: {e}")),
    }
}

fn is_present(verb: &Value) -> bool {
    verb.get("kind").and_then(|k| k.as_str()) == Some("present")
}

/// A verb implemented by `usage` channels (rather than its own `cmd`) — it must
/// always go through the negotiation engine (the planner picks a channel, the
/// executor runs it), even with no type gap, since it has no `cmd` to render.
fn has_usage(verb: &Value) -> bool {
    verb.get("usage").and_then(|u| u.as_array()).is_some_and(|a| !a.is_empty())
}

/// True when the subject's type isn't a subtype of anything the verb `accepts`
/// — a type gap that needs input coercion (4b). Verbs with no `accepts`
/// (no-subject verbs) and typeless/absent subjects never qualify.
fn needs_coercion(reg: &Value, verb: &Value, subject: &Value) -> bool {
    let accepts = match verb.get("accepts").and_then(|a| a.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => return false,
    };
    let stype = match subject.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return false,
    };
    !accepts.iter().filter_map(|p| p.as_str()).any(|p| mime::is_subtype(stype, p, reg))
}

/// Run a verb through the negotiation engine — for a `present` verb (the subject
/// is the result) or a real verb with a type gap (input coercion). Materialize
/// the subject to a file, plan its route to the environment's Accept (pinned by
/// `--as`), and execute (final step inherits stdout; the executor drives the
/// converters/renderers and runs the verb step). No route → 415.
fn exec_negotiated(reg: &Value, verb: &Value, subject: &Value, adverbs: &Value) -> i32 {
    let subject_type = subject.get("type").and_then(|t| t.as_str()).unwrap_or("application/octet-stream");
    let verb_name = verb.get("name").and_then(|n| n.as_str()).unwrap_or("");

    // The subject as a file on disk — the value the executor threads. Inline
    // content (no backing path, e.g. stdin) is materialized to a temp file.
    let subject_path: String = match subject.get("metadata").and_then(|m| m.get("path")).and_then(|p| p.as_str()) {
        Some(p) => p.to_string(),
        None => {
            let text = subject.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let tmp = std::env::temp_dir().join(format!("goo-present-{}.bin", std::process::id()));
            if std::fs::write(&tmp, text).is_err() {
                return die("present: cannot materialize the subject");
            }
            tmp.to_string_lossy().into_owned()
        }
    };

    // Real environment → Accept profile (pinned by --as if given). With --to/-o the
    // result lands at a {write} sink, which wants BYTES — so force the piped profile
    // (else `view img --to out.png` would route image→chafa→ansi into the file).
    use std::io::IsTerminal;
    let dest = adverbs.get("to").and_then(|v| v.as_str());
    let (tty, display) = if dest.is_some() {
        (false, false)
    } else {
        // A desktop surface is only a viable destination when stdout is a TTY
        // (interactive). When stdout is piped/redirected, the destination IS the
        // pipe → deliver bytes, regardless of $WAYLAND_DISPLAY — else
        // `goo yaml data.csv | less` would route to a surface and the pipe gets
        // nothing. (A GUI launcher requests a surface explicitly, not via this.)
        let tty = std::io::stdout().is_terminal();
        let disp = tty
            && (std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
                || std::env::var("DISPLAY").is_ok_and(|v| !v.is_empty()));
        (tty, disp)
    };
    let mut target = negotiation::target_from_env(tty, display);
    if let Some(as_t) = adverbs.get("as").and_then(|v| v.as_str()) {
        target = target.with_accept(as_t);
    }

    // `--using=<channel>` pins the verb's instrument (override the planner). It's
    // a constraint: validate it's actually one of the verb's `usage` channels for
    // a clean error, then force the route through it.
    let using = adverbs.get("using").and_then(|v| v.as_str());
    if let Some(u) = using {
        match verb.get("usage").and_then(|v| v.as_array()) {
            None => return die(format!("--using: '{verb_name}' isn't implemented by channels (it has no `usage`)")),
            Some(arr) if !arr.iter().any(|c| c.as_str() == Some(u)) => {
                let opts: Vec<&str> = arr.iter().filter_map(|c| c.as_str()).collect();
                return die(format!("--using '{u}': not a channel of '{verb_name}' (one of: {})", opts.join(", ")));
            }
            Some(_) => {}
        }
    }

    // Earned-hops (§4.1): default ≤1 converter hop per layer; `--hops N` raises
    // input-coercion depth, `--force` lifts the bound entirely.
    let hops = hops_from_adverbs(adverbs);

    let (available, missing) = channel_tools(reg);
    match negotiation::plan_request_using_bounded(subject_type, verb, &target, reg, &available, using, hops) {
        None => {
            // Teaching 415 first: would a deeper budget have found a route (with the
            // tools you have)? If so, show it and the flag that unlocks it.
            if let Some(msg) = deeper_route_hint(subject_type, verb, &target, reg, &available, hops) {
                return die(msg);
            }
            // Else: if a route *would* exist with everything installed, name the
            // missing tools *on that route* — actionable ("install: mlr"), not every
            // uninstalled tool.
            let hint = route_missing_tools_hint(subject_type, verb, &target, reg, &missing, hops);
            die(format!("415 · no route — can't route {subject_type} through '{verb_name}'{hint}"))
        }
        Some(plan) => match dest {
            None => match exec::execute(&plan, &subject_path, verb, reg) {
                Ok(code) => code,
                Err(e) => die(format!("{verb_name}: {e}")),
            },
            Some(d) => match exec::execute_capture_bytes(&plan, &subject_path, verb, reg) {
                Ok(out) => route_result(d, &out, reg),
                Err(e) => die(format!("{verb_name}: {e}")),
            },
        },
    }
}

/// Render a plan as a one-line route — `from →[conv: tier]→ … →(verb)→ to` — for
/// `--explain` and the teaching 415. `fallback_start` types the head when the plan
/// has no steps (shouldn't happen, but keeps it total). `color` emits ANSI styling
/// (auto-on in a TTY via [`use_color`]); the teaching 415 passes `false` (stderr).
///
/// Cost is shown by *color*, not inline text — converter names are dim, and only
/// `lossy`/`network` edges carry a `(tier)` marker (kept even in plain mode, since
/// those are the edges that matter; `cheap`/`normal`/`free` are noise).
fn render_route(plan: &negotiation::Plan, fallback_start: &str, verb_name: &str, reg: &Value, color: bool) -> String {
    let mut line = plan.steps.first().map(|s| s.from.clone()).unwrap_or_else(|| fallback_start.to_string());
    for s in &plan.steps {
        match &s.kind {
            negotiation::StepKind::Convert(name) => {
                let tier = channel_tier(reg, name);
                let notable = tier == "lossy" || tier == "network";
                let marker = if notable { format!(" ({tier})") } else { String::new() };
                if color {
                    let arrow = format!("{}→{C_RESET}", tier_color(&tier));
                    let body = format!("{C_DIM}{name}{C_RESET}{}{marker}{}", tier_color(&tier), C_RESET);
                    line.push_str(&format!(" {arrow} {body} {arrow} {}", s.to));
                } else {
                    line.push_str(&format!(" → {name}{marker} → {}", s.to));
                }
            }
            negotiation::StepKind::Verb(inst) => {
                let label = if inst.is_empty() { verb_name } else { inst.as_str() };
                if color {
                    line.push_str(&format!(" → {C_DIM}({label}){C_RESET} → {}", s.to));
                } else {
                    line.push_str(&format!(" → ({label}) → {}", s.to));
                }
            }
        }
    }
    line
}

// Hand-rolled ANSI (the scope is ~4 codes — not worth a dep). `use_color` gates them.
const C_RESET: &str = "\x1b[0m";
const C_DIM: &str = "\x1b[2m";
const C_YELLOW: &str = "\x1b[33m"; // lossy
const C_MAGENTA: &str = "\x1b[35m"; // network

/// The accent color for a cost tier — only `lossy`/`network` stand out; the rest
/// render at the terminal default (empty prefix).
fn tier_color(tier: &str) -> &'static str {
    match tier {
        "lossy" => C_YELLOW,
        "network" => C_MAGENTA,
        _ => "",
    }
}

/// Whether to emit ANSI styling: a real terminal, `NO_COLOR` unset, `TERM` not
/// `dumb`. The single gate — callers pass the bool into renderers rather than
/// re-probing. Piped/redirected stdout (incl. the bats suite) ⇒ plain.
fn use_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM").as_deref() == Ok("dumb") {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// The cmd template a plan step runs: a converter's channel `cmd`, the verb's own
/// `cmd` (plain verb), or the chosen usage channel's `cmd` (instrument). `None`
/// when none is declared (e.g. a `present` identity verb).
fn step_cmd(step: &negotiation::Step, verb: &Value, reg: &Value) -> Option<String> {
    let channel_cmd = |name: &str| -> Option<String> {
        reg.get("channels")
            .and_then(Value::as_array)?
            .iter()
            .find(|c| c.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|c| c.get("cmd").and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    match &step.kind {
        negotiation::StepKind::Convert(name) => channel_cmd(name),
        negotiation::StepKind::Verb(inst) if !inst.is_empty() => channel_cmd(inst),
        negotiation::StepKind::Verb(_) => verb.get("cmd").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from),
    }
}

/// `--explain-with steps`: an annotated per-step list — each type transition plus
/// the exact `cmd` *template* goo runs (placeholders intact, so the plumbing is
/// visible). The "what happens & how" view.
fn render_steps(plan: &negotiation::Plan, verb: &Value, verb_name: &str, reg: &Value) -> String {
    let mut out = String::new();
    for (i, s) in plan.steps.iter().enumerate() {
        let label = match &s.kind {
            negotiation::StepKind::Convert(name) => name.clone(),
            negotiation::StepKind::Verb(inst) if inst.is_empty() => format!("{verb_name} (verb)"),
            negotiation::StepKind::Verb(inst) => format!("{inst} (verb)"),
        };
        out.push_str(&format!("  {}. {label}: {} → {}\n", i + 1, s.from, s.to));
        if let Some(cmd) = step_cmd(s, verb, reg) {
            out.push_str(&format!("       {cmd}\n"));
        }
    }
    out.trim_end().to_string()
}

/// `--explain-with shell`: the commands goo runs, in order, with the subject
/// substituted into the input-reading placeholders when a real file is known
/// (else placeholders stay literal — honest for a virtual `@type` subject). NOT a
/// reconstructed one-liner: goo feeds each step the previous step's output, so the
/// commands are shown per-line with their `# from → to` data flow, not piped.
fn render_shell(plan: &negotiation::Plan, verb: &Value, reg: &Value, subj_path: Option<&str>) -> String {
    let mut out = String::from("  commands (in run order — each step reads the previous step's output):\n");
    // Only the *first* cmd-bearing step reads the subject; substitute the real path
    // there. Later steps' `{in.path}` is the prior step's output (a temp file goo
    // threads) — left literal rather than dishonestly re-pointed at the subject.
    let mut first = true;
    for s in &plan.steps {
        let Some(cmd) = step_cmd(s, verb, reg) else { continue };
        out.push_str(&format!("    # {} → {}\n", s.from, s.to));
        let rendered = if first { substitute_subject(&cmd, subj_path) } else { cmd };
        out.push_str(&format!("    {rendered}\n"));
        first = false;
    }
    out.trim_end().to_string()
}

/// Fill the subject path into the input placeholders for the `shell` view. Only
/// path placeholders, and only when a real file is known; everything else (incl.
/// `{subject.text}` for a virtual subject) is left literal rather than blanked.
fn substitute_subject(cmd: &str, subj_path: Option<&str>) -> String {
    let Some(p) = subj_path else { return cmd.to_string() };
    let q = format!("'{}'", p.replace('\'', "'\\''"));
    cmd.replace("{in.path|q}", &q)
        .replace("{in.path}", p)
        .replace("{subject.metadata.path|q}", &q)
        .replace("{subject.metadata.path}", p)
}

/// The edge label for one plan step in a route drawing: the converter/instrument
/// name, with a `(lossy)`/`(network)` marker on the edges that matter.
fn edge_label(step: &negotiation::Step, verb_name: &str, reg: &Value) -> String {
    match &step.kind {
        negotiation::StepKind::Convert(name) => {
            let tier = channel_tier(reg, name);
            if tier == "lossy" || tier == "network" {
                format!("{name} ({tier})")
            } else {
                name.clone()
            }
        }
        negotiation::StepKind::Verb(inst) => {
            let label = if inst.is_empty() { verb_name } else { inst.as_str() };
            format!("({label})")
        }
    }
}

/// `--paths` text drawing (§4.2): the ranked routes, drawn **vertically** — one
/// hop per line, indented under each route — so K paths read as a tall list, never
/// a wall of 200-char lines. Ordered as the planner decides: by Accept preference
/// first (so #1 delivers the most-preferred representation), then by cost — which
/// is why the per-route cost isn't strictly ascending across different Accepts.
fn render_paths_text(routes: &[negotiation::Plan], subject_type: &str, verb_name: &str, reg: &Value, max_hops: u8, color: bool) -> String {
    let mut out = format!("{} route(s) from {subject_type} (≤{max_hops} hops/layer, most-preferred first):\n", routes.len());
    for (i, plan) in routes.iter().enumerate() {
        out.push_str(&format!("\n  {}. cost {} → {}\n", i + 1, plan.cost, plan.delivered));
        let start = plan.steps.first().map(|s| s.from.as_str()).unwrap_or(subject_type);
        out.push_str(&format!("       {start}\n"));
        for s in &plan.steps {
            let label = edge_label(s, verb_name, reg);
            let label = if color { format!("{C_DIM}{label}{C_RESET}") } else { label };
            out.push_str(&format!("         → {label} → {}\n", s.to));
        }
    }
    out.trim_end().to_string()
}

fn type_id(order: &mut Vec<String>, t: &str) -> usize {
    match order.iter().position(|x| x == t) {
        Some(p) => p,
        None => {
            order.push(t.to_string());
            order.len() - 1
        }
    }
}

/// `--paths --format mermaid` (§4.2): the routes as a single `graph LR` DAG, where
/// shared types are shared *nodes* (K routes → one readable graph, advisor trap #2)
/// and each edge is labeled by its converter/verb. Renders in any mermaid viewer.
fn render_paths_mermaid(routes: &[negotiation::Plan], verb_name: &str, reg: &Value) -> String {
    let mut order: Vec<String> = Vec::new(); // types, first-seen order → node ids
    let mut edges: Vec<(usize, usize, String)> = Vec::new();
    for plan in routes {
        for s in &plan.steps {
            let from = type_id(&mut order, &s.from);
            let to = type_id(&mut order, &s.to);
            let label = edge_label(s, verb_name, reg).replace('|', "/");
            if !edges.iter().any(|(f, t, l)| *f == from && *t == to && *l == label) {
                edges.push((from, to, label));
            }
        }
    }
    let mut out = String::from("graph LR\n");
    for (i, t) in order.iter().enumerate() {
        out.push_str(&format!("  n{i}[\"{}\"]\n", t.replace('"', "'")));
    }
    for (f, t, l) in &edges {
        out.push_str(&format!("  n{f} -->|{l}| n{t}\n"));
    }
    out.trim_end().to_string()
}

/// The declared cost tier of a channel, for `--explain` display.
fn channel_tier(reg: &Value, name: &str) -> String {
    reg["channels"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["name"].as_str() == Some(name)))
        .and_then(|c| c["cost"].as_str())
        .unwrap_or("normal")
        .to_string()
}

/// The `tool` a channel declares, if any.
fn channel_tool(reg: &Value, name: &str) -> Option<String> {
    reg.get("channels")
        .and_then(|v| v.as_array())
        .and_then(|a| a.iter().find(|c| c.get("name").and_then(|n| n.as_str()) == Some(name)))
        .and_then(|c| c.get("tool").and_then(|t| t.as_str()))
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// How deep the teaching 415 re-searches — deep enough to find any realistic
/// route, bounded so a 415 stays fast on a large converter graph (§4.1).
const TEACH_HOPS: u8 = 8;

/// Teaching 415 (§4.1): the run found no route within the user's hop budget. Re-search
/// at a deep-but-bounded budget (with the *installed* tools, so the suggestion is
/// runnable) and, if a route exists, return a multi-line message that shows it and
/// names the flag that would allow it — `--hops N` when the extra depth is input
/// coercion, `--force` when it's >1 output hop (which `--hops` doesn't raise).
/// `None` = no route even deep, so it's a genuine 415 (the tool hint handles the
/// missing-tool case, and a route needing both more hops *and* a tool falls through
/// to the plain 415 — an acceptable corner).
fn deeper_route_hint(subject_type: &str, verb: &Value, target: &negotiation::Target, reg: &Value, available: &[String], current: negotiation::Hops) -> Option<String> {
    let deep = negotiation::Hops { a: TEACH_HOPS, b: TEACH_HOPS };
    let plan = negotiation::plan_request_using_bounded(subject_type, verb, target, reg, available, None, deep)?;
    // Count converter hops on each side of the verb edge.
    let (mut a_hops, mut b_hops, mut seen_verb) = (0u8, 0u8, false);
    for s in &plan.steps {
        match &s.kind {
            negotiation::StepKind::Verb(_) => seen_verb = true,
            negotiation::StepKind::Convert(_) if !seen_verb => a_hops += 1,
            negotiation::StepKind::Convert(_) => b_hops += 1,
        }
    }
    // Not actually a depth problem (route fits the current budget) — let the caller
    // fall through to its other hints. Guards against a spurious teach.
    if a_hops <= current.a && b_hops <= current.b {
        return None;
    }
    let verb_name = verb.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let route = render_route(&plan, subject_type, verb_name, reg, false);
    // Name the axis that's actually blocked and the flag that raises it. `--hops`
    // only lifts layer A; an output chain >1 hop needs `--force` (§4.1).
    let (intro, suggestion) = if b_hops > current.b.max(1) {
        ("no route within the output-coercion budget".to_string(), "with --force".to_string())
    } else {
        (format!("no route within {} input hop(s)", current.a), format!("with --hops {a_hops} (or --force)"))
    };
    Some(format!("415 · {intro} — a deeper route exists:\n    {route}\n  allow it {suggestion}"))
}

/// For a 415 where tool-pruning may be the cause: re-plan tool-agnostically (as
/// if everything were installed); if a route exists, return " — install: X, Y"
/// naming the missing tools *on that route*. Empty if no route exists regardless.
fn route_missing_tools_hint(subject_type: &str, verb: &Value, target: &negotiation::Target, reg: &Value, missing: &[String], hops: negotiation::Hops) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let all: Vec<String> = {
        let (a, m) = channel_tools(reg);
        a.into_iter().chain(m).collect()
    };
    // Re-plan tool-agnostically but under the SAME hop budget the run used, so the
    // hint names tools on a route the user could actually reach.
    let Some(ideal) = negotiation::plan_request_using_bounded(subject_type, verb, target, reg, &all, None, hops) else {
        return String::new(); // no route even with everything installed — not a tool problem
    };
    let mut needed: Vec<String> = Vec::new();
    for step in &ideal.steps {
        let name = match &step.kind {
            negotiation::StepKind::Convert(n) | negotiation::StepKind::Verb(n) => n,
        };
        if let Some(t) = channel_tool(reg, name) {
            if missing.contains(&t) && !needed.contains(&t) {
                needed.push(t);
            }
        }
    }
    if needed.is_empty() { String::new() } else { format!(" — install: {}", needed.join(", ")) }
}

/// True if `tool` is on PATH (or, if it contains a slash, exists as a file).
fn tool_on_path(tool: &str) -> bool {
    if tool.contains('/') {
        return std::path::Path::new(tool).exists();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file()))
        .unwrap_or(false)
}

/// The distinct channel `tool`s declared in the registry, partitioned by PATH
/// presence — `(available, missing)`. The planner prunes channels whose tool is
/// missing; the missing set feeds the 415 hint.
fn channel_tools(reg: &Value) -> (Vec<String>, Vec<String>) {
    let (mut available, mut missing) = (Vec::new(), Vec::new());
    let Some(chs) = reg.get("channels").and_then(|v| v.as_array()) else { return (available, missing) };
    for ch in chs {
        if let Some(t) = ch.get("tool").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let bucket = if tool_on_path(t) { &mut available } else { &mut missing };
            if !bucket.iter().any(|x| x == t) {
                bucket.push(t.to_string());
            }
        }
    }
    (available, missing)
}

/// Render a verb and execute it, honouring `confirm`.
fn exec_verb(
    reg: &Value,
    verb: &Value,
    subject: &Value,
    object: &Value,
    adverbs: &Value,
) -> i32 {
    // Route through the negotiation engine when (a) the verb is `present` (the
    // subject is the result; delivery is the engine's job), (b) it's implemented
    // by `usage` channels (no own cmd — the planner picks one, 2b), or (c) there's
    // a type gap so the subject needs input coercion (4b). Otherwise the subject
    // already fits and the verb has its own cmd: the unchanged legacy render+exec
    // path (parity-safe — no gap, no change).
    if is_present(verb) || has_usage(verb) || needs_coercion(reg, verb, subject) {
        return exec_negotiated(reg, verb, subject, adverbs);
    }
    let rendered = match verbs::render(reg, verb, subject, object, adverbs) {
        Ok(r) => r,
        Err(e) => return die(format!("verb_apply: {e}")),
    };
    if rendered.confirm {
        let verb_name = verb.get("name").and_then(Value::as_str).unwrap_or("");
        if confirm_preapproved(adverbs, verb_name) {
            // Scoped, per-invocation pre-approval — the verb was named in
            // `--confirm-dangerous`. Announced loudly so the bypass is never silent.
            eprintln!("goo: --confirm-dangerous: auto-approving '{verb_name}'");
        } else {
            let chip = if verb.get("destructive").and_then(Value::as_bool).unwrap_or(false) {
                " [!!]"
            } else {
                " [!]"
            };
            let what = verb.get("description").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(verb_name);
            eprintln!("goo: about to {what}{chip}");
            let label = confirm_subject_label(subject);
            if !label.is_empty() {
                eprintln!("       subject: {label}");
            }
            eprintln!("       runs: {}", rendered.cmd);
            eprint!("     proceed? [y/N] ");
            let mut ans = String::new();
            std::io::stdin().read_line(&mut ans).ok();
            match ans.trim() {
                "y" | "Y" | "yes" | "YES" => {}
                _ => {
                    eprintln!("goo: cancelled");
                    return 130;
                }
            }
        }
    }
    // No `--to` → run with inherited stdout (byte-identical to before). With
    // `--to`/`-o`, capture the result and route it to the destination instead.
    match adverbs.get("to").and_then(|v| v.as_str()) {
        None => bash_exec(&rendered.cmd),
        Some(d) => route_result(d, &bash_capture_bytes(&rendered.cmd), reg),
    }
}

/// True when `verb_name` was pre-approved this run via `--confirm-dangerous=…`.
/// Flag-only and per-invocation by design: scoping the bypass to named verbs is
/// the whole point — an exported env var would just be a persistent blanket
/// bypass (the unsafe thing `--no-confirm` was). The run-path gate is suppressed
/// for these verbs; the `confirm`/`destructive` chips still always render in
/// `describe`/`what` (decision D1 — the chip is the verb's nature, not the gate).
fn confirm_preapproved(adverbs: &Value, verb_name: &str) -> bool {
    adverbs
        .get("confirm-dangerous")
        .and_then(Value::as_str)
        .map(|s| s.split(',').map(str::trim).any(|v| v == verb_name))
        .unwrap_or(false)
}

/// A short, human label for the subject in the confirm prompt: id, else title,
/// else a one-line/40-char-capped slice of the text content.
fn confirm_subject_label(subject: &Value) -> String {
    for key in ["id", "title"] {
        if let Some(s) = subject.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    if let Some(t) = subject.get("text").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        let one = t.lines().next().unwrap_or("");
        return if one.chars().count() > 40 {
            format!("{}…", one.chars().take(40).collect::<String>())
        } else {
            one.to_string()
        };
    }
    String::new()
}

/// Warn (don't fail) if a name passed to `--confirm-dangerous` isn't actually a
/// confirm/destructive verb — a typo (`detroy`) or a misunderstanding. Scans the
/// registry once; a name matching no confirm/destructive verb entry is flagged.
fn warn_unknown_confirm_dangerous(reg: &Value, adverbs: &Value) {
    let Some(list) = adverbs.get("confirm-dangerous").and_then(Value::as_str) else { return };
    let verbs = reg.get("verbs").and_then(Value::as_array).cloned().unwrap_or_default();
    let is_dangerous = |name: &str| {
        verbs.iter().any(|v| {
            v.get("name").and_then(Value::as_str) == Some(name)
                && (v.get("confirm").and_then(Value::as_bool).unwrap_or(false)
                    || v.get("destructive").and_then(Value::as_bool).unwrap_or(false))
        })
    };
    for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !is_dangerous(name) {
            eprintln!("goo: --confirm-dangerous: '{name}' is not a confirm/destructive verb (typo?)");
        }
    }
}

/// Route a captured verb result to a `--to`/`-o` destination (file/clipboard), or
/// die cleanly on a non-writable/failed destination. See goo-protocol §12.
fn route_result(dest: &str, bytes: &[u8], reg: &Value) -> i32 {
    match address::write_to(dest, bytes, reg) {
        Ok(()) => 0,
        Err(e) => die(format!("--to: {e}")),
    }
}

/// True if any of `verb.accepts` accepts `text/plain` (subtype-aware).
fn accepts_text(verb: &Value, reg: &Value) -> bool {
    verb.get("accepts")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|p| p.as_str()).any(|p| mime::is_subtype("text/plain", p, reg)))
        .unwrap_or(false)
}

// ---------------- subcommand: plugins ----------------

fn cmd_plugins() -> i32 {
    let reg = registry::load_all();
    let plugins = reg.get("plugins").and_then(|p| p.as_array());
    let plugins = match plugins {
        Some(p) if !p.is_empty() => p,
        _ => {
            let dir = std::env::var("COSMIC_GOO_BUILTIN_PLUGINS_DIR").unwrap_or_default();
            eprintln!("(no plugins loaded — check COSMIC_GOO_BUILTIN_PLUGINS_DIR={dir})");
            return 0;
        }
    };
    for p in plugins {
        let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let file = p.get("file").and_then(|f| f.as_str()).unwrap_or("");
        match p.get("description").and_then(|d| d.as_str()) {
            Some(d) => println!("{name} — {d}"),
            None => println!("{name}"),
        }
        println!("  {file}");
    }
    0
}

// ---------------- subcommand: list ----------------

fn cmd_list(source_name: Option<&str>) -> i32 {
    let name = match source_name {
        Some(n) if !n.is_empty() => n,
        _ => return die("list: expected a source name"),
    };
    let reg = registry::load_all();
    let source = reg
        .get("sources")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.iter().find(|s| s.get("name").and_then(|n| n.as_str()) == Some(name)));
    let source = match source {
        Some(s) => s,
        None => return die(format!("list: no source named '{name}'")),
    };
    match source.get("list_cmd").and_then(|c| c.as_str()).filter(|s| !s.is_empty()) {
        Some(lc) => {
            print!("{}", bash_capture(lc));
            0
        }
        None => die(format!("list: source '{name}' has no list_cmd")),
    }
}

// ---------------- subcommand: describe ----------------

fn cmd_describe(verb_name: Option<&str>) -> i32 {
    let name = match verb_name {
        Some(n) if !n.is_empty() => n,
        _ => return die("describe: expected a verb name"),
    };
    let reg = registry::load_all();
    // Walk every (name, accepts) contributor — for a polymorphic verb (e.g. `stop`
    // declared by network/bluetooth/mpris) we render ALL impls stacked under one
    // header, not just the first match. The header carries chips (`[!]`, `[!!]`,
    // `×N`) that summarize the language-level facts; per-contributor blocks carry
    // the impl-specific details (accepts, cmd, prompt, plugin, confirm flag).
    // See `doc/design/completion-polish.md` §6 slice 1 for the locked format and
    // §2 for the chip vocabulary.
    let contributors: Vec<&Value> = reg
        .get("verbs")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter(|v| v.get("name").and_then(Value::as_str) == Some(name)).collect())
        .unwrap_or_default();
    if contributors.is_empty() {
        return die(format!("describe: no verb named '{name}'"));
    }

    // Header chips: `[!!]` (any contributor destructive) > `[!]` (any contributor
    // confirms) > `×N` (when N > 1). Aggregation is INTENTIONALLY CONSERVATIVE — a
    // verb name is "destructive" / "confirm" if ANY impl declares it. A polymorphic
    // `stop` that destroys data in plugin A but only signals exit in plugin B
    // displays `[!!]` regardless of which subject the user will hand it. After
    // dispatch the chip may have over-warned, but at language-level pick time the
    // user hasn't picked a subject yet — over-warning is the correct safety bias.
    // DO NOT "fix" this to be subject-aware: that's a per-subject OPTIONS concern
    // (where `confirm`/`destructive` already surface per-impl-correctly), not the
    // language-level describe surface.
    let any_confirm = contributors.iter().any(|v| v.get("confirm").and_then(Value::as_bool) == Some(true));
    let any_destructive = contributors.iter().any(|v| v.get("destructive").and_then(Value::as_bool) == Some(true));
    let n = contributors.len();
    let mut chips: Vec<String> = Vec::new();
    if any_destructive {
        chips.push("[!!]".into());
    }
    if any_confirm && !any_destructive {
        // `[!]` is subsumed by `[!!]` (destructive implies confirm-worthy); don't
        // render both — the stronger chip wins.
        chips.push("[!]".into());
    }
    if n > 1 {
        chips.push(format!("×{n}"));
    }
    let chip_suffix = if chips.is_empty() { String::new() } else { format!("  {}", chips.join(" ")) };
    let mut out = format!("{name}{chip_suffix}");

    // Per-contributor block. Single-impl verbs collapse to today's format (minus
    // the verb header — already on the chip line). Polymorphic verbs get each
    // contributor as an indented block; the order is registry order (the same
    // order `verbs::for_subject` and `lookup` will see, so display matches dispatch).
    for v in &contributors {
        out.push('\n');
        out.push_str(&render_contributor_block(v, n > 1));
    }
    println!("{out}");
    0
}

/// Render one verb-contributor block. With `indent` true (polymorphic case),
/// each line is prefixed for visual grouping under the header.
fn render_contributor_block(verb: &Value, indent: bool) -> String {
    let prefix = if indent { "  " } else { "" };
    let s = |k: &str| verb.get(k).and_then(|v| v.as_str());
    let mut lines: Vec<String> = Vec::new();
    if indent {
        // Section sub-header: the plugin provenance — humans recognize plugins faster
        // than `accepts` patterns, so it leads.
        if let Some(p) = s("_plugin") {
            lines.push(format!("[{p}]"));
        }
    }
    if let Some(d) = s("description") {
        lines.push(format!("description: {d}"));
    }
    // accepts: render `(no subject)` for empty arrays (subjectless verbs like
    // lock / volume-up); render the patterns joined otherwise. This closes the
    // slice-1 papercut where subjectless verbs displayed blank `accepts: `.
    // `accepts = ["*/*"]` is a catch-all that DOES take a subject (xdg-open) —
    // it renders as `*/*`, not `(no subject)`. See completion-polish.md §6 slice 2.
    let accepts_line = match verb.get("accepts").and_then(Value::as_array) {
        Some(a) if a.is_empty() => "accepts: (no subject)".to_string(),
        Some(a) => {
            let joined = a.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join(", ");
            format!("accepts: {joined}")
        }
        None => "accepts: (no subject)".to_string(),
    };
    lines.push(accepts_line);
    if let Some(ot) = s("object_type") {
        lines.push(format!("object_type: {ot}"));
    }
    match verb.get("default_for") {
        Some(Value::Array(a)) => {
            let joined = a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
            lines.push(format!("default_for: {joined}"));
        }
        Some(Value::String(d)) => lines.push(format!("default_for: {d}")),
        _ => {}
    }
    if let Some(ua) = verb.get("uses_adverbs").and_then(|u| u.as_array()) {
        let joined = ua.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
        lines.push(format!("uses_adverbs: {joined}"));
    }
    if let Some(cmd) = s("cmd") {
        lines.push(format!("cmd: {cmd}"));
    }
    if let Some(prompt) = s("prompt") {
        lines.push(format!("prompt:\n  {}", prompt.replace('\n', "\n  ")));
    }
    if verb.get("confirm").and_then(|c| c.as_bool()) == Some(true) {
        lines.push("confirm: true".to_string());
    }
    if verb.get("destructive").and_then(|c| c.as_bool()) == Some(true) {
        lines.push("destructive: true".to_string());
    }
    if !indent {
        // Single-impl case: keep today's trailing provenance line.
        if let Some(p) = s("_plugin") {
            lines.push(format!("provided by plugin: {p}"));
        }
    }
    lines.iter().map(|l| format!("{prefix}{l}")).collect::<Vec<_>>().join("\n")
}

// ---------------- subcommand: validate ----------------

fn cmd_validate() -> i32 {
    let reg = registry::load_all();
    let arr = |k: &str| reg.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut errors = 0u32;
    let err = |msg: String| {
        eprintln!("{msg}");
    };

    // Reserved subcommands an alias can never shadow.
    const RESERVED: &[&str] = &[
        "compose", "list", "describe", "plugins", "validate", "dispatch", "reload", "__complete", "help",
        "options", "what", "-h", "--help",
    ];

    // 1. Verbs: a declared accept pattern list can't contain empty strings.
    for v in arr("verbs") {
        let has_empty = v
            .get("accepts")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().any(|p| p.as_str() == Some("")))
            .unwrap_or(false);
        if has_empty {
            err(format!("verb \"{}\" has an empty accept pattern", v.get("name").and_then(|n| n.as_str()).unwrap_or("")));
            errors += 1;
        }
    }

    // 2. Adverbs declare scope (applies_to or applies_to_verbs).
    for a in arr("adverbs") {
        let scoped = a.get("applies_to").is_some() || a.get("applies_to_verbs").is_some();
        if !scoped {
            err(format!("adverb \"{}\" has neither applies_to nor applies_to_verbs", a.get("name").and_then(|n| n.as_str()).unwrap_or("")));
            errors += 1;
        }
        // 3. Selector adverbs should have a values object.
        let kind = a.get("kind").and_then(|k| k.as_str()).unwrap_or("selector");
        let nvalues = a.get("values").and_then(|v| v.as_object()).map(|o| o.len()).unwrap_or(0);
        if kind == "selector" && nvalues == 0 {
            err(format!("selector adverb \"{}\" has no values", a.get("name").and_then(|n| n.as_str()).unwrap_or("")));
            errors += 1;
        }
    }

    // 4. Sigils: single char, not a reserved/native prefix, must have expansion.
    for sg in arr("sigils") {
        let ch = sg.get("char").and_then(|c| c.as_str()).unwrap_or("");
        let expands = sg.get("expands").and_then(|e| e.as_str()).unwrap_or("");
        if ch.chars().count() != 1 {
            err(format!("sigil \"{ch}\" must be exactly one character"));
            errors += 1;
        }
        if let Some(c) = ch.chars().next() {
            if c.is_ascii_alphanumeric() || matches!(c, ':' | '+' | '.' | '/' | '~') {
                err(format!("sigil \"{ch}\" collides with a reserved/native prefix (: + . / ~ alnum)"));
                errors += 1;
            }
        }
        if expands.is_empty() {
            err(format!("sigil \"{ch}\" has no expansion"));
            errors += 1;
        }
    }

    // 5. Plugin tier (optional) must be core|desktop|cosmic.
    for p in arr("plugins") {
        if let Some(tier) = p.get("tier").and_then(|t| t.as_str()) {
            if !matches!(tier, "core" | "desktop" | "cosmic") {
                err(format!("plugin \"{}\" has invalid tier \"{tier}\" (want core|desktop|cosmic)", p.get("name").and_then(|n| n.as_str()).unwrap_or("")));
                errors += 1;
            }
        }
    }

    // 6. Command aliases: name + expands, must not shadow a subcommand.
    let verb_names: Vec<String> = arr("verbs")
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    for a in arr("aliases") {
        let aname = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if aname.is_empty() {
            continue;
        }
        let aexp = a.get("expands").and_then(|e| e.as_str()).unwrap_or("");
        if aexp.is_empty() {
            err(format!("alias \"{aname}\" has no expansion"));
            errors += 1;
        }
        if RESERVED.contains(&aname) {
            err(format!("alias \"{aname}\" shadows a reserved subcommand and will never fire"));
            errors += 1;
        }
        if verb_names.iter().any(|v| v == aname) {
            eprintln!("warning: alias \"{aname}\" shadows a verb of the same name (alias wins)");
        }
    }

    // 7. Dispatch rules: need a `matches` and a `verb` that exists.
    for (i, rule) in arr("dispatch").iter().enumerate() {
        let rmatch = rule.get("matches").and_then(|m| m.as_str()).unwrap_or("");
        let rverb = rule.get("verb").and_then(|v| v.as_str()).unwrap_or("");
        if rmatch.is_empty() {
            err(format!("dispatch rule #{} has no \"matches\" pattern", i + 1));
            errors += 1;
        }
        if rverb.is_empty() {
            err(format!("dispatch rule #{} has no \"verb\"", i + 1));
            errors += 1;
        } else if !verb_names.iter().any(|v| v == rverb) {
            err(format!("dispatch rule #{} routes to unknown verb \"{rverb}\"", i + 1));
            errors += 1;
        }
    }

    // 8. Channels (coercion converters): emits concrete, accepts non-empty, known
    // cost/consumes vocab (see negotiation §2.5). No-op until a plugin ships them.
    for msg in negotiation::validate_channels(&reg) {
        err(msg);
        errors += 1;
    }

    // 9. Detectors / checkers (content typing): impl present (cmd xor builtin),
    // known tier vocab, checker has a target (see doc/design/detection.md).
    // No-op until a plugin ships them.
    for msg in mime::validate_detectors(&reg).into_iter().chain(mime::validate_checkers(&reg)) {
        err(msg);
        errors += 1;
    }

    if errors == 0 {
        let n = |k: &str| arr(k).len();
        println!(
            "goo validate: OK ({} plugins, {} types, {} sources, {} verbs, {} adverbs, {} sigils, {} aliases, {} channels, {} dispatch)",
            n("plugins"), n("types"), n("sources"), n("verbs"), n("adverbs"), n("sigils"), n("aliases"), n("channels"), n("dispatch")
        );
        0
    } else {
        eprintln!("goo validate: {errors} error(s)");
        1
    }
}

// ---------------- subcommand: dispatch (content classification) ----------------

fn cmd_dispatch(input_arg: Option<&str>) -> i32 {
    let mut input = input_arg.unwrap_or("").to_string();
    if input.is_empty() && !std::io::stdin().is_terminal() {
        input = read_stdin();
    }
    if input.is_empty() {
        return die("dispatch: no input (give a positional or pipe stdin)");
    }
    let reg = registry::load_all();

    if let Some(m) = disp::dispatch_match(&reg, &input) {
        let verb_name = match m.get("verb").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            Some(v) => v,
            None => return die("dispatch: matched rule has no verb"),
        };
        let type_ = m.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain");
        let adverbs = m.get("adverbs").cloned().unwrap_or_else(|| json!({}));
        // subject = {type, text:input} overlaid with the rule's `fields`, then
        // .id defaults to .text.
        let mut subject = json!({ "type": type_, "text": input });
        if let Some(fields) = m.get("fields").and_then(|f| f.as_object()) {
            let obj = subject.as_object_mut().unwrap();
            for (k, v) in fields {
                obj.insert(k.clone(), v.clone());
            }
        }
        let text = subject.get("text").cloned().unwrap_or(Value::Null);
        let need_id = subject.get("id").map(|v| v.is_null()).unwrap_or(true);
        if need_id {
            subject.as_object_mut().unwrap().insert("id".into(), text);
        }
        let verb = match verbs::lookup(&reg, verb_name, None) {
            Some(v) => v,
            None => return die(format!("dispatch: rule routes to unknown verb '{verb_name}'")),
        };
        exec_verb(&reg, &verb, &subject, &Value::Null, &adverbs)
    } else {
        let subject = match address::resolve(&input, &reg, None) {
            Ok(s) => s,
            Err(_) => return die(format!("dispatch: cannot resolve '{input}'")),
        };
        let type_ = subject.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain");
        match verbs::default_for(&reg, type_) {
            Some(verb) => exec_verb(&reg, &verb, &subject, &Value::Null, &json!({})),
            None => die(format!("dispatch: no rule matched and no default verb for type '{type_}'")),
        }
    }
}

// ---------------- subcommand: __complete ----------------

fn cmd_complete(args: &[String]) -> i32 {
    let reg = registry::load_all();
    let arr = |k: &str| reg.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let stage = args.first().map(String::as_str).unwrap_or("");
    // First positional after the stage (single-arg stages use this); additional
    // positionals (multi-arg OPTIONS-backed stages) reach via `args.get(2)`/`(3)`.
    let arg = args.get(1).map(String::as_str).unwrap_or("");
    match stage {
        "subcommands" => {
            println!("list\ndescribe\nplugins\nvalidate\ncompose\ndispatch\nreload\nwhat\nhelp");
            for v in arr("verbs") {
                if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
                    println!("{n}");
                }
            }
            for a in arr("aliases") {
                if let Some(n) = a.get("name").and_then(|n| n.as_str()) {
                    println!("{n}");
                }
            }
        }
        // Dedupe: polymorphic verbs are kept as distinct (name, accepts) entries in
        // the merged registry (see `registry::merge_verbs`). For tab-completion the
        // verb NAME is what matters; emitting `stop` three times would clutter the
        // bash menu without disambiguating (display-vs-insert makes a chip impossible).
        // Polymorphism reveals itself at the SUBJECT stage (the heterogeneous menu
        // at `goo <verb> <TAB>`) and via `goo describe <verb>`; see
        // `doc/design/completion-polish.md` §3 D3.
        "verbs" => {
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for v in arr("verbs") {
                if let Some(n) = v.get("name").and_then(Value::as_str) {
                    seen.insert(n.to_string());
                }
            }
            for n in &seen {
                println!("{n}");
            }
        }
        "sources" => print_names(&arr("sources"), "name"),
        "adverbs" => {
            if arg.is_empty() {
                return 0;
            }
            if let Some(v) = arr("verbs").iter().find(|v| v.get("name").and_then(|n| n.as_str()) == Some(arg)) {
                if let Some(ua) = v.get("uses_adverbs").and_then(|u| u.as_array()) {
                    for a in ua {
                        if let Some(s) = a.as_str() {
                            println!("{s}");
                        }
                    }
                }
            }
        }
        "adverb-values" => {
            if arg.is_empty() {
                return 0;
            }
            if let Some(a) = arr("adverbs").iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(arg)) {
                if let Some(vals) = a.get("values").and_then(|v| v.as_object()) {
                    for k in vals.keys() {
                        println!("{k}");
                    }
                }
            }
        }
        "source-prefixes" => {
            for s in arr("sources") {
                if let Some(p) = s.get("prefix").and_then(|p| p.as_str()) {
                    println!(":{p}:");
                }
            }
        }
        "sigils" => print_names(&arr("sigils"), "char"),
        "verb-accepts-handle" => {
            if arg.is_empty() {
                return 0;
            }
            if let Some(v) = arr("verbs").iter().find(|v| v.get("name").and_then(|n| n.as_str()) == Some(arg)) {
                let has_handle = v
                    .get("accepts")
                    .and_then(|a| a.as_array())
                    .map(|arr| arr.iter().filter_map(|p| p.as_str()).any(|p| !p.starts_with("text/")))
                    .unwrap_or(false);
                println!("{}", if has_handle { "yes" } else { "no" });
            }
        }
        // Reads `accepts` from the registry directly — does NOT go through
        // `options::options_for`, which would filter to subject-applicable verbs
        // and could exclude the verb being asked about (the verb's own subjectless
        // verbs aren't applicable to ANY typed subject, so any dummy-subject query
        // would drop them). Subjectless ↔ `accepts == []` (verified across current
        // plugins: volume-up, mute-toggle, play-pause, lock, suspend, …);
        // `accepts = ["*/*"]` is a catch-all that DOES take a subject (xdg-open is
        // the example) — render as `yes`. See doc/design/completion-polish.md §6
        // slice 2 for the derivation lock.
        //
        // For polymorphic verbs (multiple contributors of the same name) the answer
        // is `yes` if ANY contributor requires a subject — symmetric with
        // `cmd_describe`'s conservative chip-aggregation rule.
        "verb-needs-subject" => {
            if arg.is_empty() {
                return 0;
            }
            let needs = arr("verbs")
                .iter()
                .filter(|v| v.get("name").and_then(Value::as_str) == Some(arg))
                .any(|v| match v.get("accepts").and_then(Value::as_array) {
                    Some(a) => !a.is_empty(),
                    None => true, // conservative — missing accepts treated as "needs one"
                });
            // Also handle the unknown-verb case (no contributors at all): conservative
            // `yes` so a typo doesn't accidentally claim "no subject needed."
            let any_contributor = arr("verbs").iter().any(|v| v.get("name").and_then(Value::as_str) == Some(arg));
            let answer = if any_contributor { needs } else { true };
            println!("{}", if answer { "yes" } else { "no" });
        }
        // Subject candidates for `goo <verb> <TAB>`, ranked by accepts-specificity
        // (slice #5 / §5.1). Two improvements over a flat per-pattern walk:
        //  1. Polymorphic union — gather EVERY impl of the verb name and union
        //     their `accepts`, so a polymorphic verb (`connect` over ssh/bt/net)
        //     offers subjects from all its dispatch targets, not just the first
        //     impl's. (A single `find` would miss the rest.)
        //  2. Specificity ranking — sources whose `emits` matches a more-specific
        //     accept pattern lead (`accepts_specificity`, the same SSOT scoring
        //     `lookup`/`for_subject` use). Ranking is per-SOURCE (all items of a
        //     higher-spec source precede a lower one's), with registry order as a
        //     stable tiebreak. Grouped consumers (zsh/fish/compose-GUI) present
        //     this order; bash re-sorts its flat menu alphabetically (by design).
        // Ids are deduped globally (first-seen wins, preserving rank) — which also
        // fixes the old nested loop's incidental double-listing of a source that
        // matched more than one accept pattern.
        // `goo <text-verb> <TAB>` with no subject yet → preview what the implicit
        // fallback (PRIMARY selection → clipboard) would resolve to, so the user
        // sees what Enter will grab *before* committing (§5.4 implicit-subject
        // preview / roadmap #6). Emits one ready-to-display hint line, or nothing
        // (non-text verb, empty selection+clipboard, or a peek timeout). The peek
        // is timeout-bounded (150ms) so a hung/absent compositor can't stall
        // completion. The bin owns the wording (testable here); the shell only
        // wraps it on stderr via the non-destructive hint mechanism. `accepts_text`
        // is the run-path SSOT, so the preview matches what execution would borrow.
        "implicit-preview" => {
            if arg.is_empty() {
                return 0;
            }
            let accepts = arr("verbs")
                .iter()
                .filter(|v| v.get("name").and_then(Value::as_str) == Some(arg))
                .any(|v| accepts_text(v, &reg));
            if !accepts {
                return 0;
            }
            let (text, label) = {
                let p = selection::peek_primary_timed("0.15");
                if !p.is_empty() {
                    (p, "PRIMARY selection")
                } else {
                    (selection::peek_clipboard_timed("0.15"), "clipboard")
                }
            };
            if !text.is_empty() {
                println!("if Enter: '{}'  ({label})", implicit_snippet(&text));
            }
        }
        "verb-subject-items" => {
            if arg.is_empty() {
                return 0;
            }
            let patterns: Vec<String> = {
                let mut seen = std::collections::BTreeSet::new();
                let mut out = Vec::new();
                for v in arr("verbs").iter().filter(|v| v.get("name").and_then(|n| n.as_str()) == Some(arg)) {
                    if let Some(a) = v.get("accepts").and_then(|a| a.as_array()) {
                        for p in a.iter().filter_map(|p| p.as_str()) {
                            if seen.insert(p.to_string()) {
                                out.push(p.to_string());
                            }
                        }
                    }
                }
                out
            };
            if patterns.is_empty() {
                return 0;
            }
            let pat_refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
            // Rank enumerable sources whose emits the verb accepts (skip
            // `enumerate = false` — bulk completion never bulk-lists them).
            let sources = arr("sources");
            let mut ranked: Vec<(i32, usize)> = Vec::new();
            for (i, source) in sources.iter().enumerate() {
                if source.get("enumerate") == Some(&json!(false)) {
                    continue;
                }
                let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
                if emits.is_empty() {
                    continue;
                }
                if let Some(spec) = verbs::accepts_specificity(&pat_refs, emits, &reg) {
                    ranked.push((spec, i));
                }
            }
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            // NOTE(perf): uncached `bash_capture` per source on every <TAB> — the
            // same per-keystroke fan-out the 7b entity cache covers for inference,
            // over the same sources. Sharing that cache here is a tracked follow-up.
            let mut printed = std::collections::HashSet::new();
            for (_, i) in &ranked {
                let Some(lc) = sources[*i].get("list_cmd").and_then(|c| c.as_str()) else {
                    continue;
                };
                if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(bash_capture(lc).trim()) {
                    for it in items {
                        if let Some(id) = it.get("id").and_then(|i| i.as_str()) {
                            if printed.insert(id.to_string()) {
                                println!("{id}");
                            }
                        }
                    }
                }
            }
        }
        "source-items" => {
            if arg.is_empty() {
                return 0;
            }
            let lc = arr("sources").iter().find_map(|s| {
                let by_name = s.get("name").and_then(|n| n.as_str()) == Some(arg);
                let by_prefix = s.get("prefix").and_then(|p| p.as_str()) == Some(arg);
                if by_name || by_prefix {
                    s.get("list_cmd").and_then(|c| c.as_str()).map(str::to_string)
                } else {
                    None
                }
            });
            if let Some(lc) = lc {
                print_ids(&bash_capture(&lc));
            }
        }
        // ---- OPTIONS-backed completion (goo-protocol §7 surface) ----
        // The same `options::options_for` projection the compose-gui consumes — so
        // completion never drifts from the run-path. Subject is any address (file,
        // sigil, `=TYPE`, `:type/`, `goo://…`); resolution failures degrade to "no
        // candidates" rather than an error (completion must never crash the shell).
        "options-allow" => {
            // The verbs applicable to `<subject>` — `options.allow`, one per line.
            // The subject-aware analogue of the `verbs` stage; powers any consumer
            // that wants "what can I do with this?" (a `goo what` wrapper, zsh
            // grouping, an editor plugin).
            if arg.is_empty() {
                return 0;
            }
            if let Ok(subject) = address::resolve(arg, &reg, None) {
                let view = options::options_for(&reg, &subject);
                if let Some(allow) = view.get("allow").and_then(Value::as_array) {
                    for v in allow.iter().filter_map(Value::as_str) {
                        println!("{v}");
                    }
                }
            }
        }
        "options-with" => {
            // The `With:` keys for `<verb>` given `<subject>` — i.e. the adverbs
            // that actually take effect at run time (the `uses_adverbs` gate). The
            // subject-aware counterpart of `adverbs <verb>`; consumed by goo.bash
            // when a subject is present at the `--<TAB>` position so completion
            // mirrors what `goo <verb> <subject>` would actually resolve.
            let verb = arg;
            let subj = args.get(2).map(String::as_str).unwrap_or("");
            if verb.is_empty() || subj.is_empty() {
                return 0;
            }
            if let Ok(subject) = address::resolve(subj, &reg, None) {
                let view = options::options_for(&reg, &subject);
                if let Some(with) = view.get("verbs").and_then(|v| v.get(verb)).and_then(|v| v.get("with")).and_then(Value::as_object) {
                    for k in with.keys() {
                        println!("{k}");
                    }
                }
            }
        }
        _ => {}
    }
    0
}

fn print_names(items: &[Value], key: &str) {
    for it in items {
        if let Some(n) = it.get(key).and_then(|n| n.as_str()) {
            println!("{n}");
        }
    }
}

/// Print `.id` of each item in a JSON-array string, one per line.
fn print_ids(items_json: &str) {
    if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(items_json.trim()) {
        for it in items {
            if let Some(id) = it.get("id").and_then(|i| i.as_str()) {
                println!("{id}");
            }
        }
    }
}

// ---------------- subcommand: compose (scripted only) ----------------

/// Build a sentence (subject → verb → object → adverbs → confirm) and run it.
///
/// The CLI is non-interactive: it drives compose **only** from the scripted
/// `GOO_COMPOSE_ANSWERS` queue (automation / tests) and never spawns a picker.
/// Interactive composition lives in `bin/goo` (bash) and the future native
/// `goo-compose` (#39). Each `dialog_pick` pops one pre-seeded answer; an empty
/// answer cancels (130).
fn cmd_compose() -> i32 {
    let scripted = std::env::var("GOO_COMPOSE_ANSWERS")
        .ok()
        .map(|f| std::path::Path::new(&f).is_file())
        .unwrap_or(false);
    if !scripted {
        eprintln!(
            "goo: interactive compose isn't available in the CLI — use `bin/goo compose` \
             (or the future goo-compose), or drive it with GOO_COMPOSE_ANSWERS"
        );
        return 1;
    }

    let reg = registry::load_all();

    // 1. Subject.
    let subj_addr = match dialog_pick() {
        Some(line) => line.split('\t').next().unwrap_or("").to_string(),
        None => return cancel(),
    };
    let subject = match address::resolve(&subj_addr, &reg, None) {
        Ok(s) => s,
        Err(_) => return die(format!("compose: could not resolve subject '{subj_addr}'")),
    };
    let subject_type = subject.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain").to_string();

    // 2. Verb (must accept the subject's type).
    let mut verb_names: Vec<String> = verbs::for_subject(&reg, &subject)
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .collect();
    verb_names.sort();
    verb_names.dedup();
    if verb_names.is_empty() {
        return die(format!("compose: no verbs accept type {subject_type}"));
    }
    let verb_name = match dialog_pick() {
        Some(v) => v,
        None => return cancel(),
    };
    let verb = match verbs::lookup(&reg, &verb_name, None) {
        Some(v) => v,
        None => return die(format!("compose: unknown verb '{verb_name}'")),
    };

    // 3. Object, if the verb takes one.
    let mut object = Value::Null;
    if verb.get("object_type").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).is_some() {
        let obj_addr = match dialog_pick() {
            Some(line) => line.split('\t').next().unwrap_or("").to_string(),
            None => return cancel(),
        };
        object = match address::resolve(&obj_addr, &reg, None) {
            Ok(o) => o,
            Err(_) => return die("compose: could not resolve object"),
        };
    }

    // 4. Adverbs the verb opts into (one answer per declared adverb).
    let mut adverbs = Map::new();
    if let Some(uses) = verb.get("uses_adverbs").and_then(|u| u.as_array()) {
        for aname_v in uses {
            let aname = match aname_v.as_str() {
                Some(a) => a,
                None => continue,
            };
            // Skip adverbs the registry doesn't define (no answer consumed).
            let known = reg
                .get("adverbs")
                .and_then(|a| a.as_array())
                .map(|arr| arr.iter().any(|a| a.get("name").and_then(|n| n.as_str()) == Some(aname)))
                .unwrap_or(false);
            if !known {
                continue;
            }
            match dialog_pick() {
                Some(v) => {
                    adverbs.insert(aname.to_string(), json!(v));
                }
                None => continue,
            }
        }
    }
    let adverbs = Value::Object(adverbs);

    // 5. Confirm (a "no"/empty answer cancels).
    if !dialog_confirm() {
        return cancel();
    }

    // 6. Execute through the same path the CLI uses.
    exec_verb(&reg, &verb, &subject, &object, &adverbs)
}

fn cancel() -> i32 {
    eprintln!("compose: cancelled");
    130
}

/// Pop the next pre-seeded answer from the `GOO_COMPOSE_ANSWERS` queue (one per
/// line; an empty line = cancel → `None`). The CLI has no interactive picker.
fn dialog_pick() -> Option<String> {
    let file = std::env::var("GOO_COMPOSE_ANSWERS").ok()?;
    if !std::path::Path::new(&file).is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&file).unwrap_or_default();
    let mut lines: Vec<&str> = content.lines().collect();
    let ans = if lines.is_empty() { "" } else { lines.remove(0) };
    let rest = lines.join("\n");
    let rest = if rest.is_empty() { String::new() } else { format!("{rest}\n") };
    let _ = std::fs::write(&file, rest);
    if ans.is_empty() {
        None
    } else {
        Some(ans.to_string())
    }
}

/// yes/no via the answer queue.
fn dialog_confirm() -> bool {
    matches!(dialog_pick().as_deref(), Some("yes"))
}

// ---------------- aliases ----------------

fn alias_expansion(reg: &Value, name: &str) -> Option<String> {
    reg.get("aliases")?.as_array()?.iter().find_map(|a| {
        if a.get("name").and_then(|n| n.as_str()) == Some(name) {
            a.get("expands").and_then(|e| e.as_str()).filter(|s| !s.is_empty()).map(str::to_string)
        } else {
            None
        }
    })
}

// ---------------- verb invocation ----------------

fn cmd_verb(reg: &Value, args: &[String]) -> i32 {
    let verb_name = &args[0];
    let verb = match verbs::lookup(reg, verb_name, None) {
        Some(v) => v,
        None => return die(format!("unknown verb or subcommand: {verb_name} (try 'goo plugins')")),
    };

    // Capture piped stdin once (a TTY means interactive use — no piped subject).
    let stdin_text = if std::io::stdin().is_terminal() {
        String::new()
    } else {
        read_stdin()
    };

    // Parse remaining args: positionals + --flag[=val].
    let (positionals, adverbs) = parse_args(&args[1..]);
    warn_unknown_confirm_dangerous(reg, &adverbs);
    let subject_arg = positionals.first().cloned().unwrap_or_default();
    let object_arg = positionals.get(1).cloned().unwrap_or_default();

    let accepts_count = verb.get("accepts").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
    let subject = if accepts_count > 0 {
        match resolve_subject(reg, &verb, &subject_arg, &stdin_text) {
            Ok(Subject::Value(s)) => s,
            Ok(Subject::Exit(code)) => return code,
            Err(e) => return die(e),
        }
    } else if !subject_arg.is_empty() {
        // No accepts but a positional was given — treat as text content.
        json!({ "type": "text/plain", "text": subject_arg })
    } else {
        Value::Null
    };

    let has_object_type = verb.get("object_type").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).is_some();
    let object = if !object_arg.is_empty() || has_object_type {
        match verbs::resolve_object(reg, &verb, &object_arg, &subject) {
            Ok(o) => o,
            Err(e) => return die(e),
        }
    } else {
        Value::Null
    };

    exec_verb(reg, &verb, &subject, &object, &adverbs)
}

/// Read all of stdin, stripping trailing newlines (parity with bash `$(cat)`).
fn read_stdin() -> String {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).ok();
    s.trim_end_matches('\n').to_string()
}

/// Turn a positional (or the implicit subject chain) into a subject JSON.
/// Port of `resolve_subject` in `bin/goo`, plus slice-8 verb-aware inference.
/// Which implicit source supplied a no-positional subject — drives the run-time
/// transparency nudge. `Stdin` is an explicit pipe, so it stays silent.
#[derive(Clone, Copy)]
enum ImplicitSource {
    Stdin,
    Primary,
    Clipboard,
}

/// Narrate the implicit-subject fallback at run time: when a text verb borrows
/// its subject from the PRIMARY selection or the clipboard, say so on stderr —
/// the otherwise-silent fallback chain made visible. Interactive/GUI only
/// (script pipelines stay quiet) and suppressed by `GOO_INFER_NO_NUDGE`, reusing
/// the band-nudge knobs. Never changes resolution. This is the post-resolution
/// complement to the (still-open) §5.4 completion-time preview, not that preview.
fn maybe_emit_implicit_nudge(src: ImplicitSource, text: &str) {
    use inference::Context;
    let label = match src {
        ImplicitSource::Primary => "the PRIMARY selection",
        ImplicitSource::Clipboard => "the clipboard",
        ImplicitSource::Stdin => return, // explicit pipe — never narrated
    };
    let interactive = matches!(detect_inference_context(), Context::Interactive | Context::Gui);
    if !interactive || std::env::var("GOO_INFER_NO_NUDGE").is_ok() {
        return;
    }
    eprintln!(
        "goo: no subject given — using {label}: '{}'  (pass a subject to be explicit)",
        implicit_snippet(text)
    );
}

/// A one-line, ≤40-char preview of an implicit subject: inner whitespace
/// collapsed to single spaces, trimmed, ellipsis appended when truncated.
fn implicit_snippet(text: &str) -> String {
    const MAX: usize = 40;
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > MAX {
        let head: String = collapsed.chars().take(MAX).collect();
        format!("{head}…")
    } else {
        collapsed
    }
}

fn resolve_subject(reg: &Value, verb: &Value, positional: &str, stdin_text: &str) -> Result<Subject, String> {
    // 1. Explicit positional → the addressing resolver.
    if !positional.is_empty() && address::is_explicit(positional, reg) {
        return address::resolve(positional, reg, Some(verb)).map(Subject::Value);
    }

    // 1b. A bare positional that names an existing path → resolve it as that
    // file/dir. Filesystem reality breaks the bare-token ambiguity, so
    // `goo json-keys data.json` works without a leading `./`. A rare collision (a
    // file shadowing a source name) is disambiguated with an explicit sigil
    // (`:apps/x`) or forced to text with `+`.
    if !positional.is_empty() && std::path::Path::new(positional).exists() {
        return address::resolve(&format!("./{positional}"), reg, Some(verb)).map(Subject::Value);
    }

    // 2. Bare positional.
    if !positional.is_empty() {
        // 2a. Context-sensitive inference: a positive structural signal (JSON
        // shape) for a type the verb accepts wins ahead of everything below.
        if let Some(mt) = mime::infer_for(positional, verb, reg) {
            return Ok(Subject::Value(json!({ "type": mt, "text": positional })));
        }
        // 2b. Verb-aware entity inference (slice 8 / §3.4): score the bare token
        // across the sources this verb accepts; band+context decides the action.
        // Runs BEFORE the text fallback so an exact entity match (DEFINITIVE)
        // can win for a text/* verb (`summarize <recent-file>`) — but band
        // safety means no fuzzy guess fires silently, and a non-match falls
        // straight through to the text path below (unchanged behaviour).
        // TODO(polymorphic accepts): `verb` is one impl from verbs::lookup
        // (best for an untyped lookup). A polymorphic verb (`connect` over
        // ssh/bt/net) biases toward only that impl's `accepts`, not the union
        // §3.4 implies. Pre-existing — `handle_search` has the same limitation
        // — so it's a follow-up, not a slice-8 fix.
        if inference::is_inferable_shape(positional) {
            let ctx = detect_inference_context();
            if let Ok((subj, band, reason)) = inference::infer_entity_for_verb(positional, reg, ctx, verb) {
                match act_on_verb_inference(reg, verb, positional, &subj, band, &reason, ctx) {
                    VerbInfer::Use(s) => return Ok(Subject::Value(s)),
                    VerbInfer::Picker => return Ok(Subject::Exit(2)),
                    VerbInfer::FallThrough => {}
                }
            }
        }
        // 2c. Text content (verbs that accept text/*).
        if accepts_text(verb, reg) {
            let mt = mime::detect_content(positional);
            return Ok(Subject::Value(json!({ "type": mt, "text": positional })));
        }
        // 2d. Handle resolution (ungated first-match): a source emitting an
        // accepted type, item by id/title. This is the fallback that still
        // reaches the `enumerate = false` sources a verb accepts (`:bt`, `:file`)
        // — they're excluded from scored inference at 2b by the §3.6 privacy
        // gate, so without this `connect :bt-device` / `open file` would regress.
        if let Some(item) = handle_search(reg, verb, positional) {
            return Ok(Subject::Value(item));
        }
        return Err(format!(
            "could not resolve '{positional}' against any source for verb's accepted types"
        ));
    }

    // 3. No positional: implicit chain — stdin → selection → clipboard.
    // Structural inference on stdin first (parity-safe: stdin is already read,
    // and infer_for only fires on a positive signal the verb accepts).
    if !stdin_text.is_empty() {
        if let Some(mt) = mime::infer_for(stdin_text, verb, reg) {
            return Ok(Subject::Value(json!({ "type": mt, "text": stdin_text })));
        }
    }
    if accepts_text(verb, reg) {
        // Track which implicit source supplied the text so the otherwise-silent
        // fallback can be narrated at run time. Order is the documented chain:
        // stdin (explicit) → PRIMARY selection → clipboard.
        let (mut text, mut src) = (stdin_text.to_string(), ImplicitSource::Stdin);
        if text.is_empty() {
            text = selection::primary();
            src = ImplicitSource::Primary;
        }
        if text.is_empty() {
            text = selection::clipboard();
            src = ImplicitSource::Clipboard;
        }
        if !text.is_empty() {
            maybe_emit_implicit_nudge(src, &text);
            let mt = mime::detect_content(&text);
            return Ok(Subject::Value(json!({ "type": mt, "text": text })));
        }
    }
    // Non-text accepts: implicit=true sources emitting an accepted type, first item.
    if let Some(item) = implicit_source_item(reg, verb) {
        return Ok(Subject::Value(item));
    }
    Err("no subject provided and no implicit subject available (stdin/selection/clipboard/implicit-source all empty)".into())
}

/// Map a verb-aware inference result (band + context) to an action, mirroring
/// the noun-first `act_on_inference` so the two surfaces behave identically
/// (§3.5). The only difference: a resolved subject feeds the verb in progress
/// (we re-resolve it to a full subject), rather than dispatching a default verb.
#[allow(clippy::too_many_arguments)]
fn act_on_verb_inference(
    reg: &Value,
    verb: &Value,
    raw: &str,
    subject: &Value,
    band: inference::Band,
    reason: &inference::Reason,
    ctx: inference::Context,
) -> VerbInfer {
    use inference::{Band, Context};
    let interactive = matches!(ctx, Context::Interactive | Context::Gui);
    let nudge_suppressed = std::env::var("GOO_INFER_NO_NUDGE").is_ok();

    match band {
        Band::Definitive => VerbInfer::Use(reresolve_inferred(reg, verb, subject, reason)),
        Band::High if interactive => {
            if !nudge_suppressed {
                emit_nudge(raw, subject, reason);
            }
            VerbInfer::Use(reresolve_inferred(reg, verb, subject, reason))
        }
        Band::High => {
            // Script context: log and fall through (the safe, explicit failure
            // mode — the bare token becomes text or hits "could not resolve").
            if !nudge_suppressed {
                emit_script_nudge(raw, subject, reason);
            }
            VerbInfer::FallThrough
        }
        Band::Medium if interactive => {
            if !nudge_suppressed {
                // KNOWN LIMITATION: emit_medium_picker is shared with noun-first
                // and says "re-run with the explicit address". In verb-position
                // the user typed `goo <verb> <token>`, so the correct re-run is
                // `goo <verb> :prefix/id` (the bare address would do noun-first
                // default-verb dispatch instead). The addresses shown are right;
                // only the instruction line is verb-agnostic. Deferred with the
                // rest of the real interactive picker (post-v1).
                emit_medium_picker(raw, reason);
            }
            VerbInfer::Picker
        }
        Band::Medium | Band::Low => VerbInfer::FallThrough,
    }
}

/// Turn the inference winner into a FULL subject by re-resolving its canonical
/// `:<prefix>/<id>` address (the engine's `build_subject` is intentionally
/// lossy — id/title/type only — so a verb template referencing
/// `{subject.metadata.*}` needs the address layer's complete item). Falls back
/// to the lossy subject if the re-resolve fails. `reason.winner_source` is the
/// winning source's prefix (or name), which is the correct disambiguator when
/// several sources emit the same type.
fn reresolve_inferred(reg: &Value, verb: &Value, subject: &Value, reason: &inference::Reason) -> Value {
    let id = subject.get("id").and_then(Value::as_str).unwrap_or("");
    if !reason.winner_source.is_empty() && !id.is_empty() {
        let addr = format!(":{}/{}", reason.winner_source, id);
        if let Ok(full) = address::resolve(&addr, reg, Some(verb)) {
            return full;
        }
    }
    subject.clone()
}

/// Walk sources whose `emits` matches an accepted (handle) type; return the
/// first item whose id/title contains `query` (case-insensitive), tagged.
fn handle_search(reg: &Value, verb: &Value, query: &str) -> Option<Value> {
    let accepts = verb.get("accepts").and_then(|a| a.as_array())?;
    let sources = reg.get("sources").and_then(|s| s.as_array())?;
    let q = query.to_lowercase();
    for pat in accepts.iter().filter_map(|p| p.as_str()) {
        for source in sources {
            let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
            if emits.is_empty() || !mime::is_subtype(emits, pat, reg) {
                continue;
            }
            let lc = match source.get("list_cmd").and_then(|c| c.as_str()) {
                Some(lc) => lc,
                None => continue,
            };
            let items: Vec<Value> = serde_json::from_str(bash_capture(lc).trim()).unwrap_or_default();
            let found = items.iter().find(|it| address::fuzzy_matches(it, &q));
            if let Some(it) = found {
                let mut o = it.clone();
                if let Some(m) = o.as_object_mut() {
                    m.insert("type".into(), json!(emits));
                }
                return Some(o);
            }
        }
    }
    None
}

/// First item of the first implicit=true source emitting an accepted type.
fn implicit_source_item(reg: &Value, verb: &Value) -> Option<Value> {
    let accepts = verb.get("accepts").and_then(|a| a.as_array())?;
    let sources = reg.get("sources").and_then(|s| s.as_array())?;
    for pat in accepts.iter().filter_map(|p| p.as_str()) {
        for source in sources.iter().filter(|s| s.get("implicit") == Some(&json!(true))) {
            let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
            if emits.is_empty() || !mime::is_subtype(emits, pat, reg) {
                continue;
            }
            let lc = match source.get("list_cmd").and_then(|c| c.as_str()) {
                Some(lc) => lc,
                None => continue,
            };
            let items: Vec<Value> = serde_json::from_str(bash_capture(lc).trim()).unwrap_or_default();
            if let Some(it) = items.into_iter().find(|i| !i.is_null()) {
                let mut o = it;
                if let Some(m) = o.as_object_mut() {
                    m.insert("type".into(), json!(emits));
                }
                return Some(o);
            }
        }
    }
    None
}
