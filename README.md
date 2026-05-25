# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

**Status**: alpha. The `goo` CLI is fully usable — 21 plugins, ~74 verbs, subject addressing, tab completion, and a picker-driven compose dialog. Not yet built: the pop-launcher meta-plugin (inline composition) and the native libcosmic compose GUI.

## What it is

cosmic-goo adds noun → verb → object composition over the COSMIC desktop launcher, inspired by gnome-do / Quicksilver. The same plugin-driven backend powers:

- a CLI (`goo`)
- inline composition in `cosmic-launcher`
- an on-demand compose dialog

Plugins are TOML files declaring any combination of types, sources, verbs, and adverbs.

## Documentation

Authoritative current docs live in [`doc/`](doc/):

- [`doc/index.md`](doc/index.md) — what cosmic-goo is, status, install
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
bin/        Executable entry points (`goo` CLI; later `goo-compose`)
lib/        Shell utilities (plugin loader, type matcher, verb dispatch)
plugins/    Built-in TOML plugins
tests/      bats-core test suite
recon/      Environment reconnaissance scripts and findings
docs/       Documentation (currently `docs/vision/` only)
```

## License

To be determined. Likely Apache-2.0 or MIT to match the COSMIC ecosystem.
