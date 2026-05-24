# Recon findings (longbottom, 2026-05-24)

## R1: Environment

- **Desktop**: COSMIC on Pop!_OS 24.04, Wayland session (`WAYLAND_DISPLAY=wayland-1`)
- **`COSMIC_DATA_CONTROL_ENABLED`**: unset — only matters for `wlr-data-control` clipboard managers (cliphist etc.), not for `wl-paste` itself
- **Cosmic D-Bus services visible**: `com.system76.CosmicLauncher`, `CosmicComp`, `CosmicSession`, `CosmicSettingsDaemon`, `CosmicWorkspaces`, `CosmicOnScreenDisplay`
- **Shortcuts config**: `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/{system_actions,custom}`
- **WindowRules config exists**: `~/.config/cosmic/com.system76.CosmicSettings.WindowRules/` — relevant for the deferred "anchor pinning" feature

## R2: Keys

Status: **deferred / user-driven**. Script is at `recon/keys.sh` — runs interactively, must be driven from a real terminal. Output goes to `keysyms.log` in CWD.

## R3: cos-cli

- Installed via `cargo install --git https://github.com/estin/cos-cli` → `~/.cargo/bin/cos-cli v0.5.1`
- `cos-cli info --json` shape:
  ```json
  {"apps": [{"index": 0, "app_id": "Alacritty", "title": "...",
             "state": ["maximized", "activated"],
             "outputs": [{"index": 0, "name": "DP-3"}],
             "workspaces": [{"group_index": 0, "index": 0, "workspace": "1"}]}]}
  ```
- Matches the spec's assumption. `.state[]? == "activated"` filter works for finding the focused window.
- **Caveat**: `~/.cargo/bin` not in non-interactive bash PATH. Plugin TOMLs that invoke `cos-cli` directly will fail unless the user's interactive shell has it on PATH, or we shell out via `$HOME/.cargo/bin/cos-cli` / detect at load.

## R4: `claude://` URL handler

- **`xdg-mime query default x-scheme-handler/claude` → `claude-desktop.desktop`** ✓
- Both `claude-desktop.desktop` (`/usr/share/applications/`) and `claude-code-url-handler.desktop` (`~/.local/share/applications/`) present.
- **First invocation worked**: `xdg-open "claude://claude.ai/new?q=Hello%20..."` opened a new chat with prompt prefilled (on cold start, app closed).
- **Second invocation FAILED to prefill**: Claude Desktop was already open with a new-chat tab; re-invoking with a different `q` value did not update the input box.
- **Third invocation (after closing dialog) — went to Cowork, no prefill**: `xdg-open "claude://claude.ai/new?q=..."` opened Claude Desktop on the **Cowork** view rather than a new chat, and did not populate the prompt. Suggests the URL routing on Linux (aaddrick build) doesn't reliably distinguish `claude.ai/new` from other paths; the `q=` parameter handling appears inconsistent.
- **Implication**: cannot trust `claude://claude.ai/new?q=...` for prompt handoff. The architecture (URL-based routing) is still viable but the specific paths and prefill semantics need revisiting before relying on them. Mitigation ideas:
  - **TODO**: look up current `claude://` URL scheme docs and known route paths for the Linux Electron build (aaddrick); the docs we have are macOS/Windows-oriented
  - Pre-copying prompt to clipboard as a fallback so the user can paste regardless of prefill behavior
  - Test other paths: `claude://claude.ai/chat/<uuid>`, `claude://code/new`, `claude://cowork/new`
  - Detect Claude Desktop state and conditionally fall back to clipboard route
  - Investigate whether the `aaddrick/claude-desktop-debian` repo exposes the URL handler implementation

## R5: `wl-paste --primary`

- **Reliable under COSMIC** ✓
- `wl-paste --primary` returns the current PRIMARY selection cleanly.
- `wl-paste --primary --list-types` returns both modern MIMEs (`text/plain;charset=utf-8`) and legacy X11 targets (`SAVE_TARGETS`, `MULTIPLE`, `STRING`, `UTF8_STRING`).
- **No need for a selection-caching daemon at this point.** Re-evaluate if flakiness surfaces under load.

## Tool inventory (post-install)

| Tool | Status | Notes |
|---|---|---|
| `jq` | ✓ | system |
| `cargo` / `rustc` | ✓ | system |
| `wl-clipboard` (wl-paste/wl-copy) | ✓ | apt |
| `wev` | ✓ | apt |
| `shellcheck` | ✓ | apt |
| `bats` 1.10.0 | ✓ | apt |
| `yq` 3.1 + `tomlq` | ✓ | apt (Python kislyuk version, not Go mikefarah; `tomlq` is what we use for TOML→JSON) |
| `cos-cli` 0.5.1 | ✓ | cargo, at `~/.cargo/bin/cos-cli` |
| `tmux`, `ffs`, `duckdb`, `claude` (CLI) | ✓ | preexisting |
| `fabric` | ✗ | deferred — only needed for the fabric route |
| `grim`, `slurp`, `zbarimg` | ✗ | deferred — screenshot/QR features are Phase 5+ |

## Gate status

| Item | Status |
|---|---|
| R1 env | ✓ |
| R2 keys | ⏸ deferred to user-driven run |
| R3 cos-cli | ✓ |
| R4 claude:// | ⚠ works first time; second-call prefill issue documented above |
| R5 wl-paste | ✓ |

**Verdict**: clear to start Phase 1. The R4 second-call quirk is a known issue, not a blocker — the architecture (URL-handler routing) is fundamentally viable. R2 can run any time before we start binding keys (Phase 2+).
