# Phase 1 limitations and roadmap

## Known limitations

### Template substitution is raw

The dispatcher inserts `{var}` substitutions verbatim into the bash command, with no automatic shell-escaping or URL-encoding. Template authors are responsible for safety — see [plugin-authoring](plugin-authoring.md#phase-1-limitation-raw-substitution).

**Workarounds**: use single-quoted here-strings (`cmd <<< '{var}'`) for multi-line content; inline `jq -srR @uri` for URL-encoded query strings.

**Planned fix**: a filter syntax `{var|q}` (shell-escape), `{var|uri}` (URL-encode), `{var|raw}` (current behaviour, but explicit).

### `claude://` URL handler is flaky on Linux

The smoke test (R4 in `recon/findings.md`) found that `xdg-open "claude://claude.ai/new?q=..."` only reliably prefills the new-chat input on a **cold start** of Claude Desktop. Subsequent invocations may route to Cowork or fail to update the prompt input.

**Impact**: `goo critique --via=claude-desktop` and `goo critique --via=claude-code` may visibly open Claude Desktop without the prompt populated, requiring a manual paste.

**Workaround**: use `--via=clipboard` as a fallback and paste manually. The clipboard route is the most reliable Phase 1 path.

**Planned fix**: investigate `aaddrick/claude-desktop-debian` source; consider making `claude-routing` always pre-copy to clipboard as a side effect.

### Cold-load plugin discovery is ~370ms

With ~4 plugins, parsing TOML through Python `tomlq` takes around 370ms. The implementation plan targeted <100ms.

**Workaround**: ignore it for CLI use; it's still under half a second.

**Planned fix**: either (a) switch to `mikefarah/yq` (Go binary, faster startup), (b) add a registry JSON cache with mtime invalidation, or (c) write a small Python loader that handles many TOMLs in one process.

### `cos-cli` PATH

`cos-cli` lives in `~/.cargo/bin` after `cargo install`, but that directory isn't on the non-interactive bash PATH on a clean Pop!_OS setup. `lib/selection.sh` and `plugins/apps.toml` work around this by falling back to `$HOME/.cargo/bin/cos-cli`; if you `cargo install` to a different prefix, set the `COS_CLI` env var.

### Source-style scoping (`:tmux dotfiles`) isn't implemented

The spec's launcher inline composition uses sigils like `/.../`, `:source name`, `*verb*`, `--adverb=value`. The Phase-1 CLI doesn't parse those — it uses straightforward positional + `--flag` parsing. Sigil parsing is Phase 2 (meta-plugin).

### `goo compose` is a stub

The compose dialog binary is Phase 4. Until then `goo compose` prints a placeholder.

## Roadmap

Phase 1 (current) → CLI works end-to-end.

**Phase 2**: pop-launcher meta-plugin. Inline composition with type-aware autocomplete. Subject/verb/object stages.

**Phase 3**: scenes plugin (the first "rich" directory-form plugin). Anchor scenes (browser, mail, Claude Desktop). Favorite scene slots (1–5). Scene capture wizard.

**Phase 4**: compose dialog (`goo-compose`). Libcosmic/iced GUI. Three-panel UI. Sub-150ms cold start, sub-30ms with a `goo-composed` daemon.

**Phase 5**: broadening — `tmux`, `files`, `workspaces`, `clipboard-history`, `fabric` plugins. Selection-caching daemon if needed. `content-dispatch` graduates from regex heuristics to `sitting_duck` integration.

**Phase 6**: open-source polish — pre-built packages, screenshots, bindings examples for several keyboards.

Full task-level breakdown: [`docs/vision/cosmic-goo-implementation-plan.md`](../docs/vision/cosmic-goo-implementation-plan.md).
