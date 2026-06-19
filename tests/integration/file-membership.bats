#!/usr/bin/env bats
# A file on disk is both a filesystem HANDLE and a typed DATUM: it keeps its content
# type (text/csv, application/pdf) AND carries an `inode/file` membership, so file-handle
# verbs (open / reveal / copy-path, which accept inode/*) match alongside its content
# verbs. The membership is provenance-gated — clipboard / `+text` of the same content
# type does NOT get handle verbs. See address::resolve_file / verbs::subject_types.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME"
    cd "$BATS_TEST_TMPDIR" || return 1
    printf 'name,age\nalice,30\n' > data.csv
}

@test "file membership: a CSV lists handle verbs AND content verbs, keeping its content type" {
    run "$GOO" what ./data.csv </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"text/csv"* ]]      # type stays the refined content type
    [[ "$output" == *"open"* ]]          # …and the handle verbs match via inode/file
    [[ "$output" == *"reveal"* ]]
    [[ "$output" == *"copy-path"* ]]
    [[ "$output" == *"upper"* ]]         # content verb still applies
}

@test "file membership: a bareword path (no ./) lists the same — listing agrees with run" {
    run "$GOO" what data.csv </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"copy-path"* ]]
}

@test "file membership: open dispatches on a file via inode/file (a direct match, not 415)" {
    run "$GOO" --explain open ./data.csv --explain-with route </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"inode/file"* ]]    # routes from the membership, directly
    [[ "$output" != *"415"* ]]
}

@test "file membership: --explain via the :file/ sigil is consistent with native paths (direct, not 415)" {
    # The resolved-subject path: :file/<abs> and ./data.csv must preview the same
    # direct inode/file route — membership comes from the resolved subject, uniformly.
    abs="$BATS_TEST_TMPDIR/data.csv"
    run "$GOO" --explain open ":file/$abs" --explain-with route </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"text/csv"* ]]      # typed via the resolved subject, not mis-typed text
    [[ "$output" == *"inode/file"* ]]    # direct handle-verb route
    [[ "$output" != *"415"* ]]
}

@test "file membership: content coercion is undisturbed (json-keys still routes csv2json)" {
    run "$GOO" --explain json-keys ./data.csv --explain-with route </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"csv2json"* ]]
}

@test "file membership: provenance guard — +text gets NO handle verbs (no path, no facet)" {
    run "$GOO" what +hello </dev/null
    [[ "$output" != *"copy-path"* ]]
    [[ "$output" != *"reveal"* ]]
}

# A SOURCE mints per-instance facets from its data (the contact mechanism: a subject
# is `pingable` iff its data says so), bounded by the source's declared `facets`
# allowlist. Item `a` emits the declared capability facet AND an undeclared bus type
# (`inode/file`); the allowlist keeps the former and DROPS the latter — so untrusted
# list_cmd data can't forge file verbs. `b` emits nothing.
@test "membership: a source mints declared capability facets per-instance; undeclared ones are dropped" {
    cat > "$BATS_TEST_TMPDIR/things.toml" <<'EOF'
name = "things-fixture"
[[types]]
name = "application/vnd.test.thing"
kind = "handle"
[[types]]
name = "application/vnd.test.pingable"
[[sources]]
name = "things"
prefix = "thing"
emits = "application/vnd.test.thing"
facets = ["application/vnd.test.pingable"]
list_cmd = '''printf '[{"id":"a","title":"A","_facets":["application/vnd.test.pingable","inode/file"]},{"id":"b","title":"B"}]' '''
[[verbs]]
name = "ping"
accepts = ["application/vnd.test.pingable"]
cmd = "echo pong"
EOF
    # `a` claims the DECLARED facet → `ping` applies…
    run "$GOO" -c "$BATS_TEST_TMPDIR/things.toml" what :thing/a </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"ping"* ]]
    # …but the UNDECLARED `inode/file` it also emitted is dropped → no file verbs.
    [[ "$output" != *"copy-path"* ]]
    [[ "$output" != *"reveal"* ]]
    # `b` emits no facet → no ping (the per-instance guard).
    run "$GOO" -c "$BATS_TEST_TMPDIR/things.toml" what :thing/b </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" != *"ping"* ]]
}
