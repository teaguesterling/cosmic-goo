#!/usr/bin/env bats
# Confirm gate UX (cache-staleness batch, items #2/#3). A confirm/destructive
# verb shows a friendly prompt (description + chip + subject, raw command
# secondary) and gates on y/N. `--confirm-dangerous=v1,v2` is a scoped,
# per-invocation pre-approval — it suppresses the prompt ONLY for the named
# verbs; anything else still prompts. Rust-only (new flag + prompt format), so
# this auto-skips on the bash legacy engine.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    : > "$HOME/marker"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cf.toml" <<'EOF'
name = "confirm-fixture"

[[sources]]
name = "boxes"
prefix = "box"
emits = "application/vnd.cf.box"
list_cmd = "echo '[{\"id\":\"alpha\",\"title\":\"Alpha\"}]'"

[[verbs]]
name = "zap"
accepts = ["application/vnd.cf.box"]
description = "Zap the box"
confirm = true
destructive = true
cmd = "printf 'zapped %s' {subject.id|q} > \"$HOME/marker\""

[[verbs]]
name = "poke"
accepts = ["application/vnd.cf.box"]
description = "Poke the box"
confirm = true
cmd = "printf 'poked %s' {subject.id|q} > \"$HOME/marker\""
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    if ! echo "$output" | grep -q "what"; then
        skip "engine has no scoped-confirm UX (bash legacy)"
    fi
}

@test "confirm: destructive verb shows a friendly prompt + [!!] chip; EOF cancels" {
    run "$GOO" zap :box/alpha </dev/null
    [ "$status" -eq 130 ]
    [[ "$output" =~ "about to Zap the box [!!]" ]]
    [[ "$output" =~ "subject: alpha" ]]
    [[ "$output" =~ "proceed? [y/N]" ]]
    [ -z "$(cat "$HOME/marker")" ]   # never ran
}

@test "confirm: confirm-only verb shows the [!] chip (not [!!])" {
    run "$GOO" poke :box/alpha </dev/null
    [ "$status" -eq 130 ]
    [[ "$output" =~ "about to Poke the box [!]" ]]
    [[ ! "$output" =~ "[!!]" ]]
}

@test "confirm: --confirm-dangerous=<verb> pre-approves just that verb (runs, loud note)" {
    run "$GOO" zap :box/alpha --confirm-dangerous=zap </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "auto-approving 'zap'" ]]
    [ "$(cat "$HOME/marker")" = "zapped alpha" ]
}

@test "confirm: --confirm-dangerous is scoped — an unlisted verb still prompts" {
    run "$GOO" poke :box/alpha --confirm-dangerous=zap </dev/null
    [ "$status" -eq 130 ]
    [[ "$output" =~ "proceed? [y/N]" ]]
    [ -z "$(cat "$HOME/marker")" ]
}

@test "confirm: a typo in --confirm-dangerous is flagged (not a confirm/destructive verb)" {
    run "$GOO" zap :box/alpha --confirm-dangerous=ztap </dev/null
    [[ "$output" =~ "'ztap' is not a confirm/destructive verb" ]]
    # ...and the real verb still gates (typo did not pre-approve it).
    [ "$status" -eq 130 ]
}
