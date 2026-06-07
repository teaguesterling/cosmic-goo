//! cosmic-goo engine (Rust port).
//!
//! The bash in `lib/*.sh` is the behavioral reference and the bats suite is the
//! conformance contract (see `doc/design/rust-port-scoping.md`). Modules are
//! ported one slice at a time; this crate currently covers MIME matching.

pub mod address;
pub mod adverbs;
pub mod compose;
pub mod dispatch;
pub mod exec;
pub mod history;
pub mod inference;
pub mod mime;
pub mod negotiation;
pub mod options;
pub mod registry;
pub mod selection;
pub mod shell;
pub mod template;
pub mod verbs;
