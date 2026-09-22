use super::*;
use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

pub(super) fn timestamp() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 9, 0, 0).unwrap()
}

pub(super) fn request() -> GateRequest {
    GateRequest {
        schema_version: types::GATE_SCHEMA_VERSION,
        gate_id: "gate-123".to_string(),
        binding: GateBinding::mission_plan("session-456", "mission-789", 7, "b3:plan-snapshot"),
        question: GateQuestion {
            question: "Should I deploy this plan?".to_string(),
            problem: "The change is externally visible.".to_string(),
            impact: "Users may see new behavior.".to_string(),
            options: vec![GateOption {
                label: "Deploy".to_string(),
                description: "Apply the reviewed plan.".to_string(),
            }],
            recommendation: "Deploy after approval.".to_string(),
        },
        allowed_decisions: vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject,
        ],
        continuation: GateContinuation {
            on_approve: "Continue the approved plan.".to_string(),
            on_request_changes: "Revise only the requested part.".to_string(),
            on_reject: "Stop the gated action.".to_string(),
            on_expiry: "Stop because approval expired.".to_string(),
        },
        created_at: timestamp(),
        expires_at: timestamp() + Duration::minutes(30),
        delivery: DeliveryAcknowledgement::NotAttempted,
        state: GateState::Pending,
        transport_binding: None,
        resume_directive: None,
    }
}

pub(super) fn answer(request: &GateRequest, decision: GateDecision) -> AnswerRecord {
    AnswerRecord {
        schema_version: types::GATE_SCHEMA_VERSION,
        gate_id: request.gate_id.clone(),
        binding: request.binding.clone(),
        decision,
        comment: None,
        answered_at: timestamp() + Duration::minutes(1),
        received_via: "telegram".to_string(),
    }
}

pub(super) fn store(temp: &TempDir) -> GateStore {
    GateStore::at(temp.path().join("gates"))
}

#[test]
fn gate_serialization_is_versioned_and_delivery_unknown_is_observational() {
    let mut gate = request();
    gate.delivery = DeliveryAcknowledgement::DeliveryUnknown {
        observed_at: timestamp(),
        reason: "telegram unavailable".to_string(),
    };
    let bytes = serde_json::to_vec(&gate).unwrap();
    let decoded: GateRequest = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, gate);
    assert!(matches!(decoded.state, GateState::Pending));
}

#[test]
fn create_load_and_restart_preserve_pending_gate() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let first = store(&temp);
    first.create(gate.clone()).unwrap();
    let restarted = store(&temp);
    assert_eq!(
        restarted
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        gate
    );
}

#[test]
fn exact_answer_is_stored_then_applied() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let approved = answer(&gate, GateDecision::Approve);
    assert_eq!(
        ledger.ingest_answer(approved.clone(), timestamp()).unwrap(),
        AnswerIngestOutcome::Answered
    );
    assert_eq!(
        ledger
            .mark_applied(&gate.gate_id, &gate.binding.session_id, timestamp())
            .unwrap(),
        AnswerIngestOutcome::Applied
    );
    assert_eq!(
        ledger
            .mark_applied(
                &gate.gate_id,
                &gate.binding.session_id,
                timestamp() + Duration::seconds(1),
            )
            .unwrap(),
        AnswerIngestOutcome::Idempotent
    );
    let applied = ledger
        .load(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    assert!(matches!(applied.state, GateState::Applied { answer, .. } if answer == approved));
    assert_eq!(applied.resume_directive, None);
}

#[test]
fn stale_or_wrong_binding_cannot_mutate_gate() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let mut stale = answer(&gate, GateDecision::Approve);
    match &mut stale.binding.subject {
        GateSubject::MissionPlan { plan_revision, .. } => *plan_revision += 1,
        GateSubject::SessionDecision { .. } => unreachable!("fixture is a mission plan"),
    }
    assert!(ledger.ingest_answer(stale, timestamp()).is_err());
    let mut wrong = answer(&gate, GateDecision::Approve);
    wrong.binding.session_id = "other-session".to_string();
    assert!(ledger.ingest_answer(wrong, timestamp()).is_err());
    let mut wrong_hash = answer(&gate, GateDecision::Approve);
    match &mut wrong_hash.binding.subject {
        GateSubject::MissionPlan { plan_hash, .. } => *plan_hash = "b3:other-plan".to_string(),
        GateSubject::SessionDecision { .. } => unreachable!("fixture is a mission plan"),
    }
    assert!(ledger.ingest_answer(wrong_hash, timestamp()).is_err());
    assert!(matches!(
        ledger
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Pending
    ));
    assert_eq!(
        ledger
            .load_pending_directive(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        None
    );
}

#[test]
fn decision_not_allowed_by_the_gate_fails_closed_without_mutation() {
    let temp = TempDir::new().unwrap();
    let mut gate = request();
    gate.allowed_decisions = vec![GateDecision::Approve];
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    assert!(
        ledger
            .ingest_answer(answer(&gate, GateDecision::Reject), timestamp())
            .is_err()
    );
    assert!(matches!(
        ledger
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Pending
    ));
}

#[test]
fn same_answer_is_idempotent_but_conflicting_answer_is_rejected() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let approved = answer(&gate, GateDecision::Approve);
    assert_eq!(
        ledger.ingest_answer(approved.clone(), timestamp()).unwrap(),
        AnswerIngestOutcome::Answered
    );
    assert_eq!(
        ledger.ingest_answer(approved, timestamp()).unwrap(),
        AnswerIngestOutcome::Idempotent
    );
    assert!(
        ledger
            .ingest_answer(answer(&gate, GateDecision::Reject), timestamp())
            .is_err()
    );
}

#[test]
fn request_changes_requires_and_persists_a_comment() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let mut changes = answer(&gate, GateDecision::RequestChanges);
    assert!(ledger.ingest_answer(changes.clone(), timestamp()).is_err());
    changes.comment = Some("Use the existing endpoint instead.".to_string());
    assert_eq!(
        ledger.ingest_answer(changes.clone(), timestamp()).unwrap(),
        AnswerIngestOutcome::Answered
    );
    assert!(
        matches!(ledger.load(&gate.gate_id, &gate.binding.session_id).unwrap().state, GateState::Answered { answer } if answer == changes)
    );
}

#[test]
fn reject_is_an_answered_durable_refusal() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let rejected = answer(&gate, GateDecision::Reject);
    assert_eq!(
        ledger.ingest_answer(rejected.clone(), timestamp()).unwrap(),
        AnswerIngestOutcome::Answered
    );
    assert!(
        matches!(ledger.load(&gate.gate_id, &gate.binding.session_id).unwrap().state, GateState::Answered { answer } if answer == rejected)
    );
}

#[test]
fn expiry_and_cancellation_fail_closed() {
    let temp = TempDir::new().unwrap();
    let mut expired_gate = request();
    expired_gate.gate_id = "expired-gate".to_string();
    expired_gate.expires_at = timestamp() + Duration::seconds(1);
    let ledger = store(&temp);
    ledger.create(expired_gate.clone()).unwrap();
    assert_eq!(
        ledger
            .ingest_answer(
                answer(&expired_gate, GateDecision::Approve),
                timestamp() + Duration::minutes(1)
            )
            .unwrap(),
        AnswerIngestOutcome::Expired
    );
    assert!(matches!(
        ledger
            .load(&expired_gate.gate_id, &expired_gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Expired { .. }
    ));

    let cancelled_gate = request();
    ledger.create(cancelled_gate.clone()).unwrap();
    ledger
        .cancel(
            &cancelled_gate.gate_id,
            &cancelled_gate.binding.session_id,
            "user cancelled",
            timestamp(),
        )
        .unwrap();
    assert!(
        ledger
            .ingest_answer(answer(&cancelled_gate, GateDecision::Approve), timestamp())
            .is_err()
    );
}

#[test]
fn atomic_json_bytes_remain_parseable_after_create_and_answer() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    ledger
        .ingest_answer(answer(&gate, GateDecision::Approve), timestamp())
        .unwrap();
    let bytes = std::fs::read(
        ledger
            .path_for(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
    )
    .unwrap();
    let decoded: GateRequest = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(decoded.state, GateState::Answered { .. }));
    assert!(matches!(
        decoded.resume_directive,
        Some(GateResumeDirective {
            state: GateResumeDirectiveState::Unconsumed,
            ..
        })
    ));
    assert!(
        std::fs::read_dir(temp.path().join("gates"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp."))
    );
}

#[test]
fn unknown_persisted_schema_fails_closed_without_mutation() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let path = ledger
        .path_for(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    raw["schema_version"] = serde_json::json!(999);
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let bytes_before = std::fs::read(&path).unwrap();
    assert!(
        ledger
            .ingest_answer(answer(&gate, GateDecision::Approve), timestamp())
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes_before);
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod path_tests;
