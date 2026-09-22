//! Durable, transport-neutral user gate records.
//!
//! This module owns only durable gate authority. Transport delivery and session
//! continuation are deliberately layered above it.

mod binding;
pub mod recovery;
pub mod store;
pub mod transport;
pub mod types;

pub use recovery::UnconsumedGateDirective;
pub use store::{AnswerIngestOutcome, ExpireOutcome, GateStore, RecordAnswerOutcome};
pub use transport::{GATE_TRANSPORT_BINDING_VERSION, GateTransportBinding};
pub use types::{
    AnswerRecord, DeliveryAcknowledgement, GateBinding, GateContinuation, GateDecision, GateOption,
    GateQuestion, GateRequest, GateResumeDirective, GateResumeDirectiveState, GateState,
    GateSubject,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod binding_tests;

#[cfg(test)]
mod directive_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod transport_tests;
