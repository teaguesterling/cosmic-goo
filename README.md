# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

**Status**: alpha. The `goo` CLI is fully usable — 28+ plugins, ~90 verbs, subject addressing, tab completion, negotiation/coercion, OPTIONS discovery (`goo options`), the `--explain` planner debugger, and polymorphic verbs across plugins (Rust engine). Not yet built: the pop-launcher meta-plugin (inline composition) and the native libcosmic compose GUI.

**Engines.** The Rust engine (`crates/goo`) is the **canonical** goo and the conformance target for `make test`. The bash bin (`bin/goo` + `lib/*.sh`) is a **legacy reference**, feature-frozen at pre-negotiation behavior — kept as a readable spec and a no-rust install path (`make install-bash`); new features land Rust-only.

## What it is

cosmic-goo adds noun → verb → object composition over the COSMIC desktop launcher, inspired by gnome-do / Quicksilver. The same plugin-driven backend powers:

- a CLI (`goo`)
- inline composition in `cosmic-launcher`
- an on-demand compose dialog

Plugins are TOML files declaring any combination of types, sources, verbs, and adverbs.

## Documentation

Authoritative current docs live in [`doc/`](doc/):

- [`doc/quickstart.md`](doc/quickstart.md) — **start here**: goo's core values (address → verb → coerce → route) in 5 minutes
- [`doc/index.md`](doc/index.md) — what cosmic-goo is, status, install
- [`doc/tutorial.md`](doc/tutorial.md) — learn the CLI by example, in depth
- [`doc/cli-reference.md`](doc/cli-reference.md) — `goo` command reference
- [`doc/plugin-authoring.md`](doc/plugin-authoring.md) — how to write plugins
- [`doc/examples/ms-natural-4000-bindings.md`](doc/examples/ms-natural-4000-bindings.md) — example MS Natural 4000 bindings
- [`doc/limitations.md`](doc/limitations.md) — Phase 1 limitations and roadmap

Browse locally with live reload (Material theme):

```bash
make docs-install   # one-time, prints install hints
make serve          # http://127.0.0.1:8000/
```

Configured for [Read the Docs](https://about.readthedocs.com/) via [`.readthedocs.yaml`](.readthedocs.yaml) — connecting the repo on RTD will auto-build from `mkdocs.yml`.

Original design notes and the implementation plan are in [`docs/vision/`](docs/vision/) (frozen archive — `doc/` supersedes for current behaviour).

Recon results from environment validation are in [`recon/findings.md`](recon/findings.md).

## Layout

```
crates/     Rust engine + bin (canonical) — goo-engine, goo, goo-compose-gui
bin/        Legacy bash CLI (`goo`) — feature-frozen reference
lib/        Legacy bash utilities (plugin loader, address, verbs, …)
plugins/    Built-in TOML plugins (consumed by both engines)
tests/      bats-core test suite — Rust by default (`make test`)
completions/ Shell completion scripts
doc/        Current documentation
docs/       Frozen design archive (`docs/vision/`)
recon/      Environment reconnaissance scripts and findings
```

## License

To be determined. Likely Apache-2.0 or MIT to match the COSMIC ecosystem.
