//! Template substitution — the Rust port of `verbs.sh::_substitute`.
//!
//! Replaces `{path.to.var}` (and `{path|filter}`) placeholders from a JSON
//! context. Left-to-right scan that tolerates literal `{` (a `{` not followed by
//! a valid placeholder is emitted verbatim — so JSON in a command survives).
//!
//! Filters:
//!   `|q`   shell-quote — single-quote wrapping (`'`→`'\''`), shell-safe for any
//!          content (newlines, `$()`, backticks). Behaviorally equal to the
//!          shell's `printf %q`: the bats contract is round-trip safety, not a
//!          byte-identical string, so single-quoting suffices and needs no crate.
//!   `|uri` percent-encode, matching `jq @uri` (encodeURIComponent set).
//!   `|raw` / none / unknown → verbatim.

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;

/// Characters jq `@uri` (and JS encodeURIComponent) leave unescaped, on top of
/// alphanumerics: `- _ . ! ~ * ' ( )`.
const URI_KEEP: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Substitute `{path}` / `{path|filter}` placeholders in `template` from `vars`.
pub fn substitute(template: &str, vars: &Value) -> String {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }
        // Try to parse a placeholder starting at the '{'.
        match parse_placeholder(&template[i..]) {
            Some((path, filter, consumed)) => {
                out.push_str(&render(path, filter, vars));
                i += consumed;
            }
            None => {
                out.push('{'); // literal brace
                i += 1;
            }
        }
    }
    out
}

/// Parse `{ident(.ident)*(|filter)?}` at the start of `s`. Returns
/// (path, filter, bytes_consumed) or None if it isn't a valid placeholder.
fn parse_placeholder(s: &str) -> Option<(&str, Option<&str>, usize)> {
    let b = s.as_bytes();
    debug_assert_eq!(b[0], b'{');
    let mut j = 1;
    // path: [A-Za-z_][A-Za-z0-9_.-]*
    let path_start = j;
    if j >= b.len() || !(b[j].is_ascii_alphabetic() || b[j] == b'_') {
        return None;
    }
    j += 1;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'.' | b'-')) {
        j += 1;
    }
    let path = &s[path_start..j];
    // optional |filter  (filter = [a-z]+)
    let mut filter = None;
    if j < b.len() && b[j] == b'|' {
        let fs = j + 1;
        let mut k = fs;
        while k < b.len() && b[k].is_ascii_lowercase() {
            k += 1;
        }
        if k == fs {
            return None; // empty filter name
        }
        filter = Some(&s[fs..k]);
        j = k;
    }
    if j >= b.len() || b[j] != b'}' {
        return None;
    }
    Some((path, filter, j + 1))
}

fn render(path: &str, filter: Option<&str>, vars: &Value) -> String {
    let value = lookup(path, vars);
    match filter {
        Some("q") | Some("sh") | Some("shell") => shell_quote(&value),
        Some("uri") | Some("url") => utf8_percent_encode(&value, URI_KEEP).to_string(),
        _ => value, // raw / none / unknown
    }
}

/// Dotted path lookup over JSON objects (object keys only; a missing key or a
/// null intermediate → empty string, matching jq `.a.b // empty`).
fn lookup(path: &str, vars: &Value) -> String {
    let mut cur = vars;
    for key in path.split('.') {
        match cur.get(key) {
            Some(v) => cur = v,
            None => return String::new(),
        }
    }
    match cur {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// POSIX single-quote shell-quoting: wrap in `'…'`, escaping any `'` as `'\''`.
fn shell_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::substitute;
    use serde_json::json;

    // Mirror tests/verbs.bats _substitute cases.
    #[test]
    fn replaces_nested_path() {
        let v = json!({"a":{"b":"deep value"}});
        assert_eq!(substitute("got: {a.b}", &v), "got: deep value");
    }
    #[test]
    fn unknown_path_is_empty() {
        let v = json!({"a":1});
        assert_eq!(substitute("got: '{missing.key}'", &v), "got: ''");
    }
    #[test]
    fn multiple_substitutions() {
        let v = json!({"x":"foo","y":"bar"});
        assert_eq!(substitute("{x}-{y}-{x}", &v), "foo-bar-foo");
    }
    #[test]
    fn uri_filter_matches_jq() {
        let v = json!({"x":"a b&c=d"});
        assert_eq!(substitute("{x|uri}", &v), "a%20b%26c%3Dd");
    }
    #[test]
    fn raw_equals_no_filter() {
        let v = json!({"x":"a b/c"});
        assert_eq!(substitute("{x|raw}", &v), "a b/c");
        assert_eq!(substitute("{x|raw}", &v), substitute("{x}", &v));
    }
    #[test]
    fn unknown_filter_falls_back_to_raw() {
        let v = json!({"x":"value"});
        assert_eq!(substitute("{x|bogus}", &v), "value");
    }
    #[test]
    fn literal_brace_not_a_placeholder_survives() {
        // JSON in a command: the {"id":..} braces are literal; {x} substitutes.
        let v = json!({"x":"X"});
        assert_eq!(substitute("printf '[{\"id\":\"{x}\"}]'", &v),
                   "printf '[{\"id\":\"X\"}]'");
    }

    // |q is behavioral: the rendered command must run safely and round-trip.
    #[test]
    fn q_filter_is_shell_safe_and_round_trips() {
        let v = json!({"x":"a b'c"});
        let rendered = substitute("printf %s {x|q}", &v);
        let out = std::process::Command::new("bash")
            .arg("-c").arg(&rendered).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "a b'c");
    }
    #[test]
    fn q_filter_neutralizes_injection() {
        let dir = std::env::temp_dir().join(format!("goo-tmpl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("pwned");
        let payload = format!("a; $(touch {0}) `touch {0}`", marker.display());
        let v = json!({ "x": payload });
        let rendered = substitute("printf %s {x|q}", &v);
        let out = std::process::Command::new("bash")
            .arg("-c").arg(&rendered).output().unwrap();
        assert!(!marker.exists(), "injection executed!");
        assert_eq!(String::from_utf8_lossy(&out.stdout), payload);
        std::fs::remove_dir_all(&dir).ok();
    }
}
