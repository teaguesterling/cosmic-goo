#!/usr/bin/env bats
# An untrusted subject field that reaches a verb's bash `cmd` must be `|q`-quoted, never
# hand-wrapped in '…'. A file whose NAME contains a single quote would otherwise break
# out of the manual quotes and inject commands — and goo's whole pitch is "point at any
# directory," including a hostile cloned repo. Regression for the files.toml file verbs
# (read/preview/copy-path/reveal) and the tmux verbs, which used '{subject.…}' before.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    cd "$BATS_TEST_TMPDIR" || return 1
}

@test "injection: a single-quote in a filename cannot inject via read" {
    mkdir -p d
    fn="x';touch PWNED;'.txt"             # break out of cat '…', then ; touch
    printf 'safe contents\n' > "d/$fn"
    run "$GOO" read "./d/$fn" </dev/null
    [ ! -e PWNED ]                        # the injected `touch PWNED` did NOT run
    [[ "$output" == *"safe contents"* ]]  # …and the file was actually read
}

@test "injection: preview is quote-safe too" {
    mkdir -p d
    fn="y';touch PWNED2;'.md"
    printf 'first line\n' > "d/$fn"
    run "$GOO" preview "./d/$fn" </dev/null
    [ ! -e PWNED2 ]
    [[ "$output" == *"first line"* ]]
}

# Structural guard for the WHOLE class (the runtime tests above cover read/preview;
# this covers every cmd/run/list_cmd in every shipped plugin, incl. side-effecting
# verbs like copy-path/reveal/tmux that are unsafe to run). An untrusted
# {subject|object|in}.field reaching bash must go through |q/|uri/|sh — never bare
# (word-split/inject) and never hand-quoted ('{x}', the read RCE). Block-aware so
# multi-line ('''…''') cmds are covered. Whitelist = cos-cli integer indices
# (compositor state, not attacker-controllable). The by-construction home for this is
# a future `goo validate` lint (see doc/design/facet-trust-boundary.md).
@test "lint: untrusted subject/object/in fields in shipped cmds are |q-quoted (no bare, no hand-quote)" {
    cat > "$BATS_TEST_TMPDIR/qlint.awk" <<'AWK'
function scan(line,   s) {
  while (match(line, /\{(subject|object|in)\.[a-z0-9._]+\}/)) {
    s = substr(line, RSTART, RLENGTH); line = substr(line, RSTART + RLENGTH)
    if (s=="{subject.metadata.index}"||s=="{subject.metadata.group_index}"||s=="{object.metadata.index}"||s=="{object.metadata.group_index}") continue
    print FILENAME ":" FNR ": " s
  }
}
BEGIN { inblk=0 }
{
  if (inblk) { scan($0); if ($0 ~ /\047\047\047/) inblk=0; next }
  if ($0 ~ /^[[:space:]]*(cmd|run|list_cmd|values_cmd|object_list_cmd)[[:space:]]*=/) {
    scan($0); cp=$0; if (gsub(/\047\047\047/, "X", cp)==1) inblk=1
  }
}
AWK
    run awk -f "$BATS_TEST_TMPDIR/qlint.awk" "$REPO_ROOT"/plugins/*.toml "$REPO_ROOT"/crates/goo-engine/core.toml
    [ "$status" -eq 0 ]
    # awk prints `file:line: {field}` per violation; output must be empty.
    [ -z "$output" ] || { echo "Unquoted untrusted substitutions (use |q / |uri / |sh):"; echo "$output"; false; }
}
