# cosmic-goo — Makefile
#
# Phase 1 is shell-only. Rust targets land in Phase 4.

SHELL := /bin/bash

.PHONY: help test shellcheck validate install install-user clean

help:  ## Show this help
	@awk 'BEGIN { FS = ":.*##"; printf "Available targets:\n" } /^[a-zA-Z0-9_-]+:.*##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

test:  ## Run bats test suite
	@if ! command -v bats >/dev/null 2>&1; then \
		echo "bats not found. apt install bats"; exit 1; \
	fi
	@if [ -d tests ] && ls tests/*.bats >/dev/null 2>&1; then \
		bats tests/; \
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

clean:  ## Remove build artifacts
	@rm -rf dist/ build/
