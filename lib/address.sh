# shellcheck shell=bash
# lib/address.sh — subject addressing (the goo:// domain model).
#
# Turns a user-typed argument (or a programmatic URI) into a resolved subject
# JSON object. Source this file; do not exec it. Depends on lib/types.sh,
# lib/selection.sh, lib/plugin-loader.sh. Mirrors crates/goo-engine/src/address.rs.
#
# One canonical form:
#   goo://<domain>/<path>[;q=<query>][?<refine>]
#     - value  : goo://<domain>/<path>      <path> is an EXACT locator (a
#                source item's exact id, a file path, literal text, a URL).
#     - search : goo://<domain>/;q=<query>  FUZZY query over a source's list_cmd
#                (id/title substring).
# Resolution is strict: the syntax says which you mean; no fuzzy fallback.
#
# Built-in value domains resolved here: text / file / clip / sel / stdin / url.
# Every other domain is a [[sources]] entry (matched by name OR prefix).
#
# Sigils (terminal shorthand; machines emit canonical goo://):
#   bare / ./ ~/ / scheme://   -> infer (text / file / url)
#   +x                         -> goo://text/x      (force literal text)
#   :dom/path                  -> goo://dom/path    (value, exact)
#   :dom:query                 -> goo://dom/;q=query (search, fuzzy)
#   ^  / ^name                 -> goo://clip/ , goo://clip/name
#   <other first char>         -> user [[sigils]] alias

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

# Cache the char->expansion map for the shell's lifetime.
_ADDR_SIGILS=""
_addr_sigils() {
    if [ -z "$_ADDR_SIGILS" ]; then
        _ADDR_SIGILS=$(plugin_load_all 2>/dev/null \
            | jq -r '(.sigils // [])[] | "\(.char)\t\(.expands)"' 2>/dev/null)
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

# True if RAW carries an explicit sigil / native shape / canonical URI (vs a
# bare word the caller treats as text or a type-scoped handle search). `+x`
# (force text) and `^` (clip) are explicit; a bare word is not.
address_is_explicit() {
    local raw=$1
    case "$raw" in
        :*|+*|^*|./*|../*|/*|[~]/*|goo://*) return 0 ;;
        [a-zA-Z]*://*) return 0 ;;
    esac
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

# Expand a `:`-sigil tail into canonical: the first `/` (value path) or `:`
# (`;q=` search) after the domain decides. `:dom` alone -> the domain default.
_addr_colon_sigil() {
    local rest=$1
    local before_slash="${rest%%/*}" before_colon="${rest%%:*}"
    local has_slash=0 has_colon=0
    [ "$before_slash" != "$rest" ] && has_slash=1
    [ "$before_colon" != "$rest" ] && has_colon=1
    if [ "$has_slash" = 1 ] && [ "$has_colon" = 1 ]; then
        if [ ${#before_slash} -lt ${#before_colon} ]; then
            printf 'goo://%s/%s' "$before_slash" "${rest#*/}"
        else
            printf 'goo://%s/;q=%s' "$before_colon" "${rest#*:}"
        fi
    elif [ "$has_slash" = 1 ]; then
        printf 'goo://%s/%s' "$before_slash" "${rest#*/}"
    elif [ "$has_colon" = 1 ]; then
        printf 'goo://%s/;q=%s' "$before_colon" "${rest#*:}"
    else
        printf 'goo://%s/' "$rest"
    fi
}

# Rewrite a user-typed argument into a canonical goo:// URI.
address_canonicalize() {
    local raw=$1
    case "$raw" in goo://*) printf '%s' "$raw"; return 0 ;; esac

    # Custom sigil expansion (unless a built-in/native shape), then recurse so
    # the expansion (e.g. :gad: / +) is itself canonicalized.
    case "$raw" in
        :*|+*|^*|./*|../*|/*|[~]/*|[a-zA-Z]*://*) : ;;
        *)
            local exp
            if [ -n "$raw" ] && exp=$(_addr_sigil_expand "${raw:0:1}"); then
                address_canonicalize "${exp}${raw:1}"
                return 0
            fi
            ;;
    esac

    case "$raw" in
        ^)  printf 'goo://clip/' ;;
        ^*) printf 'goo://clip/%s' "${raw#^}" ;;
        +*) printf 'goo://text/%s' "${raw#+}" ;;
        :*) _addr_colon_sigil "${raw#:}" ;;
        ./*|../*|/*|[~]/*) printf 'goo://file/%s' "$raw" ;;
        [a-zA-Z]*://*) printf 'goo://url/%s' "$raw" ;;
        *) printf 'goo://text/%s' "$raw" ;;
    esac
}

# Resolve a canonical/sigil/native address to a subject JSON object.
# Usage: address_resolve RAW [VERB_JSON]   (VERB_JSON accepted for caller/parity
# but unused — the resolver is verb-agnostic, like the Rust engine.)
address_resolve() {
    local raw=$1
    local uri rest domain after refine is_search q
    uri=$(address_canonicalize "$raw")
    case "$uri" in
        goo://*) rest=${uri#goo://} ;;
        *) echo "address_resolve: cannot canonicalize '$raw'" >&2; return 1 ;;
    esac
    case "$rest" in
        */*) domain=${rest%%/*}; after=${rest#*/} ;;
        *)   domain=$rest; after="" ;;
    esac
    if [ -z "$domain" ]; then
        echo "address: empty domain in '$uri'" >&2
        return 1
    fi
    refine=""
    case "$after" in *\?*) refine=${after#*\?}; after=${after%%\?*} ;; esac
    is_search=0; q=$after
    case "$after" in ";q="*) is_search=1; q=${after#;q=} ;; esac

    case "$domain" in
        text)          _addr_dom_text "$q" ;;
        file)          _addr_dom_file "$q" ;;
        clip)          _addr_dom_clip "$q" ;;
        sel|selection) _addr_dom_sel ;;
        stdin)         _addr_dom_stdin ;;
        url)           _addr_dom_url "$q" ;;
        *)             _addr_resolve_source_domain "$domain" "$q" "$is_search" "$refine" ;;
    esac
}

# ---- built-in value domains ----

_addr_dom_text() {
    local v=$1 mt
    mt=$(mime_detect_content "$v")
    jq -nc --arg t "$mt" --arg text "$v" '{type:$t, text:$text}'
}

_addr_dom_file() {
    local path
    path=$(_addr_abspath "$1")
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
        content=""
    fi
    jq -nc --arg t "$mt" --arg text "$content" --arg path "$path" --arg title "$title" \
        '{type:$t, text:$text, id:$path, title:$title, metadata:{path:$path}}'
}

_addr_dom_clip() {
    if [ -n "$1" ]; then
        echo "address: named clipboard buffers ('^$1') not yet supported" >&2
        return 1
    fi
    local text
    text=$(selection_clipboard)
    jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
}

_addr_dom_sel() {
    local text
    text=$(selection_primary)
    jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
}

_addr_dom_stdin() {
    local text
    text=$(cat)
    jq -nc --arg text "$text" '{type:"text/plain", text:$text}'
}

_addr_dom_url() {
    local url=$1
    jq -nc --arg url "$url" '{type:"text/x-uri", text:$url, id:$url}'
}

# Build a JSON object {key:value,...} from a `&`-separated `key=value` refine
# string. `*` wildcards are stripped (params match by case-insensitive
# substring). Echoes "{}" when empty.
_addr_params_to_json() {
    local raw=$1
    [ -z "$raw" ] && { printf '{}'; return 0; }
    local obj='{}' pair k v
    local IFS='&'
    for pair in $raw; do
        [ -z "$pair" ] && continue
        k=${pair%%=*}
        v=${pair#*=}
        [ "$k" = "$pair" ] && continue
        v=${v//\*/}
        obj=$(jq -c --arg k "$k" --arg v "$v" '. + {($k): $v}' <<<"$obj")
    done
    printf '%s' "$obj"
}

# ---- source domains (value = exact id; search = fuzzy id/title) ----

_addr_resolve_source_domain() {
    local domain=$1 q=$2 is_search=$3 refine=$4
    local registry source emits list_cmd items
    registry=$(plugin_load_all)
    source=$(jq -c --arg k "$domain" '
        .sources[] | select(.name == $k or (.prefix // "") == $k)
    ' <<<"$registry" | head -n 1)
    if [ -z "$source" ]; then
        echo "address: no domain or source named '$domain'" >&2
        return 1
    fi
    emits=$(jq -r '.emits // "text/plain"' <<<"$source")
    list_cmd=$(jq -r '.list_cmd // empty' <<<"$source")
    if [ -z "$list_cmd" ]; then
        echo "address: domain '$domain' has no list_cmd" >&2
        return 1
    fi
    items=$(bash -c "$list_cmd" 2>/dev/null)
    if [ -z "$items" ]; then
        echo "address: domain '$domain' produced no items" >&2
        return 1
    fi

    if [ -n "$refine" ]; then
        local params_json
        params_json=$(_addr_params_to_json "$refine")
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
                echo "address: no item in '$domain' matches the given ?refine" >&2
                return 1
            fi
        fi
    fi

    # Empty path/query: the domain's first item (default).
    if [ -z "$q" ]; then
        jq -c --arg type "$emits" '
            (try (. // []) catch []) | .[0] | select(. != null) | . + {type: $type}
        ' <<<"$items" 2>/dev/null
        return 0
    fi

    local match
    if [ "$is_search" = "1" ]; then
        # Fuzzy: id or title contains the query (case-insensitive).
        match=$(jq -c --arg q "$q" --arg type "$emits" '
            (try (. // []) catch []) | .[]?
            | select(
                (((.id    // "") | ascii_downcase) | contains($q | ascii_downcase))
                or
                (((.title // "") | ascii_downcase) | contains($q | ascii_downcase))
              )
            | . + {type: $type}
        ' <<<"$items" 2>/dev/null | head -n 1)
        if [ -z "$match" ]; then
            echo "address: no item matching '$q' in '$domain'" >&2
            return 1
        fi
    else
        # Value: exact id match.
        match=$(jq -c --arg q "$q" --arg type "$emits" '
            (try (. // []) catch []) | .[]? | select(.id == $q) | . + {type: $type}
        ' <<<"$items" 2>/dev/null | head -n 1)
        if [ -z "$match" ]; then
            echo "address: no item with id '$q' in '$domain'" >&2
            return 1
        fi
    fi
    printf '%s\n' "$match"
}
