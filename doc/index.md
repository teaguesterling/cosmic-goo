# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC launcher.

cosmic-goo adds noun → verb → object composition over the COSMIC desktop, inspired by gnome-do and Quicksilver. The same plugin-driven backend powers:

- a CLI (`goo`)
- inline composition in the COSMIC launcher (Phase 2)
- an on-demand compose dialog (Phase 4)

Plugins are TOML files declaring any combination of **types**, **sources**, **verbs**, and **adverbs**. The CLI is the canonical entry point — every UI surface eventually shells to it.

## Status

**The `goo` CLI is fully usable.** 24 built-in plugins (~82 verbs, 17 sources), subject addressing with sigils + native file/URL detection, `{var|q|uri}` template filters, bash tab completion, a registry cache, and a picker-driven compose dialog — covered by ~190 tests. E.g. `goo critique "text" --via=clipboard` renders a prompt onto the clipboard; `goo activate Alacritty` focuses the app; `goo calc "2+2*10"` → `22`; `goo qr-encode https://…` draws a QR in the terminal.

Not yet shipped: the pop-launcher meta-plugin for *inline* composition in `cosmic-launcher` (Phase 2), the scenes plugin (Phase 3), and the native libcosmic compose GUI (the current dialog is a shell-driven picker). See [`docs/vision/cosmic-goo-implementation-plan.md`](../docs/vision/cosmic-goo-implementation-plan.md) for the full plan and [`tutorial.md`](tutorial.md) to learn the CLI by example.

**New here? Start with the [quickstart](quickstart.md)** — goo's core values
(address → verb → coerce → route) in five minutes.

## Quick taste

```
$ goo plugins
apps — Running applications via cos-cli
  plugins/apps.toml
claude-routing — Routes text verbs to woollama (inference), Claude Desktop, Claude Code, or clipboard
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

cosmic-goo's engine is now the Rust `goo` binary (the original bash engine stays alongside as the reference, installable via `make install-bash`). The Rust bin still shells out to `bash` + `jq` at runtime. Clone the repo and you're done:

```bash
git clone <repo>
cd cosmic-goo
make test         # verify the bats suite
make build        # build the release binary (crates/target/release/goo)
```

Dependencies — apt-installable on Pop!_OS / Ubuntu:

```bash
sudo apt install -y jq yq wl-clipboard wev shellcheck bats
cargo install --git https://github.com/estin/cos-cli   # for the apps plugin
```

To install `goo` onto your `$PATH`:

```bash
make install            # builds + installs the Rust binary + plugins under ~/.local
make install-bash       # or install the bash engine instead (the reference)
```

`make install` puts the real binary at `$PREFIX/share/cosmic-goo/bin/goo-bin` behind a thin launcher that points it at the installed plugins, and links `$PREFIX/bin/goo`. Use `PREFIX=…` to relocate; `make uninstall` removes it.
