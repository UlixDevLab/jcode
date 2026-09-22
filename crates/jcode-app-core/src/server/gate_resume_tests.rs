use super::*;
use crate::gates::types::GATE_SCHEMA_VERSION;
use crate::gates::{
    AnswerRecord, DeliveryAcknowledgement, GateBinding, GateContinuation, GateOption, GateQuestion,
};
use chrono::{Duration, TimeZone};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tempfile::TempDir;
use tokio::sync::{Mutex, RwLock, broadcast};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 10, 0, 0).unwrap()
}

fn gate(id: &str, session_id: &str) -> GateRequest {
    GateRequest {
        schema_version: GATE_SCHEMA_VERSION,
        gate_id: id.to_string(),
        binding: GateBinding::mission_plan(session_id, "mission-gate-resume", 2, "b3:gate-resume"),
        transport_binding: None,
        question: GateQuestion {
            question: "Proceed?".to_string(),
            problem: "A user decision is required.".to_string(),
            impact: "The plan will be resumed or stopped.".to_string(),
            options: vec![GateOption {
                label: "Choose".to_string(),
                description: "Choose a continuation.".to_string(),
            }],
            recommendation: "Approve only when ready.".to_string(),
        },
        allowed_decisions: vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject,
        ],
        continuation: GateContinuation {
            on_approve: "Continue approved work.".to_string(),
            on_request_changes: "Revise the approved scope.".to_string(),
            on_reject: "Stop the gated work.".to_string(),
            on_expiry: "Stop after expiry.".to_string(),
        },
        created_at: now(),
        expires_at: now() + Duration::minutes(30),
        delivery: DeliveryAcknowledgement::NotAttempted,
        state: GateState::Pending,
        resume_directive: None,
    }
}

fn answer(gate: &GateRequest, decision: GateDecision, comment: Option<String>) -> AnswerRecord {
    AnswerRecord {
        schema_version: GATE_SCHEMA_VERSION,
        gate_id: gate.gate_id.clone(),
        binding: gate.binding.clone(),
        decision,
        comment,
        answered_at: now(),
        received_via: "test".to_string(),
    }
}

fn store(temp: &TempDir) -> GateStore {
    GateStore::at(temp.path().join("gates"))
}

fn empty_live_turn_context() -> LiveTurnSwarmContext {
    let members = Arc::new(RwLock::new(HashMap::new()));
    let swarms_by_id = Arc::new(RwLock::new(HashMap::<String, HashSet<String>>::new()));
    let event_history = Arc::new(RwLock::new(VecDeque::new()));
    let event_counter = Arc::new(AtomicU64::new(1));
    let (event_tx, _) = broadcast::channel(1);
    LiveTurnSwarmContext::new(
        &members,
        &swarms_by_id,
        &event_history,
        &event_counter,
        &event_tx,
    )
}

#[test]
fn approved_directive_is_delivered_once_then_cleared_as_applied() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);
    let request = gate("gate-approve", "session-approve");
    ledger.create(request.clone())?;
    ledger.record_answer(answer(&request, GateDecision::Approve, None), now())?;

    let outcome = consume_exact_directive(
        &ledger,
        &request.gate_id,
        &request.binding.session_id,
        now(),
    )?;
    let GateResumeConsumeOutcome::Deliver(delivery) = outcome else {
        panic!("expected exactly one delivery");
    };
    assert_eq!(delivery.decision, GateDecision::Approve);
    assert!(delivery.message.contains("Continue approved work."));
    assert!(matches!(
        ledger
            .load(&request.gate_id, &request.binding.session_id)?
            .state,
        GateState::Applied { .. }
    ));
    assert_eq!(
        consume_exact_directive(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now()
        )?,
        GateResumeConsumeOutcome::NoDirective
    );
    Ok(())
}

#[test]
fn request_changes_has_a_bounded_explicit_change_contract() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);
    let request = gate("gate-change", "session-change");
    ledger.create(request.clone())?;
    let comment = format!(
        "Keep this requirement. {}",
        "x".repeat(CHANGE_COMMENT_LIMIT + 20)
    );
    ledger.record_answer(
        answer(&request, GateDecision::RequestChanges, Some(comment)),
        now(),
    )?;

    let GateResumeConsumeOutcome::Deliver(delivery) = consume_exact_directive(
        &ledger,
        &request.gate_id,
        &request.binding.session_id,
        now(),
    )?
    else {
        panic!("expected change delivery");
    };
    assert_eq!(delivery.decision, GateDecision::RequestChanges);
    assert!(delivery.message.contains("bounded change contract"));
    assert!(delivery.message.contains("Keep this requirement."));
    assert!(delivery.message.ends_with('…'));
    assert!(delivery.message.len() < CHANGE_COMMENT_LIMIT + 300);
    Ok(())
}

#[test]
fn rejection_delivers_only_a_refusal_termination_instruction() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);
    let request = gate("gate-reject", "session-reject");
    ledger.create(request.clone())?;
    ledger.record_answer(answer(&request, GateDecision::Reject, None), now())?;

    let GateResumeConsumeOutcome::Deliver(delivery) = consume_exact_directive(
        &ledger,
        &request.gate_id,
        &request.binding.session_id,
        now(),
    )?
    else {
        panic!("expected rejection delivery");
    };
    assert_eq!(delivery.decision, GateDecision::Reject);
    assert!(delivery.message.contains("Do not perform the gated work"));
    assert!(delivery.message.contains("Stop the gated work."));
    Ok(())
}

#[test]
fn crash_after_claim_repairs_applied_without_second_delivery() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);
    let request = gate("gate-crash", "session-crash");
    ledger.create(request.clone())?;
    let directive =
        match ledger.record_answer(answer(&request, GateDecision::Approve, None), now())? {
            crate::gates::RecordAnswerOutcome::Recorded { directive } => directive,
            other => panic!("unexpected answer result: {other:?}"),
        };
    assert!(ledger.consume_pending_directive(
        &request.gate_id,
        &request.binding.session_id,
        &directive,
        now()
    )?);

    assert_eq!(
        consume_exact_directive(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now()
        )?,
        GateResumeConsumeOutcome::RepairedAfterCrash
    );
    assert!(matches!(
        ledger
            .load(&request.gate_id, &request.binding.session_id)?
            .state,
        GateState::Applied { .. }
    ));
    Ok(())
}

#[test]
fn unanswered_expired_and_cancelled_gates_never_produce_a_delivery() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);

    let unanswered = gate("gate-unanswered", "session-unanswered");
    ledger.create(unanswered.clone())?;
    assert_eq!(
        consume_exact_directive(
            &ledger,
            &unanswered.gate_id,
            &unanswered.binding.session_id,
            now()
        )?,
        GateResumeConsumeOutcome::NoDirective
    );

    let expired = gate("gate-expired", "session-expired");
    ledger.create(expired.clone())?;
    ledger.expire(
        &expired.gate_id,
        &expired.binding.session_id,
        now() + Duration::hours(1),
    )?;
    assert_eq!(
        consume_exact_directive(
            &ledger,
            &expired.gate_id,
            &expired.binding.session_id,
            now()
        )?,
        GateResumeConsumeOutcome::NoDirective
    );

    let cancelled = gate("gate-cancelled", "session-cancelled");
    ledger.create(cancelled.clone())?;
    ledger.cancel(
        &cancelled.gate_id,
        &cancelled.binding.session_id,
        "user withdrew the request",
        now(),
    )?;
    assert_eq!(
        consume_exact_directive(
            &ledger,
            &cancelled.gate_id,
            &cancelled.binding.session_id,
            now()
        )?,
        GateResumeConsumeOutcome::NoDirective
    );
    Ok(())
}

#[tokio::test]
async fn non_live_target_stays_unconsumed_for_durable_restart_recovery() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = store(&temp);
    let request = gate("gate-non-live", "session-non-live");
    ledger.create(request.clone())?;
    let directive =
        match ledger.record_answer(answer(&request, GateDecision::Approve, None), now())? {
            crate::gates::RecordAnswerOutcome::Recorded { directive } => directive,
            other => panic!("unexpected answer result: {other:?}"),
        };
    let sessions = Arc::new(RwLock::new(HashMap::<
        String,
        Arc<Mutex<crate::agent::Agent>>,
    >::new()));
    let queues = Arc::new(RwLock::new(HashMap::new()));

    assert_eq!(
        consume_and_deliver_to_live_session(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now(),
            &sessions,
            &queues,
            empty_live_turn_context(),
        )
        .await?,
        GateResumeLiveOutcome::DeferredForRestore
    );
    assert_eq!(
        ledger.load_pending_directive(&request.gate_id, &request.binding.session_id)?,
        Some(directive)
    );
    Ok(())
}
