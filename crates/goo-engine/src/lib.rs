//! cosmic-goo engine (Rust port).
//!
//! The bash in `lib/*.sh` is the behavioral reference and the bats suite is the
//! conformance contract (see `doc/design/rust-port-scoping.md`). Modules are
//! ported one slice at a time; this crate currently covers MIME matching.

pub mod mime;
