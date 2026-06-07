# cosmic-goo — Makefile
#
# The Rust engine (`crates/goo`) is the **canonical** goo. `make install` builds
# and installs the Rust binary. The bash engine (`bin/goo`, `lib/*.sh`) is a
# **legacy reference**, feature-frozen at pre-negotiation; it stays in the tree
# (and `make install-bash` still works) but new features land Rust-only.
#
# Conformance: `make test` runs bats against the Rust engine (canonical).
# `make test-bash` runs against the bash engine (legacy; expect ~28% skips for
# Rust-only features). `make test-both` runs both, for cross-engine parity work.
#
# The Rust binary still shells out to `bash`+`jq` at runtime (cmd templates,
# list_cmds), so those stay runtime deps regardless.

SHELL := /bin/bash

GOO_RELEASE = crates/target/release/goo
GOO_DEBUG   = crates/target/debug/goo

.PHONY: help test test-bash test-both shellcheck validate build install install-bash install-cosmic install-core uninstall tiers clean docs serve landscape landscape-check docs-install install-completion

# Install layout / tier selection.
# PREFIX defaults to a user install (~/.local, no root). TIERS selects which
# plugin tiers to install: core (pure), desktop (freedesktop/Wayland), cosmic
# (cos-cli). `install` = goo-standalone (core+desktop); `install-cosmic` adds
# the cosmic tier; `install-core` is the minimal headless engine.
PREFIX ?= $(HOME)/.local
TIERS  ?= core desktop
GOO_SHARE = $(PREFIX)/share/cosmic-goo

help:  ## Show this help
	@awk 'BEGIN { FS = ":.*##"; printf "Available targets:\n" } /^[a-zA-Z0-9_-]+:.*##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

test:  ## Run the bats conformance suite against the Rust engine (canonical)
	@command -v bats >/dev/null 2>&1 || { echo "bats not found. apt install bats"; exit 1; }
	@command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust (https://rustup.rs)"; exit 1; }
	@if [ ! -d tests ] || ! find tests -name '*.bats' -print -quit | grep -q .; then \
		echo "(no tests yet)"; exit 0; \
	fi
	@echo "Building goo (debug) for bats…"
	@cd crates && cargo build -p goo 2>&1 | tail -2
	@echo "Running bats against $(GOO_DEBUG)…"
	@GOO_BIN="$$(pwd)/$(GOO_DEBUG)" bats -r tests/

test-bash:  ## Run bats against the bash engine (legacy reference; ~28% tests skip)
	@command -v bats >/dev/null 2>&1 || { echo "bats not found. apt install bats"; exit 1; }
	@echo "Running bats against bin/goo (bash — legacy reference, feature-frozen)…"
	@echo "Expect ~94 skipped tests for Rust-only features (negotiation, OPTIONS, polymorphic verbs, …)."
	@bats -r tests/

test-both:  ## Run conformance on BOTH engines (parity verification / cross-engine work)
	@$(MAKE) --no-print-directory test
	@echo; echo "--- bash (legacy) ---"
	@$(MAKE) --no-print-directory test-bash

shellcheck:  ## Lint shell scripts under bin/ and lib/
	@if ! command -v shellcheck >/dev/null 2>&1; then \
		echo "shellcheck not found. apt install shellcheck"; exit 1; \
	fi
	@found=0; \
	for d in bin lib; do \
		if [ -d "$$d" ]; then \
			files=$$(find "$$d" -type f \( -name '*.sh' -o -perm -u+x \) ! -name '.gitkeep' 2>/dev/null); \
			if [ -n "$$files" ]; then \
				found=1; \
				echo "shellcheck $$d/"; \
				echo "$$files" | xargs shellcheck -x || exit 1; \
			fi; \
		fi; \
	done; \
	[ $$found -eq 0 ] && echo "(no shell scripts to check yet)" || true

validate:  ## Run goo validate (Phase 1 — once bin/goo exists)
	@if [ -x bin/goo ]; then \
		bin/goo validate; \
	else \
		echo "bin/goo not built yet"; \
	fi

build:  ## Build the Rust goo binary (release)
	@command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust (https://rustup.rs)"; exit 1; }
	@echo "Building $(GOO_RELEASE) (release)"
	@cd crates && cargo build --release -p goo

build-gui:  ## Build the native compose-GUI (iced; opt-in — pulls iced, not in the core build)
	@command -v cargo >/dev/null 2>&1 || { echo "cargo not found. Install Rust (https://rustup.rs)"; exit 1; }
	@echo "Building goo-compose-gui (iced)"
	@cd crates && cargo build -p goo-compose-gui

run-gui:  ## Launch the compose-GUI; Run spawns the debug goo (builds both first)
	@cd crates && cargo build -p goo -p goo-compose-gui
	@echo "Launching goo-compose-gui (Run spawns $(GOO_DEBUG))"
	@GOO_BIN="$$(pwd)/$(GOO_DEBUG)" sh -c 'cd crates && cargo run -p goo-compose-gui'

install: build  ## Install goo (Rust binary; standalone core+desktop tiers) to $PREFIX (default ~/.local)
	@echo "Installing goo (Rust) to $(PREFIX) [tiers: $(TIERS)]"
	@install -d "$(GOO_SHARE)/bin" "$(GOO_SHARE)/plugins" "$(PREFIX)/bin"
	@install -m 0755 "$(GOO_RELEASE)" "$(GOO_SHARE)/bin/goo-bin"
	@n=0; for f in plugins/*.toml; do \
		t=$$(grep -m1 '^tier = ' "$$f" | sed 's/tier = //; s/"//g'); \
		[ -z "$$t" ] && t=desktop; \
		case " $(TIERS) " in *" $$t "*) install -m 0644 "$$f" "$(GOO_SHARE)/plugins/"; n=$$((n+1));; esac; \
	done; echo "  installed $$n plugin(s)"
	@# Thin launcher: point the engine at the installed plugins (respect an
	@# existing override) and exec the real binary. The Rust bin resolves
	@# plugins by env only — no sibling path magic — so the wrapper supplies it.
	@{ echo '#!/bin/sh'; \
	   echo '# cosmic-goo launcher (generated by make install).'; \
	   echo 'export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$${COSMIC_GOO_BUILTIN_PLUGINS_DIR:-$(GOO_SHARE)/plugins}"'; \
	   echo 'exec "$(GOO_SHARE)/bin/goo-bin" "$$@"'; } > "$(GOO_SHARE)/bin/goo"
	@chmod 0755 "$(GOO_SHARE)/bin/goo"
	@ln -sf "$(GOO_SHARE)/bin/goo" "$(PREFIX)/bin/goo"
	@echo "  linked $(PREFIX)/bin/goo -> $(GOO_SHARE)/bin/goo (-> goo-bin)"
	@echo "Done. Ensure $(PREFIX)/bin is on PATH. Runtime still needs bash + jq."

install-bash:  ## Install the bash engine (LEGACY; feature-frozen pre-negotiation)
	@echo "Installing goo (bash — LEGACY reference; lacks negotiation, OPTIONS, polymorphic verbs, …)"
	@echo "Installing to $(PREFIX) [tiers: $(TIERS)]"
	@install -d "$(GOO_SHARE)/bin" "$(GOO_SHARE)/lib" "$(GOO_SHARE)/plugins" "$(PREFIX)/bin"
	@install -m 0755 bin/goo "$(GOO_SHARE)/bin/goo"
	@install -m 0644 lib/*.sh "$(GOO_SHARE)/lib/"
	@n=0; for f in plugins/*.toml; do \
		t=$$(grep -m1 '^tier = ' "$$f" | sed 's/tier = //; s/"//g'); \
		[ -z "$$t" ] && t=desktop; \
		case " $(TIERS) " in *" $$t "*) install -m 0644 "$$f" "$(GOO_SHARE)/plugins/"; n=$$((n+1));; esac; \
	done; echo "  installed $$n plugin(s)"
	@ln -sf "$(GOO_SHARE)/bin/goo" "$(PREFIX)/bin/goo"
	@echo "  linked $(PREFIX)/bin/goo -> $(GOO_SHARE)/bin/goo"
	@echo "Done. Ensure $(PREFIX)/bin is on PATH. (bin/goo finds lib/ and plugins/ as siblings under $(GOO_SHARE).)"

install-cosmic:  ## Install with the cosmic tier too (cosmic-goo: core+desktop+cosmic)
	@$(MAKE) install TIERS="core desktop cosmic"

install-core:  ## Install the minimal headless engine (core tier only)
	@$(MAKE) install TIERS="core"

uninstall:  ## Remove an install from $PREFIX
	@rm -f "$(PREFIX)/bin/goo"; rm -rf "$(GOO_SHARE)"; echo "Removed $(PREFIX)/bin/goo and $(GOO_SHARE)"

tiers:  ## List plugins grouped by dependency tier
	@for tier in core desktop cosmic; do \
		printf '\033[1m%s\033[0m\n' "$$tier"; \
		for f in plugins/*.toml; do \
			t=$$(grep -m1 '^tier = ' "$$f" | sed 's/tier = //; s/"//g'); \
			[ -z "$$t" ] && t=desktop; \
			[ "$$t" = "$$tier" ] && printf '  %s\n' "$$(basename "$$f" .toml)"; \
		done; \
	done

docs:  ## Build the docs site (output at site/)
	@if ! command -v mkdocs >/dev/null 2>&1; then \
		echo "mkdocs not found. See requirements-docs.txt for install options."; exit 1; \
	fi
	# Not --strict because index.md and limitations.md link out to docs/vision/,
	# which lives outside docs_dir on purpose (frozen design archive). Those
	# links resolve correctly on GitHub / local checkout, not in the rendered
	# site. Re-enable strict if the archive ever moves under doc/.
	mkdocs build

serve:  ## Run mkdocs dev server with live reload at http://127.0.0.1:8000/
	@if ! command -v mkdocs >/dev/null 2>&1; then \
		echo "mkdocs not found. See requirements-docs.txt for install options."; exit 1; \
	fi
	mkdocs serve

landscape:  ## Regenerate the registry-derived parts of cosmic-goo-landscape.html (stamps today's date)
	@python3 tools/gen-landscape.py --date "$$(date +%F)"

landscape-check:  ## Fail if the landscape page is stale vs the live registry
	@python3 tools/gen-landscape.py --check

docs-install:  ## Print install hints for the docs toolchain
	@echo "Install the docs toolchain via one of:"
	@echo "  pipx install mkdocs && pipx inject mkdocs mkdocs-material"
	@echo "  pip install --user -r requirements-docs.txt"
	@echo "  python -m venv .venv && .venv/bin/pip install -r requirements-docs.txt"

install-completion:  ## Install the bash completion to ~/.local/share/bash-completion/completions/goo
	@dest="$${XDG_DATA_HOME:-$$HOME/.local/share}/bash-completion/completions"; \
	mkdir -p "$$dest"; \
	cp completions/goo.bash "$$dest/goo"; \
	echo "Installed to $$dest/goo"; \
	echo "Open a new shell or run: source $$dest/goo"

clean:  ## Remove build artifacts
	@rm -rf dist/ build/ site/
