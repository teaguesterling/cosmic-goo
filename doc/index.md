# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

cosmic-goo adds noun → verb → object composition over the COSMIC desktop, inspired by gnome-do and Quicksilver. The same plugin-driven backend powers:

- a CLI (`goo`)
- inline composition in the COSMIC launcher (Phase 2)
- an on-demand compose dialog (Phase 4)

Plugins are TOML files declaring any combination of **types**, **sources**, **verbs**, and **adverbs**. The CLI is the canonical entry point — every UI surface eventually shells to it.

## Status

**Phase 1 functionally complete.** The library backbone, the CLI, four built-in plugins, and an integration test suite are in place. End-to-end on the developer host: `goo critique "text" --via=clipboard` puts a rendered prompt on the system clipboard; `goo activate Alacritty` finds and focuses the running app via `cos-cli`.

Not yet shipped: the pop-launcher meta-plugin (Phase 2), the scenes plugin (Phase 3), the compose dialog (Phase 4). See [`docs/vision/cosmic-goo-implementation-plan.md`](../docs/vision/cosmic-goo-implementation-plan.md) for the full plan.

## Quick taste

```
$ goo plugins
apps — Running applications via cos-cli
  plugins/apps.toml
claude-routing — Routes text verbs to Fabric, Claude Desktop, Claude Code, or clipboard
  plugins/claude-routing.toml
selection — Implicit subjects from the current PRIMARY selection and clipboard
  plugins/selection.toml
text-verbs — Selection-aware text actions (critique, summarize, think, draft-response)
  plugins/text-verbs.toml

$ goo critique "this paragraph could be tighter" --via=clipboard
$ wl-paste
You are providing expert review of the following passage.
Deduce the desired intent and tone, then critique accordingly.

---
this paragraph could be tighter
```

## Getting it running

cosmic-goo is shell-only in Phase 1. Clone the repo and you're done:

```bash
git clone <repo>
cd cosmic-goo
make test         # verify (85 tests, ~5 seconds)
bin/goo --help
```

Dependencies — apt-installable on Pop!_OS / Ubuntu:

```bash
sudo apt install -y jq yq wl-clipboard wev shellcheck bats
cargo install --git https://github.com/estin/cos-cli   # for the apps plugin
```

To use `goo` from anywhere, symlink `bin/goo` into your `$PATH`:

```bash
ln -s "$PWD/bin/goo" ~/.local/bin/goo
```

There is no system install yet (`make install` and `make install-user` are stubs — TBD in Phase 6).
