# bash completion for goo. Source this file from your shell init, or symlink
# into /etc/bash_completion.d/ (system) or ~/.local/share/bash-completion/completions/ (user).

_goo() {
    local cur words cword
    _init_completion -n = 2>/dev/null || {
        # Fallback if bash-completion's _init_completion is unavailable.
        cur=${COMP_WORDS[COMP_CWORD]}
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
    }

    local first=${words[1]:-}

    # Position 1: subcommand or verb name.
    if [ "$cword" -eq 1 ]; then
        local cands
        cands=$(goo __complete subcommands 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$cands" -- "$cur"))
        return 0
    fi

    # Subcommands that take a single argument we can complete.
    case "$first" in
        list)
            local cands
            cands=$(goo __complete sources 2>/dev/null)
            # shellcheck disable=SC2207
            COMPREPLY=($(compgen -W "$cands" -- "$cur"))
            return 0
            ;;
        describe)
            local cands
            cands=$(goo __complete verbs 2>/dev/null)
            # shellcheck disable=SC2207
            COMPREPLY=($(compgen -W "$cands" -- "$cur"))
            return 0
            ;;
        plugins|validate|compose|help|--help|-h|__complete)
            return 0
            ;;
    esac

    # Otherwise $first is a verb name. Handle flag completion.
    if [[ "$cur" == --*=* ]]; then
        # --flag=<TAB> — complete adverb values.
        local flag=${cur%%=*}
        flag=${flag#--}
        local prefix=${cur#*=}
        local vals
        vals=$(goo __complete adverb-values "$flag" 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$vals" -- "$prefix"))
        # Re-prefix each candidate so the shell rewrites just the value half.
        local i
        for i in "${!COMPREPLY[@]}"; do
            COMPREPLY[i]="--${flag}=${COMPREPLY[i]}"
        done
        return 0
    fi

    if [[ "$cur" == --* ]]; then
        # --<TAB> — complete adverb names for this verb.
        local prefix=${cur#--}
        local adverbs
        adverbs=$(goo __complete adverbs "$first" 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$adverbs" -- "$prefix"))
        # Re-prefix so the user gets --name=
        local i
        for i in "${!COMPREPLY[@]}"; do
            COMPREPLY[i]="--${COMPREPLY[i]}="
        done
        compopt -o nospace 2>/dev/null
        return 0
    fi

    # @source:item — addressing sigil completion.
    if [[ "$cur" == @*:* ]]; then
        # @source:<TAB> — list items from that source.
        local srcpfx=${cur#@}
        srcpfx=${srcpfx%%:*}
        local q=${cur#*:}
        local items
        items=$(goo __complete source-items "$srcpfx" 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$items" -- "$q"))
        local i
        for i in "${!COMPREPLY[@]}"; do
            COMPREPLY[i]="@${srcpfx}:${COMPREPLY[i]}"
        done
        return 0
    fi
    if [[ "$cur" == @* ]]; then
        # @<TAB> — list source prefixes (@app:, @ws:, ...).
        local prefixes
        prefixes=$(goo __complete source-prefixes 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$prefixes" -- "$cur"))
        compopt -o nospace 2>/dev/null
        return 0
    fi

    # Bare positional after a verb: if the verb accepts a handle type, offer
    # items from the matching sources. For text verbs we don't complete (the
    # subject is freeform prose / a path / stdin).
    local accepts_handle
    accepts_handle=$(goo __complete verb-accepts-handle "$first" 2>/dev/null)
    if [ "$accepts_handle" = "yes" ]; then
        local items
        items=$(goo __complete verb-subject-items "$first" 2>/dev/null)
        # shellcheck disable=SC2207
        COMPREPLY=($(compgen -W "$items" -- "$cur"))
        return 0
    fi
    return 0
}

complete -F _goo goo
