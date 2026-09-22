//! Exactly-once lifecycle consumption for accepted durable gate directives.
//!
//! The gate ledger owns authorization. This module owns the single transition
//! from its unconsumed directive to a server-owned internal continuation. A
//! delivery is only returned after the directive has been claimed and the gate
//! is durably marked `Applied`; retrying a crash after the claim repairs that
//! marker but deliberately never re-delivers a user-visible continuation.

use super::live_turn::{LiveTurnSwarmContext, run_live_system_turn_if_idle};
use super::{SessionAgents, SessionInterruptQueues, queue_soft_interrupt_for_session};
use crate::gates::{
    GateDecision, GateRequest, GateResumeDirective, GateResumeDirectiveState, GateState, GateStore,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use jcode_agent_runtime::SoftInterruptSource;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

const CHANGE_COMMENT_LIMIT: usize = 1_200;

/// One canonical internal continuation already authorized and durably applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GateResumeDelivery {
    pub(super) decision: GateDecision,
    pub(super) message: String,
}

/// Result of attempting to consume one exact gate/session directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GateResumeConsumeOutcome {
    /// The caller owns exactly one internal continuation delivery.
    Deliver(GateResumeDelivery),
    /// A prior process claimed the directive and died before recording Applied.
    /// The idempotent repair completed, but must not produce another delivery.
    RepairedAfterCrash,
    /// There is no accepted, unconsumed directive for this exact gate/session.
    NoDirective,
}

/// Result of delivering a claimed directive to a currently live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GateResumeLiveOutcome {
    DeliveredLive,
    QueuedForBusyLiveSession,
    /// No client currently owns the session. The directive is intentionally
    /// left unconsumed for a durable startup-recovery owner to claim.
    DeferredForRestore,
    RepairedAfterCrash,
    NoDirective,
}

/// Result of delivering a claimed directive to its already-restored session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GateResumeRestoredOutcome {
    DeliveredRestoredSession,
    RepairedAfterCrash,
    NoDirective,
}

/// Consume and start one canonical system continuation for the exact agent
/// already restored by the server. This never constructs a client message.
///
/// [`consume_exact_directive`] remains the sole CAS-and-Applied boundary. The
/// streaming turn starts only after that durable transition succeeds.
pub(super) async fn consume_and_deliver_to_restored_session(
    store: &GateStore,
    gate_id: &str,
    session_id: &str,
    now: DateTime<Utc>,
    agent: Arc<Mutex<crate::agent::Agent>>,
    event_tx: mpsc::UnboundedSender<crate::protocol::ServerEvent>,
) -> Result<GateResumeRestoredOutcome> {
    let restored_session_id = agent.lock().await.session_id().to_string();
    if restored_session_id != session_id {
        anyhow::bail!(
            "restored agent session {} does not match gate session {}",
            restored_session_id,
            session_id
        );
    }

    match consume_exact_directive(store, gate_id, session_id, now)? {
        GateResumeConsumeOutcome::Deliver(delivery) => {
            super::client_lifecycle::process_message_streaming_mpsc(
                agent,
                "",
                vec![],
                Some(delivery.message),
                event_tx,
            )
            .await?;
            Ok(GateResumeRestoredOutcome::DeliveredRestoredSession)
        }
        GateResumeConsumeOutcome::RepairedAfterCrash => {
            Ok(GateResumeRestoredOutcome::RepairedAfterCrash)
        }
        GateResumeConsumeOutcome::NoDirective => Ok(GateResumeRestoredOutcome::NoDirective),
    }
}

/// Consume and deliver one directive only when its target currently has a live
/// client attachment. Idle clients receive a canonical system turn without a
/// user message. Busy clients receive the same canonical message through the
/// server-owned soft-interrupt queue exactly once.
///
/// A non-live target is deliberately not consumed here. The server's durable
/// startup recovery path must claim it when it can restore the exact session;
/// staging a client-only input file would not safely auto-start that session.
pub(super) async fn consume_and_deliver_to_live_session(
    store: &GateStore,
    gate_id: &str,
    session_id: &str,
    now: DateTime<Utc>,
    sessions: &SessionAgents,
    soft_interrupt_queues: &SessionInterruptQueues,
    swarm: LiveTurnSwarmContext,
) -> Result<GateResumeLiveOutcome> {
    if !has_live_attachment(session_id, sessions, &swarm).await {
        return Ok(GateResumeLiveOutcome::DeferredForRestore);
    }

    match consume_exact_directive(store, gate_id, session_id, now)? {
        GateResumeConsumeOutcome::Deliver(delivery) => {
            if run_live_system_turn_if_idle(session_id, &delivery.message, sessions, swarm.clone())
                .await
            {
                return Ok(GateResumeLiveOutcome::DeliveredLive);
            }

            if queue_soft_interrupt_for_session(
                session_id,
                delivery.message,
                false,
                SoftInterruptSource::System,
                soft_interrupt_queues,
                sessions,
            )
            .await
            {
                Ok(GateResumeLiveOutcome::QueuedForBusyLiveSession)
            } else {
                anyhow::bail!("gate directive was applied but could not be queued for live session")
            }
        }
        GateResumeConsumeOutcome::RepairedAfterCrash => {
            Ok(GateResumeLiveOutcome::RepairedAfterCrash)
        }
        GateResumeConsumeOutcome::NoDirective => Ok(GateResumeLiveOutcome::NoDirective),
    }
}

async fn has_live_attachment(
    session_id: &str,
    sessions: &SessionAgents,
    swarm: &LiveTurnSwarmContext,
) -> bool {
    if !sessions.read().await.contains_key(session_id) {
        return false;
    }
    swarm
        .members
        .read()
        .await
        .get(session_id)
        .is_some_and(|member| !member.event_txs.is_empty() || !member.event_tx.is_closed())
}

/// Claim one exact directive, durably apply it, then return its one delivery.
///
/// This deliberately does not scan the ledger. A caller must supply both
/// durable identity components obtained from the trusted answer event or a
/// known session recovery record. That keeps foreign or stale gate records from
/// waking arbitrary sessions.
pub(super) fn consume_exact_directive(
    store: &GateStore,
    gate_id: &str,
    session_id: &str,
    now: DateTime<Utc>,
) -> Result<GateResumeConsumeOutcome> {
    let gate = store.load(gate_id, session_id)?;
    let GateState::Answered { .. } = gate.state else {
        return Ok(GateResumeConsumeOutcome::NoDirective);
    };
    let Some(directive) = gate.resume_directive else {
        return Ok(GateResumeConsumeOutcome::NoDirective);
    };

    match directive.state {
        GateResumeDirectiveState::Unconsumed => {
            if !store.consume_pending_directive(gate_id, session_id, &directive, now)? {
                return Ok(GateResumeConsumeOutcome::NoDirective);
            }
            store.mark_applied(gate_id, session_id, now)?;
            Ok(GateResumeConsumeOutcome::Deliver(GateResumeDelivery {
                decision: directive.decision,
                message: canonical_continuation(&directive),
            }))
        }
        GateResumeDirectiveState::Consumed { .. } => {
            // The compare-and-swap was the exactly-once boundary. A process can
            // die after it succeeds but before `mark_applied`; repair only the
            // ledger state so retries never surface a duplicate continuation.
            store.mark_applied(gate_id, session_id, now)?;
            Ok(GateResumeConsumeOutcome::RepairedAfterCrash)
        }
    }
}

fn canonical_continuation(directive: &GateResumeDirective) -> String {
    match directive.decision {
        GateDecision::Approve => format!(
            "Gate approved. Resume only the approved continuation below.\n\n{}",
            directive.continuation
        ),
        GateDecision::RequestChanges => format!(
            "Gate requested changes. Apply only this bounded change contract, then continue.\n\nContinuation: {}\n\nRequested changes: {}",
            directive.continuation,
            bounded_change_comment(directive.reason.as_deref().unwrap_or_default())
        ),
        GateDecision::Reject => format!(
            "Gate rejected. Do not perform the gated work. Deliver the refusal or termination outcome only.\n\n{}",
            directive.continuation
        ),
    }
}

fn bounded_change_comment(comment: &str) -> String {
    let mut bounded: String = comment.chars().take(CHANGE_COMMENT_LIMIT).collect();
    if comment.chars().nth(CHANGE_COMMENT_LIMIT).is_some() {
        bounded.push_str("…");
    }
    bounded
}

#[cfg(test)]
#[path = "gate_resume_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gate_resume_restored_tests.rs"]
mod restored_tests;
