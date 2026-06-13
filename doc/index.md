# cosmic-goo

> **G**rammar **O**f **O**perations — a compositional sentence layer over the COSMIC desktop.

cosmic-goo lets you name a thing on your desktop — an app, a window, a file, a git repo, a
Bluetooth device, a workspace — and say what to do with it, in one uniform **noun → verb →
object** grammar (inspired by gnome-do and Quicksilver). **[Read the model](the-model.md)**
for the one-page idea.

The same plugin-driven backend powers several surfaces:

- the **CLI** (`goo`) — the canonical entry point; every other surface shells to it
- a **compose dialog** (`goo-compose-gui`) — a keyboard-first, gnome-do-style launcher
  that assembles the same `goo …` sentence live
- inline composition in the COSMIC launcher — *future* (a pop-launcher meta-plugin)

Plugins are TOML files declaring any combination of **types**, **sources**, **verbs**, and
**adverbs** — so new domains (a new kind of object, a new verb) are config, not code.

## Status

**The `goo` CLI is fully usable.** ~30 built-in plugins (~92 verbs, 21 sources, 19 types),
subject addressing with sigils + native file/URL detection, polymorphic verbs and on-demand
type coercion (`--explain` shows the route), `{var|q|uri}` template filters, bash tab
completion, an entity cache, noun-first dispatch (`goo do`), action history (`goo again`),
and the `goo-compose-gui` launcher — covered by **445 conformance tests + 251 engine unit
tests**.

```sh
goo activate :app/firefox       # focus an app
goo status :repo:cosmic-goo     # short git status of a repo (fuzzy-match the name)
goo move-to :app:alacritty :ws/0:1   # move an app to a workspace
goo summarize                   # summarise the current selection through a local model
goo calc "2+2*10"               # → 22
```

Not yet shipped: the pop-launcher meta-plugin for *inline* composition in
`cosmic-launcher`, the scenes plugin, and the libcosmic port of the compose GUI (the iced
version ships today). See
[`docs/vision/cosmic-goo-implementation-plan.md`](https://github.com/teaguesterling/cosmic-goo/blob/main/docs/vision/cosmic-goo-implementation-plan.md)
for the full plan.

**New here?** Read **[The model](the-model.md)**, then the **[Quickstart](quickstart.md)** —
goo's core moves (address → verb → coerce → route) in five minutes.

## Quick taste

Address any kind of thing, then act on it:

```sh
$ goo what :repo:cosmic-goo        # what can I do with this repo?
applicable verbs for :repo:cosmic-goo  (type: application/vnd.git.repo)
    status      Show short status
    pull        Pull the current branch
    gh-pr-list  List open GitHub PRs
    open        Open in its default application   # a repo is also a directory, so
    reveal      Open the containing folder        # it inherits file verbs too
    …

$ goo status :repo:cosmic-goo      # run one
## main...origin/main

$ goo do :app/firefox              # noun-first: name the thing, see its verbs
$ goo move-to :app/firefox :ws/0:1 # verb + subject + object
$ goo connect :bt/headphones       # one connect verb works across bt / ssh / net
```

Types coerce automatically — ask a JSON verb to run on a CSV and goo finds the route:

```sh
$ goo --explain json-keys data.csv
subject: text/csv (via libmagic)
text/csv → csv2json → application/json → (json-keys) → …
```

And text is just one more subject type — `goo summarize`, `goo upper`, `goo sha256`,
`goo critique --model=big` (text verbs borrow the selection/clipboard when given no subject).

## Getting it running

cosmic-goo's engine is the Rust `goo` binary (the original bash engine stays alongside as a
frozen reference, installable via `make install-bash`). The Rust bin still shells out to
`bash` + `jq` at runtime. Clone the repo and you're done:

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

`make install` puts the real binary at `$PREFIX/share/cosmic-goo/bin/goo-bin` behind a thin
launcher that points it at the installed plugins, and links `$PREFIX/bin/goo`. Use
`PREFIX=…` to relocate; `make uninstall` removes it.
