//! Strict, transport-neutral control-request DTOs and local idempotency receipts.
//!
//! This module is deliberately not a Hermes adapter or an execution surface. It
//! validates the small request contract and persists claim/result authority so a
//! later trusted adapter can safely poll and execute requests.

mod receipt;
mod request;
mod store;

pub use receipt::{ControlExecutionResult, ControlReceipt, ControlReceiptState, ReceiptKey};
pub use request::{
    AuthenticatedPrincipal, CONTROL_RECEIPT_SCHEMA, CONTROL_SCHEMA, ControlAction, ControlRequest,
    MAX_INITIAL_PROMPT_CHARS, MAX_MESSAGE_CHARS, PinnedCreateSessionResolution,
};
pub use store::{ClaimOutcome, ControlReceiptStore, ReceiveOutcome};

#[cfg(test)]
mod tests;
