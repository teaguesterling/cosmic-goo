# Distribution: goo, goo-standalone, cosmic-goo

The engine is portable; COSMIC is just a set of plugins. The same codebase ships
as different *profiles* by selecting plugin **tiers** at install time — there are
no separate forks.

## The pieces

- **`goo`** — the engine: `bin/goo` + `lib/*.sh`. Zero COSMIC dependency
  (depends only on bash, jq, coreutils; `lib/selection.sh` adds wl-clipboard).
- **plugins** — the data layer, each tagged with a `tier`:

| tier | needs | plugins |
|---|---|---|
| **core** | bash + jq + coreutils (headless / SSH-friendly) | `text-utilities` `sigils` `calculator` `git` `tmux` |
| **desktop** | freedesktop / Wayland / PipeWire (any compositor) | `selection` `clipboard-history` `notifications` `media` `audio` `screenshots` `network` `bluetooth` `services` `urls` `claude-routing` `text-verbs` `files` `power` |
| **cosmic** | `cos-cli` / COSMIC | `apps` `workspaces` |

`goo validate` checks any declared `tier` is one of `core`/`desktop`/`cosmic`.
`make tiers` lists plugins grouped by tier.

## Profiles = install targets

```bash
make install-core     # engine + core tier — minimal, headless
make install          # engine + core + desktop — "goo-standalone"
make install-cosmic   # + cosmic tier — the full "cosmic-goo"
```

All default to a **user install** under `PREFIX=~/.local` (no root). The layout:

```
$PREFIX/share/cosmic-goo/{bin,lib,plugins}/   # engine + the selected plugin tiers
$PREFIX/bin/goo -> .../share/cosmic-goo/bin/goo
```

`bin/goo` resolves its own path (`readlink -f`) and finds `lib/`/`plugins/` as
siblings, so the symlink works and the installed `plugins/` dir (filtered to the
chosen tiers) is what loads. Override `PREFIX` for a system install
(`sudo make install-cosmic PREFIX=/usr/local`) or `TIERS` for a custom set.

`make uninstall` removes `$PREFIX/bin/goo` and `$PREFIX/share/cosmic-goo`.

## Picker portability (the dialog)

`goo compose` drives whatever picker is present — `fuzzel`/`rofi`/`wofi`/`fzf`,
with **`zenity`** as a GTK fallback. So the dialog works on a plain GTK desktop
("zenity-goo") with no wlroots picker; force one with `GOO_PICKER`.

## Naming note (packaging)

The everyday command is **`goo`**, but a Debian package named `goo` already
exists (a defunct programming language). For a `.deb`/distro package, ship the
artifact as **`cosmic-goo`** and expose `goo` via `update-alternatives` (opt-in,
no hard `Conflicts: goo`). See `doc/design/prior-art-and-architecture.md`.
