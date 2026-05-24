#!/usr/bin/env bats
# Tests for lib/url-encode.sh

setup() {
    REPO_ROOT="$(cd "$BATS_TEST_DIRNAME/.." && pwd)"
    # shellcheck source=../lib/url-encode.sh
    . "$REPO_ROOT/lib/url-encode.sh"
}

@test "url_encode: simple alphanumeric passes through" {
    run url_encode "hello"
    [ "$status" -eq 0 ]
    [ "$output" = "hello" ]
}

@test "url_encode: space -> %20" {
    run url_encode "Hello world"
    [ "$status" -eq 0 ]
    [ "$output" = "Hello%20world" ]
}

@test "url_encode: punctuation escaped" {
    run url_encode "Hello, world!"
    [ "$status" -eq 0 ]
    [ "$output" = "Hello%2C%20world%21" ]
}

@test "url_encode: ampersand and equals escaped" {
    run url_encode "a=b&c=d"
    [ "$status" -eq 0 ]
    [ "$output" = "a%3Db%26c%3Dd" ]
}

@test "url_encode: unicode (latin-1)" {
    run url_encode "café"
    [ "$status" -eq 0 ]
    [ "$output" = "caf%C3%A9" ]
}

@test "url_encode: empty string -> empty output" {
    run url_encode ""
    [ "$status" -eq 0 ]
    [ "$output" = "" ]
}

@test "url_encode: slash is preserved as path separator (RFC 3986 reserved)" {
    # jq's @uri filter encodes / as %2F. That's RFC-3986-strict and what we want
    # for safety when embedding arbitrary text in query params.
    run url_encode "foo/bar"
    [ "$status" -eq 0 ]
    [ "$output" = "foo%2Fbar" ]
}
