//! MIME type matching — the Rust port of `mime_matches` in `lib/types.sh`.
//!
//! The bash uses a `case "$mime" in $pattern)` glob. We mirror that: `*` matches
//! any run of characters (including `/`), so `text/*` matches
//! `text/plain;charset=utf-8`, `*/json` matches `application/json`, and
//! `application/vnd.x.*` matches a vendor subtype. An empty pattern or empty
//! type never matches. Other bash-glob metacharacters (`?`, `[...]`) are not
//! used by any shipped pattern, so they are treated literally here; if a plugin
//! ever needs them, extend `glob_match` and add a parity test.
//!
//! `mime_detect_path` / `mime_detect_content` (which shell out to `file` and
//! touch the filesystem) land with the address slice, since detection feeds
//! canonicalization.

/// Returns true iff `mime` matches the glob `pattern`, mirroring
/// `lib/types.sh::mime_matches`. Empty `pattern` or empty `mime` never match.
pub fn mime_matches(pattern: &str, mime: &str) -> bool {
    if pattern.is_empty() || mime.is_empty() {
        return false;
    }
    glob_match(pattern, mime)
}

/// Minimal glob where only `*` is special (matches any sequence, including an
/// empty one). Implemented by splitting the pattern on `*` and matching the
/// literal segments left-to-right: the first must be a prefix, the last a
/// suffix, and any middle segment must occur in order.
fn glob_match(pattern: &str, text: &str) -> bool {
    let segments: Vec<&str> = pattern.split('*').collect();
    if segments.len() == 1 {
        // No `*` — exact match.
        return pattern == text;
    }
    let last = segments.len() - 1;
    let mut pos = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        if i == 0 {
            // Leading literal must be a prefix.
            if !text[pos..].starts_with(seg) {
                return false;
            }
            pos += seg.len();
        } else if i == last {
            // Trailing literal must be a suffix of the remainder.
            return text[pos..].ends_with(seg);
        } else {
            // Middle literal must appear at or after the current position.
            match text[pos..].find(seg) {
                Some(idx) => pos += idx + seg.len(),
                None => return false,
            }
        }
    }
    // Pattern ended on a `*` (empty trailing segment): the rest matches.
    true
}

#[cfg(test)]
mod tests {
    use super::mime_matches;

    // These mirror, one-to-one, the `mime_matches` cases in tests/types.bats.
    #[test]
    fn exact_match() {
        assert!(mime_matches("text/plain", "text/plain"));
    }
    #[test]
    fn exact_non_match() {
        assert!(!mime_matches("text/plain", "text/markdown"));
    }
    #[test]
    fn suffix_wildcard_matches_markdown() {
        assert!(mime_matches("text/*", "text/markdown"));
    }
    #[test]
    fn suffix_wildcard_matches_plain() {
        assert!(mime_matches("text/*", "text/plain"));
    }
    #[test]
    fn suffix_wildcard_does_not_cross_supertype() {
        assert!(!mime_matches("text/*", "application/json"));
    }
    #[test]
    fn prefix_wildcard_matches() {
        assert!(mime_matches("*/json", "application/json"));
    }
    #[test]
    fn prefix_wildcard_non_match() {
        assert!(!mime_matches("*/json", "application/xml"));
    }
    #[test]
    fn vendor_wildcard_matches_subtype() {
        assert!(mime_matches(
            "application/vnd.tmux-use.*",
            "application/vnd.tmux-use.session"
        ));
    }
    #[test]
    fn vendor_wildcard_different_vendor() {
        assert!(!mime_matches(
            "application/vnd.tmux-use.*",
            "application/vnd.cos-cli.app"
        ));
    }
    #[test]
    fn text_star_matches_charset_parameter() {
        assert!(mime_matches("text/*", "text/plain;charset=utf-8"));
    }
    #[test]
    fn empty_pattern_no_match() {
        assert!(!mime_matches("", "text/plain"));
    }
    #[test]
    fn empty_mime_no_match() {
        assert!(!mime_matches("text/*", ""));
    }

    // A couple beyond the bats set, pinning glob edge cases the port relies on.
    #[test]
    fn bare_star_matches_anything_nonempty() {
        assert!(mime_matches("*", "anything/at-all"));
        assert!(!mime_matches("*", "")); // empty mime still never matches
    }
    #[test]
    fn exact_with_no_wildcard_is_strict() {
        assert!(!mime_matches("text/pla", "text/plain"));
    }
}
