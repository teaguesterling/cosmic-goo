# shellcheck shell=bash
# lib/types.sh — MIME type matching and detection.
#
# Source this file; do not exec it.
#
# Functions:
#   mime_matches PATTERN MIME   Exit 0 if MIME matches the glob PATTERN.
#                               Patterns may be exact ("text/plain"), suffix
#                               wildcard ("text/*"), prefix wildcard ("*/json"),
#                               or vendor wildcard ("application/vnd.foo.*").
#   mime_detect_path PATH       Echo the MIME type of a file on disk (uses libmagic).
#   mime_detect_content STRING  Echo the MIME type of an arbitrary string.
#                               Heuristics in order: URI regex -> text/x-uri,
#                               existing file path -> mime_detect_path, libmagic
#                               on the content itself, fallback text/plain.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/types.sh: source this file, do not exec it" >&2
    exit 1
fi

mime_matches() {
    local pattern=$1 mime=$2
    [ -z "$pattern" ] || [ -z "$mime" ] && return 1
    # Bash `case` does glob-style matching on unquoted $pattern.
    # shellcheck disable=SC2254  # intentional unquoted glob
    case "$mime" in
        $pattern) return 0 ;;
        *) return 1 ;;
    esac
}

mime_detect_path() {
    local path=$1
    if [ ! -e "$path" ]; then
        echo "mime_detect_path: not found: $path" >&2
        return 1
    fi
    file --mime-type -b -- "$path"
}

mime_detect_content() {
    local content=$1
    # 1. URI scheme (RFC 3986 scheme syntax: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":")
    if [[ "$content" =~ ^[a-zA-Z][a-zA-Z0-9+.-]*://[^[:space:]] ]]; then
        echo "text/x-uri"
        return 0
    fi
    # 2. Existing path on disk (single-line, no embedded newlines)
    if [[ "$content" != *$'\n'* ]] && [ -e "$content" ]; then
        mime_detect_path "$content"
        return $?
    fi
    # 3. libmagic on the content itself
    local detected
    detected=$(printf '%s' "$content" | file --mime-type -b -)
    if [ -n "$detected" ] && [ "$detected" != "application/octet-stream" ]; then
        echo "$detected"
        return 0
    fi
    # 4. Default
    echo "text/plain"
}
