#!/usr/bin/env bats
# Earned-hops depth control (negotiation §4.1). Auto-coercion is bounded: by
# default ≤1 converter hop per layer, so a deeper route is *earned* via `--hops N`
# (raise input-coercion depth) or `--force` (unbounded). A 1-hop coercion still
# runs with no flag — the default isn't "no coercion", it's "one hop".
#
# Fixture: a two-step chain text/plain →[up]→ text/x-up →[upup]→ text/x-upup, and
# a verb `up2` that only accepts the doubly-coerced type. Reaching it needs 2
# layer-A hops; the default 415s, `--hops 2`/`--force` succeed. A `rev1` verb
# accepting the once-coerced type is the 1-hop control.
#
# Rust-bin only (bash bin/goo has no negotiation executor). setup() auto-skips on
# any engine without it.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/run"
    HOME="$BATS_TEST_TMPDIR/home"
    export WAYLAND_DISPLAY="" DISPLAY=""   # deterministic: no display → byte sink
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"

    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/hops.toml" <<'EOF'
name = "hops"

# Present verb — the skip probe (and proves the engine executes negotiated verbs).
[[verbs]]
name = "show"
kind = "present"
accepts = ["text/*"]

# First coercion hop: text/* → text/x-up (uppercase).
[[channels]]
name = "up"
accepts = ["text/*"]
emits = "text/x-up"
cost = "cheap"
cmd = "tr a-z A-Z < {in.path|q}"

# Second coercion hop: text/x-up → text/x-upup (reverse).
[[channels]]
name = "upup"
accepts = ["text/x-up"]
emits = "text/x-upup"
cost = "cheap"
cmd = "rev < {in.path|q}"

# Needs the DOUBLY-coerced type → reachable from text/plain only via up + upup
# (2 layer-A hops). The default budget (1) can't reach it.
[[verbs]]
name = "up2"
accepts = ["text/x-upup"]
cmd = "cat {subject.metadata.path|q}"

# Needs the ONCE-coerced type → 1 layer-A hop, allowed by the default budget.
[[verbs]]
name = "rev1"
accepts = ["text/x-up"]
cmd = "cat {subject.metadata.path|q}"
EOF
    printf 'hello goo' > "$BATS_TEST_TMPDIR/sub.txt"

    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    echo "$output" | grep -q "hello" || skip "engine doesn't execute present verbs"
}

# Default budget (1 hop/layer): a 2-hop input coercion is NOT auto-taken → 415.
@test "hops: default bounds input coercion to one hop (2-hop route → 415)" {
    run "$GOO" up2 "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]
}

# --hops 2 raises the input-coercion budget → the chain runs (up then upup).
@test "hops: --hops 2 earns the second coercion hop" {
    run "$GOO" up2 "$BATS_TEST_TMPDIR/sub.txt" --hops 2 </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "OOG OLLEH" ]   # up: HELLO GOO → upup(rev): OOG OLLEH
}

# --force lifts the bound entirely → same result.
@test "hops: --force lifts the bound (unbounded coercion)" {
    run "$GOO" up2 "$BATS_TEST_TMPDIR/sub.txt" --force </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "OOG OLLEH" ]
}

# The default isn't "no coercion": a SINGLE hop still runs with no flag.
@test "hops: a single coercion hop runs by default" {
    run "$GOO" rev1 "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "HELLO GOO" ]   # up only (1 hop), then cat
}

# --explain mirrors the run budget: default 415s, --hops 2 shows the 2-hop route.
@test "hops: --explain honors the budget (415 at default, route at --hops 2)" {
    run "$GOO" --explain up2 "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"415"* ]]

    run "$GOO" --explain up2 "$BATS_TEST_TMPDIR/sub.txt" --hops 2 </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"up"* ]]
    [[ "$output" == *"upup"* ]]
    [[ "$output" == *"text/x-upup"* ]]
}
