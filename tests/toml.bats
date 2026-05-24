#!/usr/bin/env bats
# Tests for lib/toml.sh

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # shellcheck source=../lib/toml.sh
    . "$REPO_ROOT/lib/toml.sh"

    FIXTURE="$BATS_TEST_TMPDIR/sample.toml"
    cat > "$FIXTURE" <<'EOF'
name = "tmux"
description = "tmux session source and verbs"

[[types]]
name = "application/vnd.tmux-use.session"
display = "tmux session"
kind = "handle"

[[verbs]]
name = "switch"
accepts = ["application/vnd.tmux-use.session"]
default_for = "application/vnd.tmux-use.session"
cmd = "tmux-use switch {subject.id}"

[[verbs]]
name = "kill"
accepts = ["application/vnd.tmux-use.session"]
cmd = "tmux kill-session -t {subject.id}"
confirm = true
EOF
}

@test "toml_get returns scalar value as JSON" {
    run toml_get "$FIXTURE" '.name'
    [ "$status" -eq 0 ]
    [ "$output" = '"tmux"' ]
}

@test "toml_get with no query returns whole doc" {
    run toml_get "$FIXTURE"
    [ "$status" -eq 0 ]
    # Just verify it parses to something with a "name" field
    echo "$output" | jq -e '.name == "tmux"' >/dev/null
}

@test "toml_get on array element" {
    run toml_get "$FIXTURE" '.verbs[0].name'
    [ "$status" -eq 0 ]
    [ "$output" = '"switch"' ]
}

@test "toml_get on boolean" {
    run toml_get "$FIXTURE" '.verbs[1].confirm'
    [ "$status" -eq 0 ]
    [ "$output" = 'true' ]
}

@test "toml_get on missing file fails clearly" {
    run toml_get "$BATS_TEST_TMPDIR/does-not-exist.toml" '.name'
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not a file" ]]
}

@test "toml_keys lists top-level keys" {
    run toml_keys "$FIXTURE"
    [ "$status" -eq 0 ]
    # Output order is alphabetical per jq's `keys`.
    [[ "$output" =~ "description" ]]
    [[ "$output" =~ "name" ]]
    [[ "$output" =~ "types" ]]
    [[ "$output" =~ "verbs" ]]
}

@test "toml_keys lists keys of a nested object" {
    run toml_keys "$FIXTURE" '.verbs[0]'
    [ "$status" -eq 0 ]
    [[ "$output" =~ "accepts" ]]
    [[ "$output" =~ "cmd" ]]
    [[ "$output" =~ "default_for" ]]
    [[ "$output" =~ "name" ]]
}

@test "toml_keys on missing file fails clearly" {
    run toml_keys "$BATS_TEST_TMPDIR/does-not-exist.toml"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "not a file" ]]
}
