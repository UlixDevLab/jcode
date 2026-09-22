//! Trusted-local browser broker — provenance and domain types.
//!
//! This crate defines the foundational types for the planned
//! `local_browser` tool: literal loopback origin parsing, opaque element
//! references, navigation epochs, redacted action receipts, and the
//! non-serializable broker handle stub, and a fixed-size zeroizing capability
//! primitive for the future private IPC channel.
//!
//! # Scope of this milestone
//!
//! The crate is deliberately still a pure library with no Chromium process, no Unix
//! socket, no IPC, and no tool registration. The runtime behavior lands in
//! M2 (private IPC) and M3 (network/navigation boundary).
//!
//! # Authorization boundary
//!
//! This crate does **not** authorize generic Playwright, headful
//! browsers, persistent profiles, or any path that can reach a non-
//! loopback origin. The types here exist only to constrain a single
//! `local_browser` instance to a single literal loopback origin for its
//! lifetime. Any other browser work — including generic Playwright
//! mutation, evaluation, upload, persistent storage, headful sessions,
//! and unknown operations — remains native-consent protected and is
//! **out of scope** for this crate.
//!
//! Model-controlled input (intent strings, fabricated receipts,
//! approval fields, tool descriptions, or URL labels) is never accepted
//! as authorization. Receipts produced here are redacted audit output
//! only.

#![deny(rust_2018_idioms)]
#![warn(missing_docs)]

pub mod capability;
pub mod handle;
pub mod origin;
pub mod receipt;
pub mod refs;
