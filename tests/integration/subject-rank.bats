#!/usr/bin/env bats
# Subject-shape-aware listing (slice #5 / data-entry-ux.md §5.1). The
# `verb-subject-items` completion stage ranks subject candidates by
# accepts-specificity and unions a polymorphic verb's impls. Rust-only ranking
# (bash bin/goo lists in declaration order, first-impl-only), so this file
# auto-skips on the bash legacy engine via the `what`-subcommand probe.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"

    # Ranking fixture uses a genuine exact-vs-glob specificity GAP (not a
    # prefix-length tie): `launch` accepts text/x-uri (exact) AND inode/* (glob).
    #   links (emits text/x-uri)  → exact match  → ranks FIRST
    #   files (emits inode/file)  → inode/* glob → ranks SECOND
    # `files` also carries id `alink` (shared with links) to prove dedupe.
    #
    # Polymorphic-union fixture: two impls of `act`, each accepting a different
    # type; `srcB`'s item is reachable ONLY via the SECOND impl's accepts — the
    # case a single-impl lookup misses.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/rank.toml" <<'EOF'
name = "rank"

[[sources]]
name = "links"
prefix = "lnk"
emits = "text/x-uri"
list_cmd = "echo '[{\"id\":\"alink\",\"title\":\"A Link\"}]'"

[[sources]]
name = "files"
prefix = "fil"
emits = "inode/file"
list_cmd = "echo '[{\"id\":\"zfile\",\"title\":\"Z File\"},{\"id\":\"alink\",\"title\":\"dup id\"}]'"

[[sources]]
name = "srca"
prefix = "sra"
emits = "application/vnd.t.aaa"
list_cmd = "echo '[{\"id\":\"fromA\",\"title\":\"From A\"}]'"

[[sources]]
name = "srcb"
prefix = "srb"
emits = "application/vnd.t.bbb"
list_cmd = "echo '[{\"id\":\"fromB\",\"title\":\"From B\"}]'"

[[verbs]]
name = "launch"
accepts = ["inode/*", "text/x-uri"]
cmd = "true"

[[verbs]]
name = "act"
accepts = ["application/vnd.t.aaa"]
cmd = "true"

[[verbs]]
name = "act"
accepts = ["application/vnd.t.bbb"]
cmd = "true"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no subject-shape ranking (bash legacy)"
    fi
}

@test "subject-rank: exact-accept source ranks above glob-accept source" {
    # text/x-uri (exact) beats inode/* (glob) ⇒ links' item leads files' items.
    run "$GOO" __complete verb-subject-items launch </dev/null
    [ "$status" -eq 0 ]
    [ "$(printf '%s\n' "$output" | head -1)" = "alink" ]
    # zfile (from the lower-ranked files source) appears AFTER alink.
    local alink_line zfile_line
    alink_line=$(printf '%s\n' "$output" | grep -nx "alink" | head -1 | cut -d: -f1)
    zfile_line=$(printf '%s\n' "$output" | grep -nx "zfile" | cut -d: -f1)
    [ "$alink_line" -lt "$zfile_line" ]
}

@test "subject-rank: a shared id is listed once (dedupe, first-seen rank wins)" {
    run "$GOO" __complete verb-subject-items launch </dev/null
    [ "$status" -eq 0 ]
    [ "$(printf '%s\n' "$output" | grep -cx "alink")" -eq 1 ]
}

@test "subject-rank: polymorphic verb unions all impls' accepts" {
    # `fromB` is reachable ONLY via the SECOND `act` impl (accepts vnd.t.bbb).
    # A single-impl lookup would miss it; the union surfaces it.
    run "$GOO" __complete verb-subject-items act </dev/null
    [ "$status" -eq 0 ]
    echo "$output" | grep -qx "fromA"
    echo "$output" | grep -qx "fromB"
}
