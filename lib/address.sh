# shellcheck shell=bash
# lib/address.sh — subject addressing.
#
# Turns a user-typed argument (or a programmatic URI) into a resolved subject
# JSON object. Source this file; do not exec it. Depends on lib/types.sh,
# lib/selection.sh, lib/plugin-loader.sh.
#
# Canonical URI forms:
#   cosmic-goo:<source>:<input>[?<params>]   enumerable source lookup (search a
#                                            source's list_cmd output)
#   cosmic-goo+<scheme>:<value>              scheme handoff (direct construction,
#                                            no search)
#
# Sigil aliases (pure textual prefix rewrites into the canonical form):
#   @x   -> cosmic-goo:x         (e.g. @app:firefox -> cosmic-goo:app:firefox)
#   +x   -> cosmic-goo+x         (e.g. +file:a.md  -> cosmic-goo+file:a.md)
#   ^x   -> cosmic-goo+clip:x    (e.g. ^alt        -> cosmic-goo+clip:alt)
#
# Native shapes (no sigil — they identify themselves lexically):
#   ./x ../x /x ~/x   -> cosmic-goo+file://<abspath>
#   <scheme>://...    -> cosmic-goo+<scheme>://...   (http, https, claude, ...)
#   anything else     -> cosmic-goo+text:<literal>
#
# Reserved, not yet implemented: ?params on source lookups, @source/path
# (slash hierarchy), named clipboard buffers (^name), file+exists modifiers.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/address.sh: source this file, do not exec it" >&2
    exit 1
fi

_addr_dir() { cd "$(dirname -- "${BASH_SOURCE[0]}")" && pwd; }

if ! declare -F mime_detect_content >/dev/null 2>&1; then
    # shellcheck source=SCRIPTDIR/types.sh
    . "$(_addr_dir)/types.sh"
fi
if ! declare -F selection_primary >/dev/null 2>&1; then
    # shellcheck source=SCRIPTDIR/selection.sh
    . "$(_addr_dir)/selection.sh"
fi
if ! declare -F plugin_load_all >/dev/null 2>&1; then
    # shellcheck source=SCRIPTDIR/plugin-loader.sh
    . "$(_addr_dir)/plugin-loader.sh"
fi

# Core structural sigils are fixed (they ARE the canonical URI forms):
#   :x  -> cosmic-goo:x     (source path)
#   +x  -> cosmic-goo+x     (scheme handoff)
# Everything else is a customizable sigil: a single char that expands to a
# string (usually starting with : or +), declared via [[sigils]] in any plugin.
# A built-in sigils.toml ships ^ -> +clip:. Users add/override in their own
# plugin TOMLs.

# Cache the char->expansion map for the shell's lifetime.
_ADDR_SIGILS=""
_addr_sigils() {
    if [ -z "$_ADDR_SIGILS" ]; then
        # "char\texpansion" lines. Guard against an empty registry.
        _ADDR_SIGILS=$(plugin_load_all 2>/dev/null \
            | jq -r '(.sigils // [])[] | "\(.char)\t\(.expands)"' 2>/dev/null)
        # Sentinel so we don't re-query when there are genuinely no sigils.
        [ -z "$_ADDR_SIGILS" ] && _ADDR_SIGILS=$'\x00'
    fi
    [ "$_ADDR_SIGILS" = $'\x00' ] && return 0
    printf '%s\n' "$_ADDR_SIGILS"
}

address_invalidate_sigils() { _ADDR_SIGILS=""; }

# Echo the expansion for a single-char sigil, or nothing.
_addr_sigil_expand() {
    local ch=$1 sch sexp
    while IFS=$'\t' read -r sch sexp; do
        [ "$sch" = "$ch" ] && { printf '%s' "$sexp"; return 0; }
    done < <(_addr_sigils)
    return 1
}

# True if RAW carries an explicit sigil (core or custom) or native shape that
# address_resolve should handle — as opposed to a bare word the caller treats
# as text or a type-scoped handle search.
address_is_explicit() {
    local raw=$1
    case "$raw" in
        :*|+*|./*|../*|/*|[~]/*|cosmic-goo:*|cosmic-goo+*) return 0 ;;
        [a-zA-Z]*://*) return 0 ;;
    esac
    # Custom sigil? First char registered.
    [ -n "$raw" ] && _addr_sigil_expand "${raw:0:1}" >/dev/null 2>&1
}

# Absolute path without requiring existence (handler reports missing files).
_addr_abspath() {
    local p=$1
    case "$p" in
        [~]/*) p="$HOME/${p#\~/}" ;;
    esac
    if command -v realpath >/dev/null 2>&1; then
        realpath -m -- "$p"
    else
        case "$p" in
            /*) printf '%s' "$p" ;;
            *)  printf '%s/%s' "$PWD" "$p" ;;
        esac
    fi
}

# Rewrite a user-typed argument into a canonical cosmic-goo URI.
address_canonicalize() {
    local raw=$1

    # Already canonical.
    case "$raw" in cosmic-goo:*|cosmic-goo+*) printf '%s' "$raw"; return 0 ;; esac

    # Custom sigil expansion (single leading char -> expansion + rest), unless
    # the leading char is a core/native one we handle structurally below.
    case "$raw" in
        :*|+*|./*|../*|/*|[~]/*) : ;;  # core/native, skip custom expansion
        [a-zA-Z]*://*) : ;;            # native URL
        *)
            local exp
            if [ -n "$raw" ] && exp=$(_addr_sigil_expand "${raw:0:1}"); then
                raw="${exp}${raw:1}"
            fi
            ;;
    esac

    # Core structural sigils + native shapes.
    case "$raw" in
        cosmic-goo:*|cosmic-goo+*) printf '%s' "$raw" ;;
        :*) printf 'cosmic-goo:%s' "${raw#:}" ;;
        +*) printf 'cosmic-goo+%s' "${raw#+}" ;;
        ./*|../*|/*|[~]/*) printf 'cosmic-goo+file://%s' "$(_addr_abspath "$raw")" ;;
        [a-zA-Z]*://*) printf 'cosmic-goo+%s' "$raw" ;;
        *) printf 'cosmic-goo+text:%s' "$raw" ;;
    esac
}

# Resolve a canonical/sigil/native address to a subject JSON object.
# Usage: address_resolve RAW [VERB_JSON]
address_resolve() {
    local raw=$1 verb_json=${2:-null}
    local uri
    uri=$(address_canonicalize "$raw")
    case "$uri" in
        cosmic-goo:*)
            _addr_resolve_source "${uri#cosmic-goo:}" "$verb_json"
            ;;
        cosmic-goo+*)
            local rest=${uri#cosmic-goo+}
            local scheme=${rest%%:*}
            local value=${rest#*:}
            _addr_resolve_scheme "$scheme" "$value"
            ;;
        *)
            echo "address_resolve: cannot canonicalize '$raw'" >&2
            return 1
            ;;
    esac
}

# Scheme handlers (cosmic-goo+<scheme>:<value>).
_addr_resolve_scheme() {
    local scheme=$1 value=$2
    case "$scheme" in
        text)
            local mt
            mt=$(mime_detect_content "$value")
            jq -nc --arg t "$mt" --arg text "$value" '{type:$t, text:$text}'
            ;;
        file)
            # value is //<abspath> (from native) or a bare path (from +file:).
            local path=${value#//}
            if [ ! -e "$path" ]; then
                echo "address: no such file: $path" >&2
                return 1
            fi
            local mt content title
            mt=$(mime_detect_path "$path")
            title=${path##*/}
            if [[ "$mt" == text/* || "$mt" == application/json || "$mt" == application/xml ]]; then
                content=$(cat -- "$path")
            else
                content=""   # binary: leave .text empty; reference verbs use metadata.path
            fi
            jq -nc --arg t "$mt" --arg text "$content" --arg path "$path" --arg title "$title" \
                '{type:$t, text:$text, id:$path, title:$title, metadata:{path:$path}}'
            ;;
        clip)
            # value is an optional named buffer; named buffers are future work.
            if [ -n "$value" ]; then
                echo "address: named clipboard buffers ('^$value') not yet supported" >&2
                return 1
            fi
            local text
            text=$(selection_clipboard)
            jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
            ;;
        sel|selection)
            local text
            text=$(selection_primary)
            jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
            ;;
        stdin)
            local text
            text=$(cat)
            jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
            ;;
        http|https|ftp|ftps|mailto|claude|file*)
            # Treat as a URI reference. Reconstruct the original scheme:value.
            # .id carries the locator (its identity) so an opener verb can use
            # one field for both files (.id = path) and URLs (.id = the URL).
            local url="$scheme:$value"
            jq -nc --arg url "$url" '{type:"text/x-uri", text:$url, id:$url}'
            ;;
        *)
            # Unknown scheme: best-effort URI reference.
            local url="$scheme:$value"
            jq -nc --arg url "$url" '{type:"text/x-uri", text:$url, id:$url}'
            ;;
    esac
}

# Source handler (cosmic-goo:<source>:<input>[?<params>]).
# Looks up a source by name OR prefix, runs its list_cmd, and matches `input`
# against item id/title. Params after `?` are parsed off and ignored for now.
# Build a JSON object {key:value,...} from a `&`-separated `key=value` param
# string. `*` wildcards are stripped (params match by case-insensitive
# substring). Echoes "{}" when empty. This is the ?params analogue of a verb's
# valid_when: both are predicates over the item/subject JSON.
_addr_params_to_json() {
    local raw=$1
    [ -z "$raw" ] && { printf '{}'; return 0; }
    local obj='{}' pair k v
    local IFS='&'
    for pair in $raw; do
        [ -z "$pair" ] && continue
        k=${pair%%=*}
        v=${pair#*=}
        [ "$k" = "$pair" ] && continue   # no '=', skip
        v=${v//\*/}                        # strip glob stars -> substring match
        obj=$(jq -c --arg k "$k" --arg v "$v" '. + {($k): $v}' <<<"$obj")
    done
    printf '%s' "$obj"
}

_addr_resolve_source() {
    local spec=$1 verb_json=$2

    # Split off ?params and compile them to a filter object.
    local params_json='{}'
    case "$spec" in
        *\?*) params_json=$(_addr_params_to_json "${spec#*\?}"); spec=${spec%%\?*} ;;
    esac

    local source_key input
    source_key=${spec%%:*}
    if [ "$spec" = "$source_key" ]; then
        input=""
    else
        input=${spec#*:}
    fi
    if [ -z "$source_key" ]; then
        echo "address: empty source in '$spec'" >&2
        return 1
    fi

    local registry source
    registry=$(plugin_load_all)
    # Match the source by its name or its prefix field.
    source=$(jq -c --arg k "$source_key" '
        .sources[] | select(.name == $k or (.prefix // "") == $k)
    ' <<<"$registry" | head -n 1)
    if [ -z "$source" ]; then
        echo "address: no source named or prefixed '$source_key'" >&2
        return 1
    fi

    local emits list_cmd items
    emits=$(jq -r '.emits // "text/plain"' <<<"$source")
    list_cmd=$(jq -r '.list_cmd // empty' <<<"$source")
    if [ -z "$list_cmd" ]; then
        echo "address: source '$source_key' has no list_cmd" >&2
        return 1
    fi
    items=$(bash -c "$list_cmd" 2>/dev/null)
    if [ -z "$items" ]; then
        echo "address: source '$source_key' produced no items" >&2
        return 1
    fi

    # Apply ?params: keep items where every key=value matches (case-insensitive
    # substring) against the item's top-level field or its metadata field.
    if [ "$params_json" != "{}" ]; then
        items=$(jq -c --argjson p "$params_json" '
            (try (. // []) catch [])
            | map(select(. as $it
                | [ $p | to_entries[] | .key as $k | .value as $v
                    | (($it[$k] // $it.metadata[$k]) // "" | tostring | ascii_downcase
                       | contains($v | ascii_downcase)) ]
                | all))
        ' <<<"$items" 2>/dev/null)
        if [ "$(jq 'length' <<<"$items" 2>/dev/null)" = "0" ]; then
            echo "address: no item in source '$source_key' matches the given ?params" >&2
            return 1
        fi
    fi

    # No input: return the first item (sources like selection/clipboard have one).
    if [ -z "$input" ]; then
        jq -c --arg type "$emits" '
            (try (. // []) catch []) | .[0] | select(. != null) | . + {type: $type}
        ' <<<"$items" 2>/dev/null
        return 0
    fi

    # Input given: match against id or title (case-insensitive substring).
    local match
    match=$(jq -c --arg q "$input" --arg type "$emits" '
        (try (. // []) catch []) | .[]?
        | select(
            (((.id    // "") | ascii_downcase) | contains($q | ascii_downcase))
            or
            (((.title // "") | ascii_downcase) | contains($q | ascii_downcase))
          )
        | . + {type: $type}
    ' <<<"$items" 2>/dev/null | head -n 1)
    if [ -z "$match" ]; then
        echo "address: no item matching '$input' in source '$source_key'" >&2
        return 1
    fi
    printf '%s\n' "$match"
}
