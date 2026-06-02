#!/usr/bin/env python3
"""Regenerate the registry-derived parts of cosmic-goo-landscape.html.

The landscape page is part hand-written editorial (the grammar tables, the
engine-features grid, the roadmap, the composer subject chips) and part
machine-derived from the live plugin registry (the plugin / verb / source
catalogue and the headline counts). This script regenerates ONLY the latter,
in place, between `@gen:…` / `@gen:end` markers — the editorial prose is never
touched.

What it regenerates:
  - @gen:stats    the headline stat chips
  - @gen:registry the DATA.plugins / .tiers / .verbs / .sources arrays
  - @gen:footer   the "generated <date> … (N plugins · …)" footer line

Counts: plugin/verb/source/type/channel counts come from the registry dump.
The bats count comes from `bats --count` (no run). The engine-unit-test count
and the date are derived too, with graceful fallback to the values already in
the file if the tool isn't available — so a run never invents a wrong number.

Usage:
    tools/gen-landscape.py            # regenerate in place
    tools/gen-landscape.py --check    # exit 1 if regeneration would change the file
    tools/gen-landscape.py --units N  # override the engine-unit-test count
"""
import argparse, datetime, json, os, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HTML = ROOT / "cosmic-goo-landscape.html"


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def load_registry():
    """Build + run the dump_registry example against the in-tree plugins."""
    env = dict(os.environ, COSMIC_GOO_BUILTIN_PLUGINS_DIR=str(ROOT / "plugins"))
    r = run(["cargo", "run", "-q", "--example", "dump_registry"], cwd=ROOT / "crates", env=env)
    if r.returncode != 0:
        sys.exit(f"dump_registry failed:\n{r.stderr}")
    return json.loads(r.stdout)


def bats_count(fallback):
    r = run(["bats", "--count", "-r", "tests"], cwd=ROOT)
    if r.returncode == 0 and r.stdout.strip().isdigit():
        return int(r.stdout.strip())
    return fallback


def unit_count(fallback):
    """Count engine lib tests without running them (`cargo test --lib --list`)."""
    r = run(["cargo", "test", "--lib", "--", "--list"], cwd=ROOT / "crates")
    if r.returncode == 0:
        n = len(re.findall(r"^\S.*: test$", r.stdout, re.MULTILINE))
        if n:
            return n
    return fallback


def js(v):
    # Compact (no space after commas) to match the array style in the file.
    return json.dumps(v, ensure_ascii=False, separators=(",", ":"))


def gen_registry_block(reg):
    # `or` coalesces a null/missing description or tier to a sane default — the
    # embedded `core` builtin carries null for both.
    def desc(p):
        return p.get("description") or ""

    def tier(p):
        return p.get("tier") or "core"

    plugins = reg["plugins"]
    out = ["  plugins:{"]
    out += [f"    {js(p['name'])}:{js(desc(p))}," for p in plugins[:-1]]
    out += [f"    {js(plugins[-1]['name'])}:{js(desc(plugins[-1]))}"]
    out += ["  },", "  tiers:{"]
    out += [f"    {js(p['name'])}:{js(tier(p))}," for p in plugins[:-1]]
    out += [f"    {js(plugins[-1]['name'])}:{js(tier(plugins[-1]))}"]
    out += ["  },"]

    out += ["  // verbs: [name, plugin, accepts[], object_type|null, default_for|null, desc, two-step, valid_when]",
            "  verbs:["]
    vrows = []
    for v in reg["verbs"]:
        tup = [v["name"], v["_plugin"], v.get("accepts", []), v.get("object_type"),
               v.get("default_for"), v.get("description", ""),
               1 if v.get("object_type") else 0, 1 if v.get("valid_when") else 0]
        vrows.append("[" + ",".join(js(x) for x in tup) + "]")
    out.append(",\n".join(vrows))
    out += ["  ],",
            "  // sources: [name, plugin, prefix|null, emits, enumerate(0/1), implicit(0/1)]",
            "  sources:["]
    srows = []
    for s in reg["sources"]:
        tup = [s["name"], s["_plugin"], s.get("prefix"), s.get("emits", ""),
               0 if s.get("enumerate") is False else 1, 1 if s.get("implicit") is True else 0]
        srows.append("[" + ",".join(js(x) for x in tup) + "]")
    out.append(",\n".join(srows))
    out += ["  ],"]
    return "\n".join(out)


def gen_stats(reg, bats, units):
    pairs = [
        (len(reg["plugins"]), "plugins"),
        (len(reg["verbs"]), "verbs"),
        (len(reg["sources"]), "sources"),
        (len(reg["types"]), "types"),
        (len(reg["channels"]), "channels"),
        (bats, "bats conformance"),
        (units, "engine unit tests"),
    ]
    return "  stats:[" + ",".join(f'["{n}","{l}"]' for n, l in pairs) + "],"


def gen_footer(reg, bats, date):
    counts = (f"{len(reg['plugins'])} plugins · {len(reg['verbs'])} verbs · {len(reg['sources'])} sources · "
              f"{len(reg['types'])} types · {len(reg['channels'])} channels · {bats} tests")
    return (f"  cosmic-goo landscape map · generated {date} from the live registry\n"
            f"  ({counts}). Canonical docs in <code>doc/</code>;")


def warn_orphan_subjects(html, reg):
    """The composer `subjects` array is editorial (outside the markers), but its
    mime strings must match real verb `accepts` or the chip shows no verbs.
    Warn — don't fail — if a fresh registry leaves a subject orphaned."""
    m = re.search(r"subjects:\[(.*?)\n  \],", html, re.DOTALL)
    if not m:
        return
    accepts = [a for v in reg["verbs"] for a in v.get("accepts", [])]

    def matches(mime):
        return any(p == mime or p in ("*/*", "*") or
                   (p.endswith("/*") and mime.split("/")[0] == p.split("/")[0]) for p in accepts)

    for label, mime in re.findall(r'\["([^"]*)","([^"]*)"', m.group(1)):
        if mime and not matches(mime):
            print(f"  warning: subject chip {label!r} ({mime}) matches no verb's accepts", file=sys.stderr)


def splice(html, start_marker, end_marker, body):
    pat = re.compile(re.escape(start_marker) + r"[^\n]*\n.*?\n(\s*)" + re.escape(end_marker), re.DOTALL)
    if not pat.search(html):
        sys.exit(f"marker {start_marker!r} … {end_marker!r} not found in {HTML.name}")
    # Preserve the start-marker line verbatim; replace only the body before end.
    def repl(m):
        start_line = re.match(re.escape(start_marker) + r"[^\n]*", m.group(0)).group(0)
        return f"{start_line}\n{body}\n{m.group(1)}{end_marker}"
    return pat.sub(repl, html, count=1)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="exit 1 if regeneration would change the file")
    ap.add_argument("--units", type=int, default=None, help="engine-unit-test count (default: auto / preserve)")
    ap.add_argument("--date", default=None, help="generated-on date (default: today)")
    args = ap.parse_args()

    original = HTML.read_text()
    reg = load_registry()

    # Preserve current counts/date as fallbacks so a run never invents a wrong
    # value. The date is PRESERVED by default (not stamped to today): that keeps
    # `--check` measuring registry drift only, not the calendar — a CI guard
    # that failed daily regardless of drift would just get ignored. `make
    # landscape` passes an explicit `--date` to stamp a real regeneration.
    cur_units = int(m.group(1)) if (m := re.search(r'\["(\d+)","engine unit tests"\]', original)) else 0
    cur_bats = int(m.group(1)) if (m := re.search(r'\["(\d+)","bats conformance"\]', original)) else 0
    cur_date = m.group(1) if (m := re.search(r"generated (\d{4}-\d{2}-\d{2}) from", original)) else None
    bats = bats_count(cur_bats)
    units = args.units if args.units is not None else unit_count(cur_units)
    date = args.date or cur_date or datetime.date.today().isoformat()

    warn_orphan_subjects(original, reg)

    html = original
    html = splice(html, "// @gen:stats", "// @gen:end", gen_stats(reg, bats, units))
    html = splice(html, "// @gen:registry", "// @gen:end", gen_registry_block(reg))
    html = splice(html, "<!-- @gen:footer -->", "<!-- @gen:end -->", gen_footer(reg, bats, date))

    if args.check:
        if html != original:
            sys.exit("cosmic-goo-landscape.html is out of date — run tools/gen-landscape.py")
        print("landscape is up to date")
        return

    HTML.write_text(html)
    n_poly = sum(1 for n in {v["name"] for v in reg["verbs"]}
                 if sum(1 for v in reg["verbs"] if v["name"] == n) > 1)
    print(f"regenerated {HTML.name}: {len(reg['plugins'])} plugins · {len(reg['verbs'])} verbs "
          f"({n_poly} polymorphic) · {len(reg['sources'])} sources · {bats} bats · {units} units · {date}")


if __name__ == "__main__":
    main()
