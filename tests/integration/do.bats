#!/usr/bin/env bats
# Noun-first dispatch: `goo do <addr> [verb] [args…]` (data-entry-ux §4.3 / #15).
# The CLI stays verb-first by default; `do` is the explicit inversion for the
# discovery mood. With a verb it's a pure reorder of `goo <verb> <addr> [args]`
# (re-enters cmd_verb verbatim — subject/object/adverb/history all ride on it);
# with no verb it's the verb-pick (the same listing `goo what` prints).
# Rust-only (cmd_verb/negotiation engine); skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    export XDG_STATE_HOME="$BATS_TEST_TMPDIR/state"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$XDG_STATE_HOME" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/d.toml" <<'EOF'
name = "d"

[[types]]
name = "application/vnd.t.thing"
kind = "handle"

[[sources]]
name = "things"
prefix = "thg"
emits = "application/vnd.t.thing"
list_cmd = "echo '[{\"id\":\"a\",\"title\":\"A\"}]'"

# Echoes the subject id AND a selector-adverb-injected var, so a flag passed
# AFTER the verb (`do <addr> <verb> --tone=…`) is observable in the output.
[[verbs]]
name = "inspect"
accepts = ["application/vnd.t.thing"]
uses_adverbs = ["tone"]
cmd = "echo inspected {subject.id} {tone_var}"

[[verbs]]
name = "poke"
accepts = ["application/vnd.t.thing"]
cmd = "echo poked {subject.id}"

[[adverbs]]
name = "tone"
kind = "selector"
applies_to_verbs = ["inspect"]
default = "soft"
description = "How to inspect"

[adverbs.values.soft]
template_var = { tone_var = "soft" }

[adverbs.values.loud]
template_var = { tone_var = "loud" }
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    echo "$output" | grep -q "what" || skip "engine has no noun-first dispatch (bash legacy)"
}

@test "do: with a verb, runs it on the address (the reorder)" {
    run "$GOO" do :thg/a inspect </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "inspected a soft" ]]
}

@test "do <addr> <verb> is identical to <verb> <addr> — stdout AND history" {
    # The load-bearing equivalence: `do` must fork nothing. Same exit, same
    # stdout, same recorded action. Run each in a clean state dir and diff.
    s1="$XDG_STATE_HOME/cosmic-goo"; rm -rf "$s1"
    run "$GOO" do :thg/a inspect </dev/null
    do_out="$output"; do_status="$status"
    do_hist="$(cat "$s1/history.jsonl" 2>/dev/null)"

    rm -rf "$s1"
    run "$GOO" inspect :thg/a </dev/null
    verb_out="$output"; verb_status="$status"
    verb_hist="$(cat "$s1/history.jsonl" 2>/dev/null)"

    [ "$do_status" -eq "$verb_status" ]
    [ "$do_out" = "$verb_out" ]
    [ -n "$do_hist" ]                       # something WAS recorded
    [ "$do_hist" = "$verb_hist" ]           # …and it's the same action
}

@test "do: flags after the verb ride through the reorder" {
    # `--tone=loud` sits after the verb; it must reach the verb's adverbs.
    run "$GOO" do :thg/a inspect --tone=loud </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "inspected a loud" ]]
    # …and identically to the verb-first spelling.
    run "$GOO" inspect :thg/a --tone=loud </dev/null
    [[ "$output" =~ "inspected a loud" ]]
}

@test "do: with no verb, lists the applicable verbs (the verb-pick)" {
    run "$GOO" do :thg/a </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "applicable verbs for :thg/a" ]]
    [[ "$output" =~ "inspect" ]]
    [[ "$output" =~ "poke" ]]
}

@test "do: the no-verb listing matches 'goo what' (shared SSOT)" {
    run "$GOO" do :thg/a </dev/null
    do_out="$output"
    run "$GOO" what :thg/a </dev/null
    [ "$do_out" = "$output" ]
}

@test "do: no address is a usage error" {
    run "$GOO" do </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "usage: goo do" ]]
}

@test "do: appears in the subcommand completion list" {
    run "$GOO" __complete subcommands </dev/null
    echo "$output" | grep -qx "do"
}
