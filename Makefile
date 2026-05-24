# cosmic-goo — Makefile
#
# Phase 1 is shell-only. Rust targets land in Phase 4.

SHELL := /bin/bash

.PHONY: help test shellcheck validate install install-user clean docs serve docs-install install-completion

help:  ## Show this help
	@awk 'BEGIN { FS = ":.*##"; printf "Available targets:\n" } /^[a-zA-Z0-9_-]+:.*##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

test:  ## Run bats test suite
	@if ! command -v bats >/dev/null 2>&1; then \
		echo "bats not found. apt install bats"; exit 1; \
	fi
	@if [ -d tests ] && find tests -name '*.bats' -print -quit | grep -q .; then \
		bats -r tests/; \
	else \
		echo "(no tests yet)"; \
	fi

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

install:  ## Install to /usr/local (system-wide) — Phase 1 TBD
	@echo "install target not implemented yet"

install-user:  ## Install to ~/.local (user-only) — Phase 1 TBD
	@echo "install-user target not implemented yet"

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
