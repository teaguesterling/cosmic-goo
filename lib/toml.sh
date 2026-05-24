# shellcheck shell=bash
# lib/toml.sh — TOML → JSON helpers, used by the plugin loader and CLI.
#
# Source this file; do not exec it.
#
# Functions:
#   toml_get  FILE [QUERY]    Run a jq-style query against a TOML file; emit JSON.
#                             QUERY defaults to "." (whole document).
#   toml_keys FILE [PATH]     List keys (raw, one per line) at PATH inside the file.
#                             PATH defaults to "." (top-level keys).
#
# Implementation: shells to `tomlq` from the Python `yq` package (kislyuk/yq).
# `tomlq` parses TOML into JSON and pipes through jq, so the query language is jq.

# Guard against direct execution.
if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/toml.sh: source this file, do not exec it" >&2
    exit 1
fi

# Ensure tomlq is on PATH (we know we're being sourced because of the guard above).
if ! command -v tomlq >/dev/null 2>&1; then
    echo "lib/toml.sh: tomlq not found on PATH (apt install yq)" >&2
    return 1
fi

toml_get() {
    local file=$1
    local query=${2:-.}
    if [ ! -f "$file" ]; then
        echo "toml_get: not a file: $file" >&2
        return 1
    fi
    tomlq "$query" "$file"
}

toml_keys() {
    local file=$1
    local path=${2:-.}
    if [ ! -f "$file" ]; then
        echo "toml_keys: not a file: $file" >&2
        return 1
    fi
    tomlq -r "($path) | keys[]" "$file"
}
