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

@test "providers: a hostile verb name never becomes a verb (injection impossible by construction)" {
    # The name comes from a project-local registry — treat it as attacker data.
    # NOTE the run template is UNQUOTED ({verb.name}, no |q): safety must come from
    # name validation dropping the hostile stub, NOT from the author quoting.
    marker="$BATS_TEST_TMPDIR/PWNED"
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<EOF
name = "cwd-fixture"
[[types]]
name = "application/vnd.goo.cwd"
kind = "handle"
[[providers]]
name = "evil"
for_type = "application/vnd.goo.cwd"
list_cmd = "printf '[{\"name\":\"a;touch $marker\",\"description\":\"x\"},{\"name\":\"safe\",\"description\":\"ok\"}]'"
run = "echo ran-{verb.name}"
EOF
    # The hostile name is filtered out of the listing; the valid sibling survives.
    run "$GOO" what :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"safe"* ]]
    [[ "$output" != *"touch"* ]]
    # Invoking the hostile name resolves to nothing and never runs the ;touch.
    run "$GOO" do :cwd "a;touch $marker"
    [ "$status" -ne 0 ]
    [[ "$output" == *"unknown verb"* ]]
    [ ! -e "$marker" ]
}

@test "providers: untrusted description cannot reach the cmd (only {verb.name} is templated)" {
    # description is untrusted free text from the same registry as the name, and
    # CANNOT be charset-restricted (it's prose). A dynamic verb must therefore
    # expose ONLY its validated name to the template — never {verb.description}.
    marker="$BATS_TEST_TMPDIR/DPWN"
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<EOF
name = "cwd-fixture"
[[types]]
name = "application/vnd.goo.cwd"
kind = "handle"
[[providers]]
name = "de"
for_type = "application/vnd.goo.cwd"
list_cmd = "printf '[{\"name\":\"go\",\"description\":\"\$(touch $marker)\"}]'"
run = "echo desc={verb.description} name={verb.name}"
EOF
    run "$GOO" do :cwd go
    [ "$status" -eq 0 ]
    [[ "$output" == *"name=go"* ]]   # the validated name IS available
    [[ "$output" != *"touch"* ]]     # the description is NOT in the template namespace
    [ ! -e "$marker" ]               # …so the $(touch) never ran
}

@test "providers: validate rejects a static verb with a shell-unsafe name" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/bad.toml" <<'EOF'
name = "bad-name"
[[verbs]]
name = "a;rm -rf"
accepts = ["text/*"]
cmd = "true"
EOF
    run "$GOO" validate
    [ "$status" -ne 0 ]
    [[ "$output" == *"not a valid name"* ]]
}
