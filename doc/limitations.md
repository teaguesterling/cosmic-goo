# Limitations and roadmap

## Current limitations

### `claude://` URL handler is flaky on Linux

The smoke test (R4 in `recon/findings.md`) found that `xdg-open "claude://claude.ai/new?q=..."` only reliably prefills the new-chat input on a **cold start** of Claude Desktop. Subsequent invocations may route to Cowork or fail to update the prompt input.

**Impact**: `goo critique --via=claude-desktop` / `--via=claude-code` may open Claude Desktop without the prompt populated, needing a manual paste.

**Workaround**: `--via=clipboard` is the reliable route — paste wherever you like.

**Planned fix**: investigate the `aaddrick/claude-desktop-debian` URL handler; possibly have `claude-routing` always pre-copy to clipboard as a side effect.

### Compose/launcher enumerate every cheap source's `list_cmd`

`goo compose` and bare-positional completion run the `list_cmd` of every source marked `enumerate = true`. Slow or huge sources are opted out with `enumerate = false` (bluetooth, files, services, repos, clipboard-history — reachable on demand via `:prefix:`). The remaining enumerable sources are run serially, not in parallel, so the subject picker's cold open is roughly the sum of `apps` + `workspaces` + `tmux` + `sinks` + `network` (~300ms here). Parallelizing them is a future optimization.

### `clipboard-history` needs session setup

`cliphist` only has data if (1) `COSMIC_DATA_CONTROL_ENABLED=1` is set (wlr-data-control) and (2) a `wl-paste --watch cliphist store` daemon runs in the session. Until then the source yields `[]` cleanly. See `plugins/clipboard-history.toml`.

### `cos-cli` PATH

`cos-cli` installs to `~/.cargo/bin`, which isn't on the non-interactive bash PATH on a clean Pop!_OS setup. `lib/selection.sh` and `plugins/apps.toml` fall back to `$HOME/.cargo/bin/cos-cli`; override with the `COS_CLI` env var for other prefixes.

### Inline launcher composition isn't built yet

The spec's `cosmic-launcher` inline grammar (typing a sentence with type-aware autocomplete) is the pop-launcher meta-plugin — not yet implemented. Today you compose via the CLI, the `bin/goo compose` picker dialog, or the early `goo-compose-gui` (iced). Note the CLI *does* understand `goo://` addressing — `:dom/id` (value), `:dom:query` (search), `+text`, `^`, native `./ ~/ https://`, and customizable sigils — see [cli-reference](cli-reference.md#subject-addressing).

## Resolved since the original plan

These were limitations in earlier drafts and are now fixed:

- **Raw template substitution** → solved by `{var|q}` / `{var|uri}` filters ([plugin-authoring](plugin-authoring.md#filters-making-substitutions-safe)).
- **`goo compose` was a stub** → a working picker-driven dialog in `bin/goo`; the Rust CLI is non-interactive (scripted), and the GUI is the new `goo-compose-gui` (iced v1 scaffold; libcosmic swap planned).
- **~370ms cold load** → a registry mtime cache (`$XDG_RUNTIME_DIR/cosmic-goo/registry.json`) makes warm loads ~10ms; cold load only recurs after a plugin edit.
- **Source scoping unimplemented** → goo:// addressing works in the CLI now (value/search/`?refine`).
- **Shell-only / no install** → the engine is now the **Rust `goo` binary** (bash stays the reference; `make install` / `make install-bash`); 235+-test bats suite passes on both engines.
- **Content-dispatch & canonical scheme** → `[[dispatch]]` + `goo dispatch` shipped; the **`goo://` scheme** is canonical (`goo://domain/path`, strict value/search), with **GOO default-verb dispatch** (`goo goo://…` runs the type's `default_for` verb).

## Roadmap

Done: the **Rust engine + CLI** (the default `goo`; bash is the reference), **24 plugins** (~82 verbs, 17 sources, incl. non-text handle domains), goo:// addressing + **GOO default-verb dispatch**, content-dispatch, completion, filters, command aliases, the compose dialog, and `goo-compose-gui` v1 (iced) scaffolding. Remaining:

- **`goo-compose-gui` build-out** — grow the iced dialog (verb pane → exec), then **swap to libcosmic** for the native COSMIC look (the bones port mostly mechanically).
- **pop-launcher meta-plugin** — inline `cosmic-launcher` composition with type-aware autocomplete, emitting canonical `goo://` URIs.
- **`good` daemon (#31)** — warm registry + the HTTP-shaped request protocol over a unix socket (`GET`/`OPTIONS`, `Using:`/`To:`/`With:`/`Log:`, channels). Gated on a consumer (the launcher).
- **type system, inference & coercion** — the next major arc: richer MIME modeling, shape+content inference, and `emits`≠`accepts` coercion (auto-route through a `{process}` channel) — what unlocks data-sink channels (SQL/S3/server). Designed-not-built.
- **scenes plugin**, fabric patterns as verbs, packaging, more bindings examples.

The addressing + request-protocol design is captured in [`doc/design/addressing-and-protocol.md`](design/addressing-and-protocol.md) (the goo:// URI layer — domains, capabilities, sigils) and [`doc/design/goo-protocol.md`](design/goo-protocol.md) (the request/wire layer — verbs, slots, params, OPTIONS); the daemon-era pieces are gated there. Full original plan: [`docs/vision/cosmic-goo-implementation-plan.md`](../docs/vision/cosmic-goo-implementation-plan.md).
