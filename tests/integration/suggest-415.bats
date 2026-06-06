#!/usr/bin/env bats
# Conversion suggestions on a 415 (data-entry-ux §6.8 / roadmap #14). When a verb
# can't be routed to a subject (no coercion path → 415), goo now also names the
# verbs that accept the subject's type DIRECTLY — running one of those won't 415.
# Drawn from OPTIONS.allow (the same SSOT `goo what` shows), minus the failed verb
# and any destructive verb. Rust-only (the negotiation/415 engine); skips on bash.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    export HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/s415.toml" <<'EOF'
name = "s415"

[[types]]
name = "application/vnd.t.thing"
kind = "handle"

[[types]]
name = "application/vnd.t.orphan"
kind = "handle"

[[sources]]
name = "things"
prefix = "thg"
emits = "application/vnd.t.thing"
list_cmd = "echo '[{\"id\":\"a\",\"title\":\"A\"}]'"

[[sources]]
name = "orphans"
prefix = "orp"
emits = "application/vnd.t.orphan"
list_cmd = "echo '[{\"id\":\"a\",\"title\":\"A\"}]'"

# Accepts a type the subjects ISN'T, with no channel to it → always 415s.
[[verbs]]
name = "render"
accepts = ["image/x-special"]
cmd = "true"

# A `present` verb that DOES accept the thing type — so it's in OPTIONS.allow.
# Forced to an unreachable Accept (`--as image/x-special`) it 415s on the OUTPUT
# side. This is the case where the failed verb is itself a candidate, so the
# `n != failed_verb` filter actually has work to do (it must drop `view`).
[[verbs]]
name = "view"
kind = "present"
accepts = ["application/vnd.t.thing"]

# These accept the thing type directly → the safe alternatives.
[[verbs]]
name = "inspect"
accepts = ["application/vnd.t.thing"]
cmd = "true"

[[verbs]]
name = "poke"
accepts = ["application/vnd.t.thing"]
cmd = "true"

# Destructive — must NOT be suggested as an alternative.
[[verbs]]
name = "destroy"
accepts = ["application/vnd.t.thing"]
destructive = true
cmd = "true"
EOF
    cd "$BATS_TEST_TMPDIR" || return 1

    run "$GOO" __complete subcommands </dev/null
    echo "$output" | grep -q "what" || skip "engine has no negotiation/415 teaching (bash legacy)"
}

@test "415: suggests verbs that accept the subject's type" {
    run "$GOO" render :thg/a </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "415" ]]
    [[ "$output" =~ "try a verb that accepts application/vnd.t.thing: view, inspect, poke" ]]
}

@test "415: the failed verb and destructive verbs are not suggested" {
    run "$GOO" render :thg/a </dev/null
    [[ ! "$output" =~ "destroy" ]]   # destructive alternative withheld
    # 'render' is the failed verb — it appears in the error head ("through
    # 'render'") but must NOT appear in the suggestion list. Isolate that line
    # and assert it's render-free (the head-mention would mask a real leak).
    suggest_line="$(printf '%s\n' "$output" | grep 'try a verb that accepts')"
    [ -n "$suggest_line" ]                  # the suggestion line exists
    [[ ! "$suggest_line" =~ "render" ]]     # …and render is not in it
}

@test "415: a present-verb that 415s on output is excluded from its own suggestions" {
    # `view` accepts thing (so it's a candidate) but --as forces an Accept it
    # can't reach → 415. The hint must name the OTHER thing-accepting verbs and
    # NOT re-suggest `view` itself — the one case the failed-verb filter matters.
    run "$GOO" view :thg/a --as image/x-special </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "415" ]]
    [[ "$output" =~ "through 'view'" ]]                  # view is the failed verb
    suggest_line="$(printf '%s\n' "$output" | grep 'try a verb that accepts')"
    [[ "$suggest_line" =~ "inspect" ]]
    [[ "$suggest_line" =~ "poke" ]]
    [[ ! "$suggest_line" =~ "view" ]]                    # …and not re-suggested
    [[ ! "$suggest_line" =~ "destroy" ]]                 # destructive still withheld
}

@test "415: no suggestion line when nothing safely accepts the type" {
    # :orp/a has type vnd.t.orphan, which NO verb accepts → no alternatives.
    run "$GOO" render :orp/a </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" =~ "415" ]]
    [[ ! "$output" =~ "try a verb that accepts" ]]
}
