# Changelog

All notable changes to cosmic-goo are documented here. The format loosely follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [0.1.0] — 2026-07-01

First tagged release. cosmic-goo — **G**rammar **O**f **O**perations — is a
compositional `subject → verb → object` sentence layer over the COSMIC desktop: a
noun-first CLI and keyboard launcher where you address a thing, pick what to do with
it, and the engine negotiates how.

This is an early `0.x`: the core engine and CLI are built and covered by a
conformance suite, but some capabilities are still designed-not-built and a few
heuristics are calibrated rather than proven (see *Known limitations*).

### Core engine (`goo-engine`) + CLI (`goo`)

- **Addressing grammar** — canonical `goo://<domain>/<path>` URIs with sigils: `:`
  (source entity, exact value vs. `:src:query` fuzzy search), `+` (force text), `^`
  (clipboard), `=` (type assertion), plus native file-path and URL shapes.
- **Type detection** — registry-driven checkers + an OS-MIME lattice (`is_a` /
  subtype), an authoritative extension signal, and libmagic fallback; `--explain`
  annotates which signal chose the type.
- **File-vs-data membership** — a file on disk carries both its content type and an
  `inode/file` membership, so handle verbs (`open`/`reveal`/`copy-path`) and content
  verbs both apply; clipboard/`+text` data of the same type correctly does not gain
  handle verbs (provenance guard).
- **Verb negotiation** — a Dijkstra planner routes a subject to a verb's accepted
  type through declared `[[channels]]` converters; `--explain` previews the chosen
  route (and `--paths` enumerates the route graph). Output routing via `--to`/`-o`
  (file + clipboard destinations).
- **Polymorphic dispatch** — a verb name with multiple impls (`show`, `connect`,
  `stop`, …) selects the impl that accepts the *resolved* subject, verb-first
  (`goo show :br/main`) and noun-first (`goo do :br/main show`).
- **Entity-name inference** — a bare token is scored across enumerable sources and
  bucketed into confidence bands (definitive → silent, high → nudge, medium →
  picker, low → text), verb-aware.
- **Dynamic verb providers** (`[[providers]]`) — a subject type can contribute verbs
  at listing time from a `list_cmd` (e.g. per-project commands on `:cwd`).
- **Security** — command injection made impossible by construction (`|q` shell-quote,
  `|uri` percent-encode, validated verb-name identifiers); a per-source declared
  facet allowlist so untrusted `list_cmd` output can't forge verb-granting
  memberships; terminal-display sanitization for untrusted strings
  (`Tainted`/`DisplayView`).

### Plugins

- A starter set of TOML plugins across the desktop: apps/windows, audio, bluetooth,
  network, clipboard, containers, git, processes, services, mounts, screenshots,
  tmux, URLs, working-directory, and text utilities.
- **woollama** is the canonical local-LLM route for the text verbs
  (`summarize`/`critique`/`think`/`draft-response`) with a `model` selector adverb
  and live model-id completion.
- A dep-tiered `providers/` starter collection and a worked `:contact` source
  demonstrating per-instance capability facets (`emailable`/`callable`/`messageable`).

### GUI

- **`goo-compose-gui`** (iced) — a keyboard-driven `Subject → Verb → Object`
  launcher: type-to-filter each pane, recency-reordered verbs, a live CLI-equivalent
  preview, run-on-commit for plain verbs, an armed Ready pane for gated/destructive
  verbs, an adverb panel, and a run → result/error stage that surfaces output
  (including LLM replies) without freezing the window.
- **`goo-control-gui`** (iced) — a registry/entity/plugin browser (v1).

### Known limitations

- Inference confidence bands are **calibrated, not proven** against a real corpus —
  treat the boundaries as tunable.
- Parts of the type-detection signal model and several expansion entities are
  **designed, not yet built**.
- No CI is wired up yet; the suite is run locally (`make test`).
- The `goo-engine` doc-tests require a rustdoc toolchain with `libLLVM` available.

[0.1.0]: https://github.com/teaguesterling/cosmic-goo/releases/tag/v0.1.0
