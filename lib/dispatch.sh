#!/usr/bin/env bash
# lib/dispatch.sh — content-dispatch rule table (#32), a plumber-style classifier.
#
# A [[dispatch]] rule classifies raw text by an ERE `matches` pattern and routes
# it to a verb, optionally rewriting the subject with the regex captures:
#
#   [[dispatch]]
#   matches = 'RFC:?[[:space:]]*([0-9]+)'        # POSIX ERE; captures -> ${1}..
#   type    = "text/x-uri"                        # type assigned to the subject
#   set     = { text = "https://www.rfc-editor.org/rfc/rfc${1}.txt" }
#   verb    = "open-url"                          # verb to dispatch to
#   adverbs = { engine = "google" }              # optional adverb seed
#
# Rules are tried in load order; first `matches` wins; matching is single-shot
# (a rewritten subject is NOT re-fed through the table). `${N}` interpolates
# capture N (0 = whole match) into any `set` value (nested too) before dispatch.
#
# Regex is bash ERE, so use POSIX classes: \s -> [[:space:]], \d -> [[:digit:]],
# \w -> [[:alnum:]_].

# Build a JSON array of the current BASH_REMATCH captures (index 0 = full match).
_dispatch_caps_json() {
    local c out=()
    for c in "${BASH_REMATCH[@]}"; do
        out+=("$c")
    done
    printf '%s\n' "${out[@]}" | jq -R . | jq -sc .
}

# Render a matched rule into a dispatch descriptor:
#   { verb, type, adverbs, fields }
# `fields` is the rule's `set` map with every string value interpolated.
_dispatch_render() {
    local rule=$1 caps_json=$2
    jq -nc --argjson rule "$rule" --argjson caps "$caps_json" '
        def interp: gsub("\\$\\{(?<n>[0-9]+)\\}"; ($caps[(.n|tonumber)] // ""));
        def deepinterp: walk(if type == "string" then interp else . end);
        {
            verb:    $rule.verb,
            type:    ($rule.type // null),
            adverbs: (($rule.adverbs // {}) | deepinterp),
            fields:  (($rule.set // {}) | deepinterp)
        }
    '
}

# dispatch_match TEXT
# Echo the dispatch descriptor for the first rule whose `matches` ERE matches
# TEXT; return 1 if no rule matches.
dispatch_match() {
    local text=$1 registry
    registry=$(plugin_load_all 2>/dev/null) || return 1
    local n i rule pattern
    n=$(jq '.dispatch | length' <<<"$registry")
    for (( i = 0; i < n; i++ )); do
        rule=$(jq -c ".dispatch[$i]" <<<"$registry")
        pattern=$(jq -r '.matches // empty' <<<"$rule")
        [ -z "$pattern" ] && continue
        if [[ "$text" =~ $pattern ]]; then
            _dispatch_render "$rule" "$(_dispatch_caps_json)"
            return 0
        fi
    done
    return 1
}
