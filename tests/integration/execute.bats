#!/usr/bin/env bats
# Present-verb execution: `goo <present-verb> <subject>` plans the route to the
# environment's Accept and *runs* it through the negotiation executor (the
# executor driving the renderers). Uses a fixture with real commands (tr) so the
# pipeline runs deterministically; display vars are cleared so the environment is
# a plain byte sink (piped) unless `--as` pins the Accept.
#
# Rust-bin only (bash bin/goo has no negotiation executor — a present verb with
# no cmd errors in render). setup() auto-skips on any engine without it.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    export WAYLAND_DISPLAY="" DISPLAY=""   # deterministic: no display → byte sink
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/efix.toml" <<'EOF'
name = "efix"

[[verbs]]
name = "show"
kind = "present"
accepts = ["text/*"]

# A real verb that accepts text/x-up — reachable from a text/plain subject only
# via the `up` channel (input coercion).
[[verbs]]
name = "revit"
accepts = ["text/x-up"]
emits = "text/x-rev"
cmd = "rev < {subject.metadata.path|q}"

# A real verb that accepts the subject directly — exercises the legacy path
# (no gap → no negotiation).
[[verbs]]
name = "echo-it"
accepts = ["text/plain"]
cmd = "cat {subject.metadata.path|q}"

[[channels]]
name = "up"
accepts = ["text/*"]
emits = "text/x-up"
cost = "cheap"
cmd = "tr a-z A-Z < {in.path|q}"
EOF
    printf 'hello goo' > "$BATS_TEST_TMPDIR/sub.txt"

    # Skip unless this engine executes present verbs (bash errors in render).
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    echo "$output" | grep -q "hello" || skip "engine doesn't execute present verbs"
}

@test "execute: present verb delivers the subject (identity, byte sink)" {
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hello goo" ]
}

@test "execute: --as routes through a renderer (text → text/x-up via up)" {
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" --as=text/x-up </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "HELLO GOO" ]
}

@test "execute: --as with no reachable representation → 415" {
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" --as=image/png </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]
}

# 4b: a real verb whose accepts the subject doesn't satisfy → input coercion
# (the `up` channel) then the verb runs.
@test "execute: real verb coerces its input (text → up → revit)" {
    run "$GOO" revit "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "OOG OLLEH" ]   # up: HELLO GOO → rev: OOG OLLEH
}

# 4b: no gap (subject already accepted) → unchanged legacy render+exec path.
@test "execute: no type gap runs the legacy path unchanged" {
    run "$GOO" echo-it "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hello goo" ]
}

# 4b: a type gap with no coercion route → clean 415 (not the verb's own error).
@test "execute: real-verb gap with no route → 415" {
    printf '{"k":1}' > "$BATS_TEST_TMPDIR/d.json"   # application/json, no path to text/x-up here
    run "$GOO" revit "$BATS_TEST_TMPDIR/d.json" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]
}
