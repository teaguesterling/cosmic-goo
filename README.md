# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

**Status**: pre-alpha, Phase 1 in progress. Not yet runnable end-to-end.

## What it is

cosmic-goo adds noun → verb → object composition over the COSMIC desktop launcher, inspired by gnome-do / Quicksilver. The same plugin-driven backend powers:

- a CLI (`goo`)
- inline composition in `cosmic-launcher`
- an on-demand compose dialog

Plugins are TOML files declaring any combination of types, sources, verbs, and adverbs.

## Documentation

The current design lives in [`docs/vision/`](docs/vision/) as the working spec:

- [`cosmic-goo-README.md`](docs/vision/cosmic-goo-README.md) — full project introduction
- [`cosmic-goo-spec.md`](docs/vision/cosmic-goo-spec.md) — architecture and plugin format
- [`cosmic-goo-implementation-plan.md`](docs/vision/cosmic-goo-implementation-plan.md) — task-level Phase 1–6 breakdown
- [`cosmic-goo-CONTRIBUTING.md`](docs/vision/cosmic-goo-CONTRIBUTING.md) — plugin authoring & contribution guide
- [`cosmic-goo-design-history.md`](docs/vision/cosmic-goo-design-history.md) — archived predecessor design (Cosmic Scenes)

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
