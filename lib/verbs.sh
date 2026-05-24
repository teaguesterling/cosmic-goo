# shellcheck shell=bash
# lib/verbs.sh — verb lookup, adverb resolution, template substitution, dispatch.
#
# Source this file; do not exec it. Depends on lib/plugin-loader.sh and lib/types.sh.
#
# Functions:
#   verb_lookup NAME [TYPE]            JSON for the verb of that name (optionally filtered
#                                      to those whose `accepts` matches TYPE). Empty on miss.
#   verb_default_for TYPE              JSON for the verb whose `default_for` matches TYPE.
#   verb_for_subject SUBJECT_JSON      JSON, one verb per line, applicable to subject.type.
#   verb_apply VERB_JSON SUBJECT_JSON [OBJECT_JSON] [ADVERBS_JSON]
#                                      Resolve adverbs, render template, execute the command.
#   verb_invalidate_cache              Drop the in-memory registry cache (use after plugin edits).
#
# Convention for selector adverbs (clarification of the spec):
#   [[adverbs]]
#   name = "via"
#   kind = "selector"
#   default = "fabric"
#   [adverbs.values.fabric]
#   template = "..."
#   [adverbs.values.clipboard]
#   template = "..."
#
# Template substitution syntax: `{path.to.var}`. Paths are dotted into the
# context dict: { subject, object, verb, adverbs, cwd, <injected> }. Values
# from selector adverbs' `template_var` get spread at the top level.
#
# Phase-1 limitation: substitutions are RAW. Template authors handle shell
# quoting (single-quote around `{var}`, use bash here-strings, etc.). A
# `{var|filter}` syntax for shell-quote, URL-encode, etc. is future work.

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/verbs.sh: source this file, do not exec it" >&2
    exit 1
fi

# Resolve our own dir to find siblings.
_v_dir() {
    local src=${BASH_SOURCE[0]}
    cd "$(dirname -- "$src")" && pwd
}

# Source siblings on first load.
if ! declare -F plugin_load_all >/dev/null 2>&1; then
    # shellcheck source=SCRIPTDIR/plugin-loader.sh
    . "$(_v_dir)/plugin-loader.sh"
fi
if ! declare -F mime_matches >/dev/null 2>&1; then
    # shellcheck source=SCRIPTDIR/types.sh
    . "$(_v_dir)/types.sh"
fi

# Module-level registry cache (in-memory, per shell).
_VERBS_REGISTRY=""

_verbs_registry() {
    [ -n "$_VERBS_REGISTRY" ] || _VERBS_REGISTRY=$(plugin_load_all)
    printf '%s' "$_VERBS_REGISTRY"
}

verb_invalidate_cache() {
    _VERBS_REGISTRY=""
}

verb_lookup() {
    local name=$1 type=${2:-}
    local verb
    verb=$(jq -c --arg n "$name" '.verbs[] | select(.name == $n)' <<<"$(_verbs_registry)")
    [ -z "$verb" ] && return 1
    if [ -n "$type" ]; then
        local pattern matched=0
        while IFS= read -r pattern; do
            [ -z "$pattern" ] && continue
            if mime_matches "$pattern" "$type"; then
                matched=1
                break
            fi
        done < <(jq -r '.accepts[]?' <<<"$verb")
        [ "$matched" -eq 0 ] && return 1
    fi
    printf '%s\n' "$verb"
}

verb_default_for() {
    local type=$1
    jq -c --arg t "$type" '
        .verbs[] | select((.default_for // null) == $t)
    ' <<<"$(_verbs_registry)" | head -n 1
}

verb_for_subject() {
    local subject_json=$1
    local subject_type
    subject_type=$(jq -r '.type // empty' <<<"$subject_json")
    [ -z "$subject_type" ] && return 1
    local verb pattern
    while IFS= read -r verb; do
        while IFS= read -r pattern; do
            [ -z "$pattern" ] && continue
            if mime_matches "$pattern" "$subject_type"; then
                printf '%s\n' "$verb"
                break
            fi
        done < <(jq -r '.accepts[]?' <<<"$verb")
    done < <(jq -c '.verbs[]' <<<"$(_verbs_registry)")
}

# Substitute {path.to.var} (and {path.to.var|filter}) placeholders in TEMPLATE
# with values from VARS_JSON.
#
# Filters:
#   |q  (alias |sh, |shell)  shell-quote via printf %q — safe as a bare argv
#                            token or `<<<` here-string, immune to embedded
#                            quotes/newlines/metacharacters.
#   |uri (alias |url)        percent-encode via jq @uri — safe inside a URL
#                            query string (single-quote the surrounding URL).
#   |raw (or no filter)      insert verbatim. The default; required for things
#                            like numeric ids and URL prefixes that must not be
#                            escaped.
_substitute() {
    local template=$1 vars_json=$2
    local result=""
    local rest=$template
    while [[ "$rest" =~ ^([^{]*)\{([a-zA-Z_][a-zA-Z0-9_.-]*)([|]([a-z]+))?\}(.*)$ ]]; do
        local before=${BASH_REMATCH[1]}
        local path=${BASH_REMATCH[2]}
        local filter=${BASH_REMATCH[4]}
        rest=${BASH_REMATCH[5]}
        local value
        value=$(jq -r ".${path} // empty" <<<"$vars_json" 2>/dev/null)
        case "$filter" in
            q|sh|shell) value=$(printf '%q' "$value") ;;
            uri|url)    value=$(printf '%s' "$value" | jq -sRr '@uri') ;;
            raw|"")     : ;;
            *)          : ;;  # unknown filter: leave raw rather than erroring
        esac
        result+="${before}${value}"
    done
    result+="$rest"
    printf '%s' "$result"
}

# Resolve adverbs for a verb. Echoes a JSON object:
#   { selected: {via: "clipboard", ...},
#     template_vars: {depth_prefix: "...", ...},
#     route_template: "<adverb-provided cmd template, or null if none>" }
_resolve_adverbs() {
    local verb_json=$1 adverbs_json=$2
    local registry
    registry=$(_verbs_registry)
    local uses_adverbs
    uses_adverbs=$(jq -c '.uses_adverbs // []' <<<"$verb_json")
    jq -nc \
        --argjson verb "$verb_json" \
        --argjson user_choices "$adverbs_json" \
        --argjson registry "$registry" \
        --argjson uses "$uses_adverbs" '
        # For each adverb the verb uses, find its definition in the registry.
        # Compute effective value: user override -> default. Collect template
        # variables and any route template provided by a selector adverb.
        reduce $uses[] as $aname (
            {selected: {}, template_vars: {}, route_template: null};
            ($registry.adverbs[] | select(.name == $aname)) as $adverb |
            ($user_choices[$aname] // $adverb.default) as $val |
            .selected[$aname] = $val |
            # If the adverb is a selector and has a chosen-value object with
            # `template_var`, merge those vars in.
            ( if ($adverb.kind // "selector") == "selector" and ($adverb.values[$val] // null) != null
              then .template_vars += ($adverb.values[$val].template_var // {})
              else . end ) |
            # First selector adverb that provides a `template` for the chosen
            # value gets to dictate the route template (e.g. via=clipboard).
            ( if .route_template == null and ($adverb.values[$val].template // null) != null
              then .route_template = $adverb.values[$val].template
              else . end )
        )
    '
}

# verb_apply VERB_JSON SUBJECT_JSON [OBJECT_JSON] [ADVERBS_JSON]
verb_apply() {
    local verb_json=$1 subject_json=$2 object_json=${3:-null} adverbs_json=${4:-'{}'}

    # 1. Validate subject type matches accepts.
    local subject_type
    subject_type=$(jq -r '.type // empty' <<<"$subject_json")
    if [ -n "$subject_type" ]; then
        local pattern matched=0
        while IFS= read -r pattern; do
            [ -z "$pattern" ] && continue
            if mime_matches "$pattern" "$subject_type"; then matched=1; break; fi
        done < <(jq -r '.accepts[]?' <<<"$verb_json")
        if [ "$matched" -eq 0 ]; then
            echo "verb_apply: subject type '$subject_type' does not match verb accepts" >&2
            return 1
        fi
    fi

    # 2. Validate object type if verb has object_type.
    local expected_object_type
    expected_object_type=$(jq -r '.object_type // empty' <<<"$verb_json")
    if [ -n "$expected_object_type" ]; then
        local got_object_type
        got_object_type=$(jq -r '.type // empty' <<<"$object_json")
        if [ -z "$got_object_type" ]; then
            echo "verb_apply: verb requires object of type '$expected_object_type'" >&2
            return 1
        fi
        if ! mime_matches "$expected_object_type" "$got_object_type"; then
            echo "verb_apply: object type '$got_object_type' does not match '$expected_object_type'" >&2
            return 1
        fi
    fi

    # 3+4. Resolve adverbs (selected values, injected template_vars, optional route template).
    local resolved
    resolved=$(_resolve_adverbs "$verb_json" "$adverbs_json")

    # 5. Build the substitution context. template_vars merge in at top level.
    local cwd=$PWD
    local context
    context=$(jq -nc \
        --argjson verb "$verb_json" \
        --argjson subject "$subject_json" \
        --argjson object "$object_json" \
        --argjson resolved "$resolved" \
        --arg cwd "$cwd" '
        {
            subject: $subject,
            object: $object,
            verb: $verb,
            adverbs: $resolved.selected,
            cwd: $cwd
        } + ($resolved.template_vars // {})
    ')

    # 6. Render verb.prompt with that context (so adverb routes can reference {verb.prompt}).
    local raw_prompt rendered_prompt
    raw_prompt=$(jq -r '.prompt // empty' <<<"$verb_json")
    if [ -n "$raw_prompt" ]; then
        rendered_prompt=$(_substitute "$raw_prompt" "$context")
        # Re-inject into context so {verb.prompt} now sees the rendered text.
        context=$(jq -nc \
            --argjson ctx "$context" \
            --arg rendered "$rendered_prompt" '
            $ctx | .verb.prompt = $rendered
        ')
    fi

    # 7. Pick the command:
    #    - if an adverb-supplied route template exists, use it (e.g. via=clipboard);
    #    - else if the verb has a cmd, use it;
    #    - else fail.
    local route_template raw_cmd
    route_template=$(jq -r '.route_template // empty' <<<"$resolved")
    raw_cmd=$(jq -r '.cmd // empty' <<<"$verb_json")

    local template
    if [ -n "$route_template" ]; then
        template=$route_template
    elif [ -n "$raw_cmd" ]; then
        template=$raw_cmd
    else
        echo "verb_apply: verb has neither cmd nor an adverb-routed template" >&2
        return 1
    fi

    # 8. Substitute and confirm.
    local rendered_cmd
    rendered_cmd=$(_substitute "$template" "$context")

    local confirm
    confirm=$(jq -r '.confirm // false' <<<"$verb_json")
    if [ "$confirm" = "true" ]; then
        echo "About to run: $rendered_cmd" >&2
        read -r -p "Proceed? [y/N] " ans
        case "$ans" in
            y|Y|yes|YES) ;;
            *) echo "verb_apply: cancelled" >&2; return 130 ;;
        esac
    fi

    # 9. Execute.
    bash -c "$rendered_cmd"
}
