#!/usr/bin/env bats
# Dynamic verb providers ([[providers]]) — a provider's list_cmd enumerates verbs
# for any subject whose type matches for_type, synthesizing one verb per emitted
# {name, description}. Exercised here on the built-in :cwd subject. No external
# tool: the fixture provider's list_cmd is a plain printf.  Rust-only feature.
#
# See doc/design/dynamic-verb-providers.md.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR"
    cd "$BATS_TEST_TMPDIR" || return 1

    # The :cwd type + a provider that contributes two dynamic verbs for it. The
    # run template echoes {verb.name} so we can prove the synthesized verb both
    # carries its name and executes.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<'EOF'
name = "cwd-fixture"

[[types]]
name = "application/vnd.goo.cwd"
display = "working directory"
kind = "handle"

[[providers]]
name = "fix"
for_type = "application/vnd.goo.cwd"
list_cmd = "printf '[{\"name\":\"foo\",\"description\":\"the foo verb\"},{\"name\":\"bar\",\"description\":\"the bar verb\"}]'"
run = "echo ran-{verb.name|q}"
EOF
}

bad_provider() {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<EOF
name = "cwd-fixture"
[[types]]
name = "application/vnd.goo.cwd"
kind = "handle"
[[providers]]
name = "broken"
for_type = "application/vnd.goo.cwd"
list_cmd = "$1"
run = "echo nope"
EOF
}

@test "providers: validate counts the provider" {
    run "$GOO" validate
    [ "$status" -eq 0 ]
    [[ "$output" == *"1 providers"* ]]
}

@test "providers: goo what :cwd lists the dynamic verbs with descriptions" {
    run "$GOO" what :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"foo"* ]]
    [[ "$output" == *"the foo verb"* ]]
    [[ "$output" == *"bar"* ]]
    [[ "$output" == *"the bar verb"* ]]
}

@test "providers: verb-first runs the dynamic verb (goo foo :cwd)" {
    run "$GOO" foo :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"ran-foo"* ]]   # {verb.name} substituted in the run template
}

@test "providers: noun-first runs the dynamic verb (goo do :cwd bar)" {
    run "$GOO" do :cwd bar
    [ "$status" -eq 0 ]
    [[ "$output" == *"ran-bar"* ]]
}

@test "providers: a real typo still dies fast as unknown verb" {
    run "$GOO" nope :cwd
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown verb"* ]]
}

@test "providers: static verbs win a name collision (provider can't shadow)" {
    # Add a static verb named foo accepting the cwd type; it must win.
    cat >> "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<'EOF'

[[verbs]]
name = "foo"
accepts = ["application/vnd.goo.cwd"]
description = "static foo"
cmd = "echo static-foo"
EOF
    run "$GOO" foo :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"static-foo"* ]]
    [[ "$output" != *"ran-foo"* ]]
}

@test "providers: a list_cmd that errors yields no verbs (graceful, no crash)" {
    bad_provider "exit 1"
    run "$GOO" what :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"no applicable verbs"* ]]
}

@test "providers: non-JSON list_cmd output yields no verbs (graceful)" {
    bad_provider "echo not-json-at-all"
    run "$GOO" what :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"no applicable verbs"* ]]
}

@test "providers: a hostile verb name does not inject (run template uses |q)" {
    # The name comes from a project-local registry — treat it as attacker data.
    # The run template's {verb.name|q} must neutralize shell metacharacters.
    marker="$BATS_TEST_TMPDIR/PWNED"
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<EOF
name = "cwd-fixture"
[[types]]
name = "application/vnd.goo.cwd"
kind = "handle"
[[providers]]
name = "evil"
for_type = "application/vnd.goo.cwd"
list_cmd = "printf '[{\"name\":\"a;touch $marker\",\"description\":\"x\"}]'"
run = "echo ran-{verb.name|q}"
EOF
    run "$GOO" do :cwd "a;touch $marker"
    [ "$status" -eq 0 ]
    [ ! -e "$marker" ]   # the ;touch must NOT have executed
}
