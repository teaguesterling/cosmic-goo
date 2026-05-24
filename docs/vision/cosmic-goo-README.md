# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

---

## What is this?

cosmic-goo extends the COSMIC desktop launcher (and is invocable from anywhere via a CLI) with **noun → verb → object** composition. Think gnome-do or Quicksilver, rebuilt for COSMIC's plugin model.

Instead of "launch app" or "open file" as opaque defaults, cosmic-goo lets you compose sentences like:

```
firefox move ws-2
dotfiles switch
/selected text/ critique --via=claude-desktop
:tmux project-x kill
```

The same machinery is reachable three ways:

- **CLI**: `goo critique --via=fabric` — scriptable, bindable to any key
- **Inline in COSMIC launcher**: type a sentence with type-aware autocomplete
- **Compose dialog**: a richer three-panel GUI, summoned on demand

cosmic-goo is **plugin-driven**. A single TOML file declares any combination of types, sources, verbs, and adverbs. Adding new integrations doesn't require touching the core.

---

## Status

**Pre-alpha. Not yet runnable.**

The architecture is specified ([`doc/architecture.md`](doc/architecture.md)), reconnaissance scripts exist for environment verification, and an implementation plan ([`doc/implementation-plan.md`](doc/implementation-plan.md)) is in place. Code is not yet written.

The project is being developed in the open with deliberate scope. The current target is Phase 1: a working CLI with three plugins covering all three execution paths (Fabric API, Claude Desktop URL handler, Claude Code).

---

## Why it exists

The COSMIC launcher (`pop-launcher` daemon + `cosmic-launcher` frontend) is excellent at flat search → activate, but lacks the compositional grammar that made gnome-do feel like magic in 2008–2012. cosmic-goo aims to be that grammar layer — not by replacing the launcher, but by registering as a meta-plugin and exposing the same composition via CLI.

Beyond the launcher: cosmic-goo's CLI lets you bind keys, write scripts, and integrate AI workflows (Fabric, Claude Desktop, Claude Code) with selection-aware actions.

---

## Inspiration & prior art

- [gnome-do](https://wiki.gnome.org/Apps(2f)Do.html) — the original Linux noun-verb launcher
- [Quicksilver](https://qsapp.com/) — the macOS forerunner
- [pop-os/launcher](https://github.com/pop-os/launcher) — the COSMIC launcher daemon and plugin protocol cosmic-goo extends
- [fabric](https://github.com/danielmiessler/fabric) — pattern-based LLM prompt library, used as one execution route
- [Daniel Miessler's pattern collection](https://github.com/danielmiessler/fabric/tree/main/patterns) — the source of many text-verb prompts

---

## Getting started

*Not yet — code doesn't exist. When it does, this section will cover:*

- Installation (cargo install / package / source build)
- Environment requirements (COSMIC, `wl-clipboard`, `cos-cli`, optional: `fabric`)
- First-run setup (`goo validate`, plugin discovery)
- Binding example keys (links to `doc/examples/`)

---

## Documentation

- [Architecture spec](doc/architecture.md) — the design, in detail
- [Plugin authoring](doc/plugin-authoring.md) — how to write a plugin (forthcoming)
- [CLI reference](doc/cli-reference.md) — full command reference (forthcoming)
- [Implementation plan](doc/implementation-plan.md) — what's being built, in what order

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). The most valuable contributions early on are:

1. **Plugin TOMLs** for tools you use
2. **Recon results** from your COSMIC environment (different machines, different shells)
3. **Bindings examples** for keyboards beyond the Microsoft Natural Ergonomic 4000

Code contributions welcome but the API may shift through Phase 1–4. Architectural feedback via issues is more useful than PRs right now.

---

## License

To be determined. Likely Apache-2.0 or MIT to match the COSMIC ecosystem. Track this in [#1](https://github.com/teaguesterling/cosmic-goo/issues/1) (when the repo exists).

---

## Etymology

GOO = Grammar Of Operations.

Also: the connective goo between your launcher and your tools. The thing that holds the system together when you press a key.

The Cosmic Girl (Jamiroquai) etymological branch was considered and reluctantly rejected.
