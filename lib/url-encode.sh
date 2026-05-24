# shellcheck shell=bash
# lib/url-encode.sh — URL percent-encoding helper.
#
# Source this file; do not exec it.
#
# Functions:
#   url_encode STRING   Echo a percent-encoded version of STRING (RFC 3986),
#                       suitable for embedding in query parameters.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/url-encode.sh: source this file, do not exec it" >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "lib/url-encode.sh: jq not found on PATH" >&2
    return 1
fi

url_encode() {
    printf '%s' "$1" | jq -sRr '@uri'
}
