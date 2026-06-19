#!/usr/bin/env bats
# The contacts example (doc/examples/contacts.toml) — the per-instance capability-facet
# worked example. A contact is emailable/callable/messageable iff its vCard has the
# field; the source mints those facets per-instance, so the verb list adapts per contact.
# Backend: a directory of .vcf files via $GOO_CONTACTS_DIR.

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/../.." && pwd)"
    GOO="${GOO_BIN:-$REPO_ROOT/bin/goo}"
    EX="$REPO_ROOT/doc/examples/contacts.toml"
    export COSMIC_GOO_BUILTIN_PLUGINS_DIR="$REPO_ROOT/plugins"
    export XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/xdg"
    export XDG_RUNTIME_DIR="$BATS_TEST_TMPDIR/runtime"
    HOME="$BATS_TEST_TMPDIR/home"
    export GOO_CONTACTS_DIR="$BATS_TEST_TMPDIR/contacts"
    mkdir -p "$XDG_CONFIG_HOME" "$XDG_RUNTIME_DIR" "$HOME" "$GOO_CONTACTS_DIR"
    cd "$BATS_TEST_TMPDIR" || return 1
    # Alice: email + phone (all three facets). Bob: email only.
    printf 'BEGIN:VCARD\r\nUID:alice-1\r\nFN:Alice Smith\r\nEMAIL:alice@example.com\r\nTEL:+15551234567\r\nEND:VCARD\r\n' > "$GOO_CONTACTS_DIR/alice.vcf"
    printf 'BEGIN:VCARD\r\nUID:bob-2\r\nFN:Bob Jones\r\nEMAIL:bob@example.com\r\nEND:VCARD\r\n' > "$GOO_CONTACTS_DIR/bob.vcf"
}

@test "contacts: the example validates (facets are declared types → allowlist OK)" {
    run "$GOO" -c "$EX" validate </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" =~ "OK" ]]
}

@test "contacts: card resolves and prints the vCard fields" {
    run "$GOO" -c "$EX" card :contact:alice </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"Alice Smith"* ]]
    [[ "$output" == *"alice@example.com"* ]]
    [[ "$output" == *"+15551234567"* ]]
}

@test "contacts: bare address runs the default verb (card)" {
    run "$GOO" -c "$EX" :contact:bob </dev/null
    [ "$status" -eq 0 ]
    [[ "$output" == *"Bob Jones"* ]]
}

@test "contacts: facet adaptivity — Alice (email+phone) offers all; Bob (email only) doesn't offer call/message" {
    run "$GOO" -c "$EX" what :contact:alice </dev/null
    [[ "$output" == *"email"* ]]
    [[ "$output" == *"call"* ]]
    [[ "$output" == *"message"* ]]
    run "$GOO" -c "$EX" what :contact:bob </dev/null
    [[ "$output" == *"email"* ]]      # Bob has an email
    [[ "$output" != *"call"* ]]       # …but no phone → no callable facet
    [[ "$output" != *"message"* ]]
}

@test "contacts: a facet verb on a contact lacking the field is cleanly 415'd, not crashed" {
    run "$GOO" -c "$EX" call :contact:bob </dev/null
    [ "$status" -ne 0 ]
    [[ "$output" == *"no route"* || "$output" == *"415"* ]]
}

@test "contacts: no vCard dir → no contacts, never an error in the listing" {
    rm -rf "$GOO_CONTACTS_DIR"
    run "$GOO" -c "$EX" what :contact:alice </dev/null
    # The address can't resolve (no items), but it must be a clean failure, not a panic.
    [[ "$output" != *"panic"* ]]
}
