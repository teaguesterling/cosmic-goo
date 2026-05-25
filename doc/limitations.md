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

The spec's `cosmic-launcher` inline grammar (typing a sentence with type-aware autocomplete) is the pop-launcher meta-plugin — not yet implemented. Today you compose via the CLI or the `goo compose` picker dialog. Note the CLI *does* understand the addressing sigils (`:source:`, `+scheme:`, `^`, customizable) — see [cli-reference](cli-reference.md#subject-addressing).

## Resolved since the original plan

These were limitations in earlier drafts and are now fixed:

- **Raw template substitution** → solved by `{var|q}` / `{var|uri}` filters ([plugin-authoring](plugin-authoring.md#filters-making-substitutions-safe)).
- **`goo compose` was a stub** → now a working picker-driven dialog (fuzzel/rofi/wofi/fzf). The *native libcosmic GUI* is still future polish.
- **~370ms cold load** → a registry mtime cache (`$XDG_RUNTIME_DIR/cosmic-goo/registry.json`) makes warm loads ~10ms; cold load only recurs after a plugin edit.
- **Source scoping unimplemented** → `:source:query` addressing works in the CLI now (Phase 2 reuses it for the launcher).

## Roadmap

The CLI, 21 plugins, addressing, completion, filters, and the compose dialog are done. Remaining, roughly in spec-phase order:

- **pop-launcher meta-plugin** — inline `cosmic-launcher` composition with type-aware autocomplete, emitting the canonical `cosmic-goo:` URIs the CLI already understands.
- **scenes plugin** — workspace/app/tmux/cwd "scenes": anchors (browser, mail, Claude Desktop) and favorite slots.
- **native compose dialog** — a libcosmic/iced three-panel GUI replacing the shell picker; sub-100ms wake, possibly a `goo-composed` daemon.
- **command aliases** — user-defined `@g`/`%x`-style verb+adverb shortcuts (the configuration ratchet).
- **broadening & polish** — `fabric` patterns as verbs, `content-dispatch` via `sitting_duck`, packaging, more bindings examples.

Full task-level breakdown: [`docs/vision/cosmic-goo-implementation-plan.md`](../docs/vision/cosmic-goo-implementation-plan.md).
