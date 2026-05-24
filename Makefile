# cosmic-goo — Makefile
#
# Phase 1 is shell-only. Rust targets land in Phase 4.

SHELL := /bin/bash

.PHONY: help test shellcheck validate install install-user clean manual open-manual

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

manual:  ## Render doc/ to a single self-contained HTML file at doc/manual.html
	@if ! command -v pandoc >/dev/null 2>&1; then \
		echo "pandoc not found. apt install pandoc"; exit 1; \
	fi
	@pandoc \
		--standalone \
		--embed-resources \
		--toc \
		--toc-depth=2 \
		--metadata title="cosmic-goo manual" \
		--metadata "subtitle=Phase 1 — Grammar Of Operations" \
		--css=doc/manual.css \
		-o doc/manual.html \
		doc/intro.md \
		doc/cli-reference.md \
		doc/plugin-authoring.md \
		doc/examples/ms-natural-4000-bindings.md \
		doc/limitations.md
	@echo "wrote doc/manual.html"

open-manual: manual  ## Build the manual and open it in your default browser
	@xdg-open doc/manual.html

clean:  ## Remove build artifacts
	@rm -rf dist/ build/ doc/manual.html
