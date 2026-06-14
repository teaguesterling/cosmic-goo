#!/usr/bin/env bats
# Untrusted text shown in a TERMINAL surface must not carry control characters.
# Source- and provider-derived strings (verb descriptions, subject titles/ids, a
# rendered cmd that interpolates them) are not author-controlled — a raw ANSI
# escape / OSC title-set / CR-LF could recolor the terminal, rewrite its title,
# or spoof other listing lines. `display_safe` strips control chars at every such
# print site. These are the negative tests. (Control bytes are generated at
# runtime via awk %c so no raw control byte lives in the fixture/source.)

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"

    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$BATS_TEST_TMPDIR/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$COSMIC_GOO_BUILTIN_PLUGINS_DIR" "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    cd "$BATS_TEST_TMPDIR" || return 1
    # An ESC byte (0x1b), built without any backslash escape.
    ESC="$(awk 'BEGIN{printf "%c", 27}')"
}

# Does $output contain a raw ESC (0x1b)? grep -P \x1b is the check.
has_esc() { printf '%s' "$1" | grep -qP '\x1b'; }

@test "display: a provider verb description with ANSI/OSC escapes is sanitized in the listing" {
    # list_cmd emits a description containing real ESC bytes; jq JSON-encodes them
    # () so it's valid JSON, and the engine decodes them back to control chars.
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cwd.toml" <<'EOF'
name = "cwd-fixture"
[[types]]
name = "application/vnd.goo.cwd"
kind = "handle"
[[providers]]
name = "e"
for_type = "application/vnd.goo.cwd"
list_cmd = '''awk 'BEGIN{printf "%c[31mRED%c]0;PWNtail",27,27}' | jq -Rsc '[{name:"go",description:.}]' '''
run = "echo hi"
EOF
    run "$GOO" what :cwd
    [ "$status" -eq 0 ]
    [[ "$output" == *"go"* ]]            # the verb still lists
    [[ "$output" == *"RED"* ]]           # printable part of the description survives
    ! has_esc "$output"                  # …but no raw ESC reaches the terminal
}

@test "display: --explain --explain-with shell sanitizes a hostile filename in the shown command" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/catf.toml" <<'EOF'
name = "catf"
[[verbs]]
name = "catf"
accepts = ["text/*"]
cmd = "cat {subject.metadata.path|q}"
EOF
    # A real file whose NAME carries a raw ESC byte (Linux allows it); its path is
    # baked into the command shown by --explain.
    f="$BATS_TEST_TMPDIR/$(awk 'BEGIN{printf "f%cesc.txt", 27}')"
    printf 'hi\n' > "$f"
    run "$GOO" --explain catf "$f" --explain-with shell
    [ "$status" -eq 0 ]
    [[ "$output" == *"fesc.txt"* ]]   # printable part of the path is shown…
    ! has_esc "$output"               # …with no raw ESC baked into the command
}

@test "display: an untrusted subject (title/text + rendered cmd) is sanitized in the confirm prompt" {
    cat > "$COSMIC_GOO_BUILTIN_PLUGINS_DIR/cf.toml" <<'EOF'
name = "cf-esc"
[[verbs]]
name = "zap"
accepts = ["text/*"]
confirm = true
cmd = "echo {subject.text}"
EOF
    subj="${ESC}]0;PWNhello"             # subject text with a raw OSC title-set escape
    # EOF on stdin cancels the prompt; bats `run` captures the (stderr) prompt text.
    run "$GOO" zap "+$subj" </dev/null
    [[ "$output" == *"subject:"* ]]      # the prompt rendered the subject label…
    [[ "$output" == *"PWNhello"* ]]      # …printable part intact
    ! has_esc "$output"                  # subject: AND runs: lines carry no raw ESC
}
