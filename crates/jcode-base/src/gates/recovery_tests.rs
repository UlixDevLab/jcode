use super::*;
use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

fn timestamp() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 9, 0, 0).unwrap()
}

fn request(gate_id: &str, session_id: &str) -> GateRequest {
    GateRequest {
        schema_version: types::GATE_SCHEMA_VERSION,
        gate_id: gate_id.to_string(),
        binding: GateBinding::mission_plan(session_id, "mission-recovery", 7, "b3:plan-snapshot"),
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

fn answer(gate: &GateRequest) -> AnswerRecord {
    AnswerRecord {
        schema_version: types::GATE_SCHEMA_VERSION,
        gate_id: gate.gate_id.clone(),
        binding: gate.binding.clone(),
        decision: GateDecision::Approve,
        comment: None,
        answered_at: timestamp() + Duration::minutes(1),
        received_via: "telegram".to_string(),
    }
}

fn store(temp: &TempDir) -> GateStore {
    GateStore::at(temp.path().join("gates"))
}

fn answered(ledger: &GateStore, gate: &GateRequest) -> GateResumeDirective {
    match ledger.record_answer(answer(gate), timestamp()).unwrap() {
        RecordAnswerOutcome::Recorded { directive } => directive,
        outcome => panic!("expected recorded answer, got {outcome:?}"),
    }
}

fn overwrite(ledger: &GateStore, gate: &GateRequest, value: &GateRequest) {
    let path = ledger
        .path_for(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn recovery_returns_only_sorted_unconsumed_answered_directives_after_restart() {
    let temp = TempDir::new().unwrap();
    let ledger = store(&temp);
    let first = request("gate-b", "session-a");
    let second = request("gate-a", "session-a");
    let consumed = request("gate-c", "session-a");
    let applied = request("gate-a", "session-b");
    let pending = request("gate-pending", "session-b");
    let cancelled = request("gate-cancelled", "session-b");
    let expired = request("gate-expired", "session-b");
    let unanswerable = request("gate-unanswerable", "session-b");
    for gate in [
        &first,
        &second,
        &consumed,
        &applied,
        &pending,
        &cancelled,
        &expired,
        &unanswerable,
    ] {
        ledger.create((*gate).clone()).unwrap();
    }
    let first_directive = answered(&ledger, &first);
    let second_directive = answered(&ledger, &second);
    let consumed_directive = answered(&ledger, &consumed);
    assert!(
        ledger
            .consume_pending_directive(
                &consumed.gate_id,
                &consumed.binding.session_id,
                &consumed_directive,
                timestamp() + Duration::minutes(2),
            )
            .unwrap()
    );
    answered(&ledger, &applied);
    assert_eq!(
        ledger
            .mark_applied(&applied.gate_id, &applied.binding.session_id, timestamp())
            .unwrap(),
        AnswerIngestOutcome::Applied
    );
    ledger
        .cancel(
            &cancelled.gate_id,
            &cancelled.binding.session_id,
            "operator cancelled",
            timestamp(),
        )
        .unwrap();
    ledger
        .expire(
            &expired.gate_id,
            &expired.binding.session_id,
            timestamp() + Duration::minutes(31),
        )
        .unwrap();
    let mut unanswerable_record = unanswerable.clone();
    unanswerable_record.state = GateState::Unanswerable {
        answer: answer(&unanswerable),
    };
    overwrite(&ledger, &unanswerable, &unanswerable_record);

    let first_path = ledger
        .path_for(&first.gate_id, &first.binding.session_id)
        .unwrap();
    let before_scan = std::fs::read(&first_path).unwrap();
    let recovered = store(&temp).load_unconsumed_directives().unwrap();
    assert_eq!(std::fs::read(first_path).unwrap(), before_scan);
    assert_eq!(
        recovered,
        vec![
            UnconsumedGateDirective {
                gate_id: second.gate_id,
                session_id: second.binding.session_id,
                directive: second_directive,
            },
            UnconsumedGateDirective {
                gate_id: first.gate_id,
                session_id: first.binding.session_id,
                directive: first_directive,
            },
        ]
    );
}

#[test]
fn recovery_fails_closed_for_malformed_and_unknown_schema_records() {
    let temp = TempDir::new().unwrap();
    let ledger = store(&temp);
    let gate = request("gate-corrupt", "session-corrupt");
    ledger.create(gate.clone()).unwrap();
    let path = ledger
        .path_for(&gate.gate_id, &gate.binding.session_id)
        .unwrap();
    std::fs::write(&path, b"not-json").unwrap();
    assert!(ledger.load_unconsumed_directives().is_err());

    let mut unknown = gate;
    unknown.schema_version += 1;
    std::fs::write(path, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert!(ledger.load_unconsumed_directives().is_err());
}

#[cfg(unix)]
#[test]
fn recovery_rejects_root_session_and_record_symlinks_without_touching_outside() {
    for kind in ["root", "session", "record"] {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("gates");
        let outside = temp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let sentinel = outside.join("sentinel");
        std::fs::write(&sentinel, b"untouched").unwrap();
        match kind {
            "root" => symlink(&outside, &root).unwrap(),
            "session" => {
                std::fs::create_dir(&root).unwrap();
                symlink(&outside, root.join("session-link")).unwrap();
            }
            "record" => {
                let session = root.join("session-record");
                std::fs::create_dir_all(&session).unwrap();
                symlink(&sentinel, session.join("gate-record.json")).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(GateStore::at(&root).load_unconsumed_directives().is_err());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
    }
}

#[test]
fn concurrent_recovery_readers_observe_only_parseable_records() {
    let temp = TempDir::new().unwrap();
    let ledger = store(&temp);
    let gate = request("gate-concurrent", "session-concurrent");
    ledger.create(gate.clone()).unwrap();
    let directive = answered(&ledger, &gate);
    let writer = ledger.clone();
    let reader = ledger.clone();
    let gate_for_writer = gate.clone();
    let writer_thread = std::thread::spawn(move || {
        for minute in 2..32 {
            writer
                .record_delivery(
                    &gate_for_writer.gate_id,
                    &gate_for_writer.binding.session_id,
                    DeliveryAcknowledgement::DeliveryUnknown {
                        observed_at: timestamp() + Duration::minutes(minute),
                        reason: "transport retry".to_string(),
                    },
                )
                .unwrap();
        }
    });
    let reader_thread = std::thread::spawn(move || {
        for _ in 0..30 {
            assert_eq!(
                reader.load_unconsumed_directives().unwrap(),
                vec![UnconsumedGateDirective {
                    gate_id: "gate-concurrent".to_string(),
                    session_id: "session-concurrent".to_string(),
                    directive: directive.clone(),
                }]
            );
        }
    });
    writer_thread.join().unwrap();
    reader_thread.join().unwrap();
}
