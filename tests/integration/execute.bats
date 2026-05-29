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

# A usage verb (no own cmd) implemented by a channel — exercises 2b: the
# planner picks the channel, the executor runs its cmd in the verb's context.
[[verbs]]
name = "yell"
accepts = ["text/*"]
suffix = "!!!"
usage = ["shout"]

[[channels]]
name = "shout"
accepts = ["text/*"]
emits = "text/x-shout"
cost = "normal"
cmd = "printf '%s%s' {subject.text|q} {verb.suffix|q}"

# A verb with two usage channels — exercises #1 (--using pins one). A usage
# channel reads {subject.*} (verb context), not {in.path}.
[[verbs]]
name = "say"
accepts = ["text/*"]
usage = ["loud", "quiet"]

[[channels]]
name = "loud"
accepts = ["text/*"]
emits = "text/x-said"
cost = "cheap"
cmd = "tr a-z A-Z < {subject.metadata.path|q}"

[[channels]]
name = "quiet"
accepts = ["text/*"]
emits = "text/x-said"
cost = "normal"
cmd = "tr A-Z a-z < {subject.metadata.path|q}"

# Emits two raw bytes (0xFF 0xFE) that are NOT valid UTF-8 — to prove --to routes
# binary intact (a String/utf8-lossy capture would inflate them to 6 bytes).
[[verbs]]
name = "rawbytes"
accepts = ["text/*"]
cmd = 'printf "\377\376"'
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

# 2b: a usage verb (no own cmd) runs via its chosen channel, in verb context
# ({subject.text} + {verb.suffix} both resolve).
@test "execute: usage verb runs its channel (multi-instrument execution)" {
    printf 'hi' > "$BATS_TEST_TMPDIR/y.txt"
    run "$GOO" yell "$BATS_TEST_TMPDIR/y.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hi!!!" ]
}

# 1: the planner picks the cheapest usage channel by default; --using pins one.
@test "execute: --using pins the usage channel (overrides the planner)" {
    printf 'Hi' > "$BATS_TEST_TMPDIR/s.txt"
    run "$GOO" say "$BATS_TEST_TMPDIR/s.txt" </dev/null          # cheapest = loud
    [ "$status" -eq 0 ]
    [ "$output" = "HI" ]
    run "$GOO" say "$BATS_TEST_TMPDIR/s.txt" --using=quiet </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "hi" ]
}

@test "execute: --using with a non-channel fails cleanly" {
    printf 'Hi' > "$BATS_TEST_TMPDIR/s.txt"
    run "$GOO" say "$BATS_TEST_TMPDIR/s.txt" --using=bogus </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"not a channel of 'say'"* ]]
}

# --- output routing: --to / -o (file + clipboard) ---

# legacy path (echo-it: own cmd, no gap): --to captures + writes to the file.
@test "execute: --to <file> routes a verb result to a file (legacy path)" {
    run "$GOO" echo-it "$BATS_TEST_TMPDIR/sub.txt" --to "$BATS_TEST_TMPDIR/out.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "" ]                                   # nothing on stdout
    [ "$(cat "$BATS_TEST_TMPDIR/out.txt")" = "hello goo" ]
}

# -o is sugar for --to a file.
@test "execute: -o <file> is sugar for --to a file" {
    run "$GOO" echo-it "$BATS_TEST_TMPDIR/sub.txt" -o "$BATS_TEST_TMPDIR/o.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/o.txt")" = "hello goo" ]
}

# present/negotiated path + the Accept-piped pin: --to delivers the SUBJECT bytes
# (identity, byte sink), not a rendered surface.
@test "execute: --to delivers piped bytes from a present verb (identity)" {
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" --to "$BATS_TEST_TMPDIR/p.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/p.txt")" = "hello goo" ]
}

# --as still composes with --to: route through `up`, then write the result.
@test "execute: --as routes through a converter, --to writes the result" {
    run "$GOO" show "$BATS_TEST_TMPDIR/sub.txt" --as=text/x-up --to "$BATS_TEST_TMPDIR/u.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/u.txt")" = "HELLO GOO" ]
}

# a non-writable destination errors cleanly (not a panic / not silent).
@test "execute: --to a non-writable destination fails cleanly" {
    run "$GOO" echo-it "$BATS_TEST_TMPDIR/sub.txt" --to 'goo://text/nope' </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"not writable"* ]]
}

# --using (instrument) and --to (destination) are orthogonal slots — they compose.
@test "execute: --using + --to compose (instrument picks channel, --to routes)" {
    printf 'Hi' > "$BATS_TEST_TMPDIR/s2.txt"
    run "$GOO" say "$BATS_TEST_TMPDIR/s2.txt" --using=quiet --to "$BATS_TEST_TMPDIR/q.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$(cat "$BATS_TEST_TMPDIR/q.txt")" = "hi" ]   # quiet = lowercase, then routed to the file
}

# wart #2: piped stdout delivers bytes even when $WAYLAND_DISPLAY is set — the
# coercion path must NOT route to a surface when someone's reading the pipe.
@test "execute: piped stdout delivers bytes even with a display set" {
    run env WAYLAND_DISPLAY=wayland-test DISPLAY=:99 "$GOO" revit "$BATS_TEST_TMPDIR/sub.txt" </dev/null
    [ "$status" -eq 0 ]
    [ "$output" = "OOG OLLEH" ]   # coerced text → up → rev, delivered to the pipe (not a surface)
}

# bytes mode: --to routes BINARY intact (no utf8-lossy inflation).
@test "execute: --to preserves raw binary bytes (no utf8 corruption)" {
    run "$GOO" rawbytes "$BATS_TEST_TMPDIR/sub.txt" --to "$BATS_TEST_TMPDIR/raw.bin" </dev/null
    [ "$status" -eq 0 ]
    [ "$(wc -c < "$BATS_TEST_TMPDIR/raw.bin")" -eq 2 ]   # 2 raw bytes, not 6 (lossy would inflate)
}

# clipboard destination (needs wl-copy + a compositor — tool-aware skip).
@test "execute: --to ^ writes the result to the clipboard" {
    command -v wl-copy >/dev/null || skip "wl-copy not installed"
    command -v wl-paste >/dev/null || skip "wl-paste not installed"
    run "$GOO" echo-it "$BATS_TEST_TMPDIR/sub.txt" --to '^' </dev/null
    [ "$status" -eq 0 ] || skip "no wayland clipboard in this env"
    [ "$(wl-paste --no-newline 2>/dev/null)" = "hello goo" ]
}
