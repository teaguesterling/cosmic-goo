# shellcheck shell=bash
# lib/plugin-loader.sh — plugin discovery, parsing, and registry assembly.
#
# Source this file; do not exec it. Depends on lib/toml.sh.
#
# Functions:
#   plugin_dirs                   Echo plugin search dirs, lowest to highest precedence.
#   plugin_discover               Echo paths of all plugin TOML files in precedence order.
#   plugin_load FILE              Echo JSON for one plugin file (with provenance fields).
#   plugin_load_all               Echo JSON registry assembled from all discovered plugins.
#   plugin_registry_export        Alias of plugin_load_all (named per the implementation plan).
#
# Registry shape:
#   { "plugins":[{name,dir,file,description}],
#     "types":   [item, ...],
#     "sources": [item, ...],
#     "verbs":   [item, ...],
#     "adverbs": [item, ...] }
#
# Each contributed item gains `_plugin` (name of contributing plugin) and
# `_plugin_dir` (its directory, so relative cmd paths can resolve).
#
# Override-by-name: when two plugins contribute items with the same `name`,
# the later-loaded one wins. Precedence order (lowest first):
#   1. COSMIC_GOO_BUILTIN_PLUGINS_DIR (defaults to /usr/share/cosmic-goo/plugins)
#   2. /etc/cosmic-goo/plugins
#   3. ${XDG_CONFIG_HOME:-$HOME/.config}/cosmic-goo/plugins
#   4. $PWD/.cosmic-goo/plugins (project-local)

if [ "${BASH_SOURCE[0]}" = "${0}" ]; then
    echo "lib/plugin-loader.sh: source this file, do not exec it" >&2
    exit 1
fi

# Resolve our own dir to find sibling lib/toml.sh.
_pl_dir() {
    local src=${BASH_SOURCE[0]}
    cd "$(dirname -- "$src")" && pwd
}

# Source toml.sh once.
if ! declare -F toml_get >/dev/null 2>&1; then
    _toml_sh="$(_pl_dir)/toml.sh"
    if [ ! -f "$_toml_sh" ]; then
        echo "plugin-loader: lib/toml.sh not found at $_toml_sh" >&2
        return 1
    fi
    # shellcheck source=SCRIPTDIR/toml.sh
    . "$_toml_sh"
    unset _toml_sh
fi

plugin_dirs() {
    printf '%s\n' \
        "${COSMIC_GOO_BUILTIN_PLUGINS_DIR:-/usr/share/cosmic-goo/plugins}" \
        "/etc/cosmic-goo/plugins" \
        "${XDG_CONFIG_HOME:-$HOME/.config}/cosmic-goo/plugins" \
        "$PWD/.cosmic-goo/plugins"
}

plugin_discover() {
    local d f sub
    while IFS= read -r d; do
        [ -d "$d" ] || continue
        # Single-file plugins: plugins/<name>.toml
        for f in "$d"/*.toml; do
            [ -f "$f" ] || continue
            printf '%s\n' "$f"
        done
        # Directory plugins: plugins/<name>/plugin.toml
        for sub in "$d"/*/; do
            [ -f "${sub}plugin.toml" ] || continue
            printf '%s\n' "${sub}plugin.toml"
        done
    done < <(plugin_dirs)
}

# Echo JSON for one plugin file, with provenance fields added to each item.
plugin_load() {
    local file=$1
    if [ ! -f "$file" ]; then
        echo "plugin_load: not a file: $file" >&2
        return 1
    fi
    local plugin_json plugin_dir
    plugin_dir=$(cd "$(dirname -- "$file")" && pwd)
    if ! plugin_json=$(toml_get "$file" '.'); then
        echo "plugin_load: failed to parse $file" >&2
        return 1
    fi
    jq -c --arg dir "$plugin_dir" --arg file "$file" '
        # Fall back to the file basename (sans .toml) if the plugin omits `name`.
        ( .name // ($file | split("/")[-1] | sub("\\.toml$"; "")) ) as $pname |
        {
            plugins: [{
                name: $pname,
                dir: $dir,
                file: $file,
                description: (.description // null)
            }],
            types:   ((.types   // []) | map(. + {_plugin: $pname, _plugin_dir: $dir})),
            sources: ((.sources // []) | map(. + {_plugin: $pname, _plugin_dir: $dir})),
            verbs:   ((.verbs   // []) | map(. + {_plugin: $pname, _plugin_dir: $dir})),
            adverbs: ((.adverbs // []) | map(. + {_plugin: $pname, _plugin_dir: $dir}))
        }
    ' <<<"$plugin_json"
}

# Merge a plugin contribution into a running registry. Later (=new) wins on `.name`.
_plugin_merge() {
    local registry=$1 contrib=$2
    jq -nc --argjson reg "$registry" --argjson new "$contrib" '
        # `unique_by` keeps the first occurrence; put new ahead of existing so
        # the new item wins for collisions.
        def override($a; $b): ($a + $b) | unique_by(.name);
        {
            plugins: override($new.plugins; $reg.plugins),
            types:   override($new.types;   $reg.types),
            sources: override($new.sources; $reg.sources),
            verbs:   override($new.verbs;   $reg.verbs),
            adverbs: override($new.adverbs; $reg.adverbs)
        }
    '
}

plugin_load_all() {
    local registry='{"plugins":[],"types":[],"sources":[],"verbs":[],"adverbs":[]}'
    local file contrib
    while IFS= read -r file; do
        if contrib=$(plugin_load "$file"); then
            registry=$(_plugin_merge "$registry" "$contrib")
        fi
    done < <(plugin_discover)
    printf '%s\n' "$registry"
}

plugin_registry_export() {
    plugin_load_all
}
