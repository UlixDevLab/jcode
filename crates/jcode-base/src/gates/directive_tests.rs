use super::*;
use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

fn timestamp() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 9, 0, 0).unwrap()
}

fn request() -> GateRequest {
    GateRequest {
        schema_version: types::GATE_SCHEMA_VERSION,
        gate_id: "gate-directive".to_string(),
        binding: GateBinding::mission_plan(
            "session-directive",
            "mission-directive",
            7,
            "b3:plan-snapshot",
        ),
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

fn answer(request: &GateRequest, decision: GateDecision) -> AnswerRecord {
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

fn store(temp: &TempDir) -> GateStore {
    GateStore::at(temp.path().join("gates"))
}

fn recorded_directive(outcome: RecordAnswerOutcome) -> GateResumeDirective {
    match outcome {
        RecordAnswerOutcome::Recorded { directive }
        | RecordAnswerOutcome::Idempotent { directive } => directive,
        RecordAnswerOutcome::Expired => panic!("expected a recorded answer"),
    }
}

#[test]
fn reject_is_answered_then_applied_with_one_durable_resume_directive() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let rejected = answer(&gate, GateDecision::Reject);

    let directive =
        recorded_directive(ledger.record_answer(rejected.clone(), timestamp()).unwrap());
    assert_eq!(directive.decision, GateDecision::Reject);
    assert_eq!(directive.reason, None);
    assert_eq!(directive.continuation, gate.continuation.on_reject);
    assert!(matches!(
        directive.state,
        GateResumeDirectiveState::Unconsumed
    ));
    let answered = ledger
        .load(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    assert!(matches!(answered.state, GateState::Answered { answer } if answer == rejected));
    assert_eq!(answered.resume_directive, Some(directive.clone()));

    assert_eq!(
        recorded_directive(ledger.record_answer(rejected, timestamp()).unwrap()),
        directive
    );
    let restarted = store(&temp);
    assert_eq!(
        restarted
            .load_pending_directive(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        Some(directive.clone())
    );
    assert!(
        restarted
            .consume_pending_directive(
                &gate.gate_id,
                &gate.binding.session_id,
                &directive,
                timestamp() + Duration::minutes(2),
            )
            .unwrap()
    );
    assert_eq!(
        restarted
            .load_pending_directive(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        None
    );
    assert!(
        !restarted
            .consume_pending_directive(
                &gate.gate_id,
                &gate.binding.session_id,
                &directive,
                timestamp() + Duration::minutes(3),
            )
            .unwrap()
    );

    assert_eq!(
        restarted
            .mark_applied(&gate.gate_id, &gate.binding.session_id, timestamp())
            .unwrap(),
        AnswerIngestOutcome::Applied
    );
    let applied = restarted
        .load(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    assert!(
        matches!(applied.state, GateState::Applied { answer, .. } if answer.decision == GateDecision::Reject)
    );
    assert_eq!(applied.resume_directive, None);
}

#[test]
fn invalid_expired_and_conflicting_answers_never_create_or_change_a_directive() {
    let temp = TempDir::new().unwrap();
    let gate = request();
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let mut stale = answer(&gate, GateDecision::Approve);
    match &mut stale.binding.subject {
        GateSubject::MissionPlan { plan_revision, .. } => *plan_revision += 1,
        GateSubject::SessionDecision { .. } => unreachable!("fixture is a mission plan"),
    }
    assert!(ledger.record_answer(stale, timestamp()).is_err());
    assert_eq!(
        ledger
            .load_pending_directive(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        None
    );

    let approved = answer(&gate, GateDecision::Approve);
    let directive = recorded_directive(ledger.record_answer(approved, timestamp()).unwrap());
    assert!(
        ledger
            .record_answer(answer(&gate, GateDecision::Reject), timestamp())
            .is_err()
    );
    assert_eq!(
        ledger
            .load_pending_directive(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        Some(directive)
    );

    let mut expired = request();
    expired.gate_id = "expired-directive".to_string();
    expired.expires_at = timestamp() + Duration::seconds(1);
    ledger.create(expired.clone()).unwrap();
    assert_eq!(
        ledger
            .record_answer(
                answer(&expired, GateDecision::Reject),
                timestamp() + Duration::minutes(1),
            )
            .unwrap(),
        RecordAnswerOutcome::Expired
    );
    assert_eq!(
        ledger
            .load_pending_directive(&expired.gate_id, &expired.binding.session_id)
            .unwrap(),
        None
    );
}
