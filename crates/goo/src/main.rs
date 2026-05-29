//! `goo` — the cosmic-goo CLI. A thin orchestration layer over `goo-engine`,
//! ported from `bin/goo` (which stays canonical until this passes the bats
//! conformance suite). Subcommands and verb invocation assemble a subject /
//! object / adverbs, hand them to the engine to render a command, and exec it
//! via `bash -c` — exactly as the shell does.
//!
//! Plugins are found via env only (`COSMIC_GOO_BUILTIN_PLUGINS_DIR` and the
//! XDG dirs `registry::dirs()` reads); no path magic. Exit codes: 0 / 1
//! (catch-all) / 130 (cancel).

use goo_engine::{address, dispatch as disp, exec, mime, negotiation, registry, selection, verbs};
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
        Some("dispatch") => cmd_dispatch(args.get(1).map(String::as_str)),
        Some("__complete") => cmd_complete(args.get(1).map(String::as_str), args.get(2).map(String::as_str)),
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
            cmd_verb(&reg, args)
        }
    }
}

/// The `GOO` default verb: resolve `addr` to a subject, look up its type's
/// `default_for` verb, and run it (with any trailing `--adverbs` / object).
/// No applicable default → a clean error (the CLI analog of the protocol's
/// 415/300). Never guesses; only `default_for` verbs run this way.
fn cmd_goo(reg: &Value, addr: &str, rest: &[String]) -> i32 {
    let subject = match address::resolve(addr, reg, None) {
        Ok(s) => s,
        Err(e) => return die(e.replace("address: ", "")),
    };
    let type_ = subject.get("type").and_then(|t| t.as_str()).unwrap_or("text/plain");
    let verb = match verbs::default_for(reg, type_) {
        Some(v) => v,
        None => return die(format!("no default verb for type '{type_}'")),
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

fn print_usage() {
    print!(
        "goo — Grammar Of Operations CLI

USAGE
    goo <verb> [POSITIONAL...] [--FLAG=VALUE]
    goo <address>                        Resolve the address, run its type's default verb (GOO)
    goo list <source>                    Emit source items as JSON
    goo describe <verb>                  Show verb details
    goo dispatch <input>                 Classify content and route to a verb
    goo compose                          Build a sentence (scripted via GOO_COMPOSE_ANSWERS)
    goo plugins                          List loaded plugins
    goo validate                         Validate all loaded plugins
    goo <verb> … [--using CHANNEL]       --using pins the channel that performs a verb
    goo <verb> … [--to DEST | -o FILE]   route the result to a file / clipboard (^) instead of stdout
    goo --explain <verb> [@TYPE|subj]    Show the negotiation plan (route/415) — read-only
                                         [--as TYPE] [--using CHANNEL] [--explain-env tty|cosmic|desktop|piped]

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

use goo_engine::shell::{bash_capture, bash_exec};

/// `goo --explain VERB [SUBJECT|@TYPE] [--as TYPE] [--explain-env ENV]` — the
/// negotiation plan explainer (goo-debug). Read-only: shows the Accept profile
/// and the planned route (or a 415), never runs anything. `@<mime>` asserts the
/// subject type virtually (no file needed); `--explain-env tty|cosmic|desktop|
/// piped` overrides the detected environment (default: isatty + $WAYLAND_DISPLAY).
fn cmd_explain(args: &[String]) -> i32 {
    let reg = registry::load_all();
    let (mut verb_name, mut subj, mut type_override, mut as_type, mut env_ovr, mut using) =
        (None, None, None, None, None, None);
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
        } else if let Some(v) = a.strip_prefix("--explain-env=") {
            env_ovr = Some(v);
        } else if a == "--explain-env" {
            env_ovr = args.get(i + 1).map(String::as_str);
            i += 1;
        } else if let Some(t) = a.strip_prefix('@') {
            type_override = Some(t); // @<mime> — assert the subject type virtually
        } else if verb_name.is_none() {
            verb_name = Some(a);
        } else if subj.is_none() {
            subj = Some(a);
        }
        i += 1;
    }

    let verb_name = match verb_name {
        Some(v) => v,
        None => return die("explain: usage: goo --explain VERB [@TYPE|subject] [--as TYPE] [--explain-env tty|cosmic|desktop|piped]"),
    };
    let verb = match reg["verbs"].as_array().and_then(|a| a.iter().find(|v| v["name"].as_str() == Some(verb_name))) {
        Some(v) => v.clone(),
        None => return die(format!("explain: unknown verb '{verb_name}'")),
    };

    // Type the subject the SAME WAY the run does — via the detection signals —
    // and record which fired, so the annotation is truthful (not detect_path /
    // detect_content directly, which would bypass the extension signal + checkers).
    let (subject_type, type_source): (String, &str) = if let Some(t) = type_override {
        (t.to_string(), "explicit")
    } else if let Some(s) = subj {
        if std::path::Path::new(s).exists() {
            address::type_for_path(s, &reg)
                .unwrap_or_else(|_| ("application/octet-stream".into(), "libmagic"))
        } else if let Some((t, src)) = mime::infer_for_with_source(s, &verb, &reg) {
            (t, src)
        } else {
            (mime::detect_content(s), "content")
        }
    } else {
        return die("explain: needs a subject — e.g. `@image/png` or a file path");
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
    match negotiation::plan_request_using(&subject_type, &verb, &target, &reg, &all_tools, using) {
        None => {
            println!("415 · no route — {subject_type} can't be presented here (verb: {verb_name})");
            1
        }
        Some(plan) => {
            let start = plan.steps.first().map(|s| s.from.clone()).unwrap_or_else(|| subject_type.clone());
            let mut line = start;
            for s in &plan.steps {
                match &s.kind {
                    negotiation::StepKind::Convert(name) => {
                        line.push_str(&format!(" →[{name}: {}]→ {}", channel_tier(&reg, name), s.to));
                    }
                    negotiation::StepKind::Verb(inst) => {
                        let label = if inst.is_empty() { verb_name } else { inst.as_str() };
                        line.push_str(&format!(" →({label})→ {}", s.to));
                    }
                }
            }
            println!("{line}   (cost {})", plan.cost);
            0
        }
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
        let disp = std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
            || std::env::var("DISPLAY").is_ok_and(|v| !v.is_empty());
        (std::io::stdout().is_terminal(), disp)
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

    let (available, missing) = channel_tools(reg);
    match negotiation::plan_request_using(subject_type, verb, &target, reg, &available, using) {
        None => {
            // No route with the installed tools. If a route *would* exist with
            // everything installed, name the missing tools *on that route* — so
            // the hint is actionable ("install: mlr"), not every uninstalled tool.
            let hint = route_missing_tools_hint(subject_type, verb, &target, reg, &missing);
            die(format!("415 · no route — can't route {subject_type} through '{verb_name}'{hint}"))
        }
        Some(plan) => match dest {
            None => match exec::execute(&plan, &subject_path, verb, reg) {
                Ok(code) => code,
                Err(e) => die(format!("{verb_name}: {e}")),
            },
            Some(d) => match exec::execute_capture(&plan, &subject_path, verb, reg) {
                Ok(out) => route_result(d, out.as_bytes(), reg),
                Err(e) => die(format!("{verb_name}: {e}")),
            },
        },
    }
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

/// For a 415 where tool-pruning may be the cause: re-plan tool-agnostically (as
/// if everything were installed); if a route exists, return " — install: X, Y"
/// naming the missing tools *on that route*. Empty if no route exists regardless.
fn route_missing_tools_hint(subject_type: &str, verb: &Value, target: &negotiation::Target, reg: &Value, missing: &[String]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let all: Vec<String> = {
        let (a, m) = channel_tools(reg);
        a.into_iter().chain(m).collect()
    };
    let Some(ideal) = negotiation::plan_request(subject_type, verb, target, reg, &all) else {
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
        eprintln!("About to run: {}", rendered.cmd);
        let mut ans = String::new();
        std::io::stdin().read_line(&mut ans).ok();
        match ans.trim() {
            "y" | "Y" | "yes" | "YES" => {}
            _ => {
                eprintln!("goo: verb_apply: cancelled");
                return 130;
            }
        }
    }
    // No `--to` → run with inherited stdout (byte-identical to before). With
    // `--to`/`-o`, capture the result and route it to the destination instead.
    match adverbs.get("to").and_then(|v| v.as_str()) {
        None => bash_exec(&rendered.cmd),
        Some(d) => route_result(d, bash_capture(&rendered.cmd).as_bytes(), reg),
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
    let verb = match verbs::lookup(&reg, name, None) {
        Some(v) => v,
        None => return die(format!("describe: no verb named '{name}'")),
    };
    let s = |k: &str| verb.get(k).and_then(|v| v.as_str());
    let mut out = format!("verb: {}", s("name").unwrap_or(""));
    if let Some(d) = s("description") {
        out += &format!("\ndescription: {d}");
    }
    let accepts = verb
        .get("accepts")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    out += &format!("\naccepts: {accepts}");
    if let Some(ot) = s("object_type") {
        out += &format!("\nobject_type: {ot}");
    }
    match verb.get("default_for") {
        Some(Value::Array(a)) => {
            let joined = a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
            out += &format!("\ndefault_for: {joined}");
        }
        Some(Value::String(d)) => out += &format!("\ndefault_for: {d}"),
        _ => {}
    }
    if let Some(ua) = verb.get("uses_adverbs").and_then(|u| u.as_array()) {
        let joined = ua.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ");
        out += &format!("\nuses_adverbs: {joined}");
    }
    if let Some(cmd) = s("cmd") {
        out += &format!("\ncmd: {cmd}");
    }
    if let Some(prompt) = s("prompt") {
        out += &format!("\nprompt:\n  {}", prompt.replace('\n', "\n  "));
    }
    if verb.get("confirm").and_then(|c| c.as_bool()) == Some(true) {
        out += "\nconfirm: true";
    }
    out += &format!("\nprovided by plugin: {}", s("_plugin").unwrap_or(""));
    println!("{out}");
    0
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
        "compose", "list", "describe", "plugins", "validate", "dispatch", "__complete", "help",
        "-h", "--help",
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

fn cmd_complete(stage: Option<&str>, arg: Option<&str>) -> i32 {
    let reg = registry::load_all();
    let arr = |k: &str| reg.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let arg = arg.unwrap_or("");
    match stage.unwrap_or("") {
        "subcommands" => {
            println!("list\ndescribe\nplugins\nvalidate\ncompose\ndispatch\nhelp");
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
        "verbs" => print_names(&arr("verbs"), "name"),
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
        "verb-subject-items" => {
            if arg.is_empty() {
                return 0;
            }
            let verb = arr("verbs").into_iter().find(|v| v.get("name").and_then(|n| n.as_str()) == Some(arg));
            if let Some(verb) = verb {
                let accepts: Vec<String> = verb
                    .get("accepts")
                    .and_then(|a| a.as_array())
                    .map(|arr| arr.iter().filter_map(|p| p.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                for pattern in &accepts {
                    for source in arr("sources").iter().filter(|s| s.get("enumerate") != Some(&json!(false))) {
                        let emits = source.get("emits").and_then(|e| e.as_str()).unwrap_or("");
                        if emits.is_empty() || !mime::is_subtype(emits, pattern, &reg) {
                            continue;
                        }
                        if let Some(lc) = source.get("list_cmd").and_then(|c| c.as_str()) {
                            print_ids(&bash_capture(lc));
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
    let subject_arg = positionals.first().cloned().unwrap_or_default();
    let object_arg = positionals.get(1).cloned().unwrap_or_default();

    let accepts_count = verb.get("accepts").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
    let subject = if accepts_count > 0 {
        match resolve_subject(reg, &verb, &subject_arg, &stdin_text) {
            Ok(s) => s,
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
/// Port of `resolve_subject` in `bin/goo`.
fn resolve_subject(reg: &Value, verb: &Value, positional: &str, stdin_text: &str) -> Result<Value, String> {
    // 1. Explicit positional → the addressing resolver.
    if !positional.is_empty() && address::is_explicit(positional, reg) {
        return address::resolve(positional, reg, Some(verb));
    }

    // 1b. A bare positional that names an existing path → resolve it as that
    // file/dir. Filesystem reality breaks the bare-token ambiguity, so
    // `goo json-keys data.json` works without a leading `./`. A rare collision (a
    // file shadowing a source name) is disambiguated with an explicit sigil
    // (`:apps/x`) or forced to text with `+`.
    if !positional.is_empty() && std::path::Path::new(positional).exists() {
        return address::resolve(&format!("./{positional}"), reg, Some(verb));
    }

    // 2. Bare positional.
    if !positional.is_empty() {
        // Context-sensitive inference: a positive structural signal (JSON shape)
        // for a type the verb accepts wins ahead of the text/handle fallbacks.
        // Returns None for unstructured content, so the text path below is
        // reached exactly as before.
        if let Some(mt) = mime::infer_for(positional, verb, reg) {
            return Ok(json!({ "type": mt, "text": positional }));
        }
        if accepts_text(verb, reg) {
            let mt = mime::detect_content(positional);
            return Ok(json!({ "type": mt, "text": positional }));
        }
        // Handle resolution: a source emitting an accepted type, item by id/title.
        if let Some(item) = handle_search(reg, verb, positional) {
            return Ok(item);
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
            return Ok(json!({ "type": mt, "text": stdin_text }));
        }
    }
    if accepts_text(verb, reg) {
        let mut text = stdin_text.to_string();
        if text.is_empty() {
            text = selection::primary();
        }
        if text.is_empty() {
            text = selection::clipboard();
        }
        if !text.is_empty() {
            let mt = mime::detect_content(&text);
            return Ok(json!({ "type": mt, "text": text }));
        }
    }
    // Non-text accepts: implicit=true sources emitting an accepted type, first item.
    if let Some(item) = implicit_source_item(reg, verb) {
        return Ok(item);
    }
    Err("no subject provided and no implicit subject available (stdin/selection/clipboard/implicit-source all empty)".into())
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
