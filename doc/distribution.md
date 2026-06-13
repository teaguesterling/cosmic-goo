# Distribution: goo, goo-standalone, cosmic-goo

The engine is portable; COSMIC is just a set of plugins. The same codebase ships
as different *profiles* by selecting plugin **tiers** at install time — there are
no separate forks.

## The pieces

- **`goo`** — the engine. The canonical engine is the **Rust `goo` binary**
  (`crates/`); the original bash engine (`bin/goo` + `lib/*.sh`) stays alongside as
  a **frozen reference**, installable via `make install-bash`. The Rust bin still
  shells to `bash` + `jq` at runtime, so the runtime deps are the same. The engine
  has zero COSMIC dependency — COSMIC is just the `cosmic` plugin tier.
- **plugins** — the data layer, each tagged with a `tier`. **30 plugins ship**
  (29 TOML files + the embedded `core` declarations). The complete tiering:

| tier | needs | plugins |
|---|---|---|
| **core** | bash + jq + coreutils (headless / SSH-friendly) | `calculator` `containers` `content` `git` `processes` `sigils` `ssh-hosts` `text-utilities` `tmux` `core`¹ |
| **desktop** | freedesktop / Wayland / PipeWire (any compositor) | `audio` `bluetooth` `claude-routing` `clipboard-history` `emoji` `files` `media` `mounts` `network` `notifications` `power` `presentation` `recent` `screenshots` `selection` `services` `text-verbs` `urls` |
| **cosmic** | `cos-cli` / COSMIC | `apps` `workspaces` |

¹ `core` is embedded in the binary (`include_str!`'d, seeded before discovered
plugins) — it's always present and not install-selectable; the other 29 are TOML
files filtered by tier at install time.

`goo validate` checks any declared `tier` is one of `core`/`desktop`/`cosmic`.
`make tiers` lists plugins grouped by tier. A custom plugin validates against
[`schema/cosmic-goo-plugin.schema.json`](https://github.com/teaguesterling/cosmic-goo/blob/main/schema/cosmic-goo-plugin.schema.json)
(authoring-time) and `goo validate` (load-time).

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
