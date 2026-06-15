//! Untrusted-text containment for the terminal.
//!
//! Strings that reach goo from outside its own code — a source `list_cmd`'s item
//! titles/ids, a `[[providers]]` stub's description, the clipboard / PRIMARY
//! selection / stdin — are untrusted. Printed raw they can carry ANSI escapes, an
//! OSC title-set, or a CR/LF that recolors the terminal, rewrites its title, or
//! spoofs other lines of a listing. The shell-injection vector is closed *by
//! construction* (a verb name is a validated identifier; a dynamic verb exposes
//! only its name to the cmd); this module gives the terminal-display vector the
//! same shape, so we stop auditing every `println!` for "did I sanitize?".
//!
//! [`Tainted`] holds an untrusted string and has **no `Display`** — you cannot
//! `format!("{}", t)` it. The only ways out are `.sanitized()` (control chars
//! stripped, safe for a terminal) and `.expose()` (the raw bytes, explicit, for
//! a *functional* use like addressing — never for the terminal). [`DisplayView`]
//! is a lens over a subject/verb `Value` whose string accessors return `Tainted`,
//! so a display function that takes a `DisplayView` physically has no raw field to
//! print.
//!
//! Honest ceiling: the bin still holds the underlying `Value` for dispatch, so a
//! future call site *can* reach past the lens with `value.get("title")?.as_str()`
//! and print that — that still compiles. What the types buy is that the
//! least-resistance path (the accessor) is the safe one, any bypass is a visible
//! `Value::as_str()` sitting in display code, and every function written against
//! `DisplayView`/`Tainted` is fully closed. Absolute would need the engine to
//! abandon `Value`, which isn't worth it — the engine never prints (it consumes
//! untrusted strings through `|q`).

use serde_json::Value;

/// Strip every Unicode control character (C0, DEL, C1) and keep printable text.
/// The single definition of "safe to write to a terminal"; both [`Tainted`] and
/// the snippet path go through it.
pub fn sanitize(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// An untrusted string. **No `Display`** — see the module docs.
#[derive(Clone, PartialEq, Eq)]
pub struct Tainted(String);

impl Tainted {
    pub fn new(s: impl Into<String>) -> Self {
        Tainted(s.into())
    }

    /// Terminal-safe form: control characters stripped.
    pub fn sanitized(&self) -> String {
        sanitize(&self.0)
    }

    /// The raw, unsanitized value — for *functional* use only (addressing,
    /// matching, a path handed to the filesystem). Never write this to a
    /// terminal; that's what [`Tainted::sanitized`] is for. Named to make the
    /// raw escape hatch loud at every call site.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// Redacting Debug: `{:?}` is a print path too, so it must not leak raw control
// bytes. Show the sanitized content (and mark it Tainted so logs are honest).
impl std::fmt::Debug for Tainted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tainted({:?})", self.sanitized())
    }
}

// Deliberately NO `impl Display for Tainted`. That omission is the guarantee.

/// A read-only display lens over a subject/verb `Value`: its string accessors
/// return [`Tainted`], so display code built on a `DisplayView` cannot reach a
/// printable-raw field. The engine keeps the `Value`; this is constructed at the
/// display boundary.
pub struct DisplayView<'a>(&'a Value);

impl<'a> DisplayView<'a> {
    pub fn new(value: &'a Value) -> Self {
        DisplayView(value)
    }

    fn field(&self, key: &str) -> Tainted {
        Tainted::new(self.0.get(key).and_then(Value::as_str).unwrap_or_default())
    }

    pub fn id(&self) -> Tainted {
        self.field("id")
    }
    pub fn title(&self) -> Tainted {
        self.field("title")
    }
    pub fn text(&self) -> Tainted {
        self.field("text")
    }
    pub fn description(&self) -> Tainted {
        self.field("description")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Control bytes from char codes — a literal ESC in source gets mangled by the
    // harness, and the point is to avoid raw control bytes in the file anyway.
    fn esc() -> char {
        char::from(27)
    }

    #[test]
    fn sanitized_strips_control_keeps_printable_unicode() {
        let t = Tainted::new(format!("{}[31mRED{}]0;PWN{}tail", esc(), esc(), char::from(7)));
        assert_eq!(t.sanitized(), "[31mRED]0;PWNtail");
        assert_eq!(Tainted::new("héllo · 日本語 ✓").sanitized(), "héllo · 日本語 ✓");
    }

    #[test]
    fn sanitized_drops_cr_lf_so_a_value_cannot_spoof_lines() {
        let t = Tainted::new(format!("ok{}{}    fake-line", char::from(13), char::from(10)));
        let out = t.sanitized();
        assert!(!out.contains('\r') && !out.contains('\n'));
        assert_eq!(out, "ok    fake-line");
    }

    #[test]
    fn expose_is_raw_for_functional_use() {
        let raw = format!("a{}b", esc());
        let t = Tainted::new(raw.clone());
        assert_eq!(t.expose(), raw); // raw preserved for addressing/matching
        assert_ne!(t.sanitized(), raw); // but the displayable form is stripped
    }

    #[test]
    fn debug_is_redacted_not_raw() {
        let t = Tainted::new(format!("x{}y", esc()));
        let dbg = format!("{t:?}");
        assert!(!dbg.contains(esc())); // {:?} must not leak the control byte
        assert!(dbg.contains("xy"));
    }

    #[test]
    fn display_view_accessors_yield_sanitized() {
        let subject = json!({
            "type": "application/vnd.test",
            "id": format!("id{}x", esc()),
            "title": format!("t{}itle", esc()),
            "text": format!("bo{}dy", esc()),
        });
        let v = DisplayView::new(&subject);
        assert_eq!(v.id().sanitized(), "idx");
        assert_eq!(v.title().sanitized(), "title");
        assert_eq!(v.text().sanitized(), "body");
        assert!(v.description().is_empty()); // missing field → empty Tainted
    }
}
