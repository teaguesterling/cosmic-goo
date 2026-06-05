#!/usr/bin/env bats
# JSON Schema for plugin TOML (#11 / data-entry-ux §6.12). The schema at
# schema/cosmic-goo-plugin.schema.json gives plugin authors editor validation +
# completion. This test keeps it honest: it must be a valid Draft-07 schema,
# every shipped plugin (and the dispatch example) must conform, and it must
# actually reject a malformed plugin (so the schema isn't vacuously permissive).
#
# Needs `tomlq` (TOML->JSON) and python `jsonschema`; skips cleanly when either
# is absent — they're authoring/CI conveniences, not core engine conformance
# deps, so this never blocks `make test` on a bare box.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    SCHEMA="$REPO_ROOT/schema/cosmic-goo-plugin.schema.json"
    command -v tomlq >/dev/null 2>&1 || skip "tomlq not available"
    python3 -c 'import jsonschema' 2>/dev/null || skip "python jsonschema not available"
}

# validate <toml-file> — prints schema errors (path -> message), nonzero on fail.
validate() {
    tomlq . "$1" | python3 -c '
import json, sys
from jsonschema import Draft7Validator
schema = json.load(open(sys.argv[1]))
errs = sorted(Draft7Validator(schema).iter_errors(json.load(sys.stdin)), key=lambda e: list(e.path))
for e in errs:
    print(list(e.path), "->", e.message)
sys.exit(1 if errs else 0)
' "$SCHEMA"
}

@test "schema: is itself a valid Draft-07 JSON Schema" {
    run python3 -c '
import json, sys
from jsonschema import Draft7Validator
Draft7Validator.check_schema(json.load(open(sys.argv[1])))
' "$SCHEMA"
    [ "$status" -eq 0 ]
}

@test "schema: every shipped plugin conforms" {
    local f bad=0
    for f in "$REPO_ROOT"/plugins/*.toml; do
        run validate "$f"
        if [ "$status" -ne 0 ]; then
            echo "FAIL: $f"; echo "$output"; bad=1
        fi
    done
    [ "$bad" -eq 0 ]
}

@test "schema: the dispatch example conforms" {
    run validate "$REPO_ROOT/plugins/dispatch.toml.example"
    [ "$status" -eq 0 ]
}

@test "schema: rejects a malformed plugin (not vacuously permissive)" {
    local bad="$BATS_TEST_TMPDIR/bad.toml"
    # singular [[verb]] typo (caught by top-level additionalProperties) AND an
    # invalid channel cost (caught by the enum).
    cat > "$bad" <<'EOF'
name = "bad"

[[verb]]
name = "oops"

[[channels]]
name = "c"
emits = "application/json"
cost = "supersonic"
EOF
    run validate "$bad"
    [ "$status" -ne 0 ]
    [[ "$output" =~ "verb" || "$output" =~ "supersonic" || "$output" =~ "Additional" ]]
}
