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
        binding: GateBinding::mission_plan(session_id, "mission-transport", 7, "b3:plan-snapshot"),
        question: GateQuestion {
            question: "Should I deliver this gate?".to_string(),
            problem: "The change is externally visible.".to_string(),
            impact: "Users may see new behavior.".to_string(),
            options: vec![GateOption {
                label: "Deliver".to_string(),
                description: "Send the reviewed gate.".to_string(),
            }],
            recommendation: "Deliver after review.".to_string(),
        },
        allowed_decisions: vec![GateDecision::Approve],
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
        resume_directive: None,
        transport_binding: None,
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
        received_via: "hermes".to_string(),
    }
}

fn store(temp: &TempDir) -> GateStore {
    GateStore::at(temp.path().join("gates"))
}

fn transport(gate: &GateRequest) -> GateTransportBinding {
    GateTransportBinding::for_gate(&gate.gate_id, gate.binding.clone()).unwrap()
}

#[test]
fn legacy_json_without_transport_binding_decodes_to_none() {
    let gate = request("gate-legacy", "session-legacy");
    let mut value = serde_json::to_value(gate).unwrap();
    value.as_object_mut().unwrap().remove("transport_binding");
    let decoded: GateRequest = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.transport_binding, None);
}

#[test]
fn deterministic_hermes_envelope_id_is_transparent_or_bounded_digest() {
    let gate = request("gate-123", "session-456");
    assert_eq!(transport(&gate).envelope_id, "jcode-gate-gate-123");

    let long_id = "g".repeat(256);
    let long_gate = request(&long_id, "session-long");
    assert_eq!(
        transport(&long_gate).envelope_id,
        "jcode-gate-sha256-cfb6b1696df45c9f250c548d55631d0215868e3d382511062435b8292f300378"
    );
}

#[test]
fn create_and_restart_preserve_pending_transport_binding() {
    let temp = TempDir::new().unwrap();
    let gate = request("gate-restart", "session-restart");
    let binding = transport(&gate);
    let first = store(&temp);
    assert_eq!(
        first
            .create_pending_transported(gate.clone(), binding.clone())
            .unwrap(),
        binding
    );
    assert_eq!(
        store(&temp)
            .load_transport_binding(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        Some(binding)
    );
}

#[test]
fn session_decision_transport_binding_persists_across_restart() {
    let temp = TempDir::new().unwrap();
    let mut gate = request("gate-session-decision", "session-session-decision");
    gate.binding =
        GateBinding::session_decision("session-session-decision", 4, "b3:session-decision-packet");
    let binding = transport(&gate);
    store(&temp)
        .create_pending_transported(gate.clone(), binding.clone())
        .unwrap();
    assert_eq!(
        store(&temp)
            .load_transport_binding(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        Some(binding)
    );
}

#[test]
fn transport_binding_survives_answer_and_applied_lifecycle_states() {
    let temp = TempDir::new().unwrap();
    let gate = request("gate-applied-binding", "session-applied-binding");
    let binding = transport(&gate);
    let ledger = store(&temp);
    ledger
        .create_pending_transported(gate.clone(), binding.clone())
        .unwrap();
    ledger.record_answer(answer(&gate), timestamp()).unwrap();
    ledger
        .mark_applied(&gate.gate_id, &gate.binding.session_id, timestamp())
        .unwrap();

    let restarted = store(&temp);
    assert_eq!(
        restarted
            .load_transport_binding(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        Some(binding)
    );
    assert!(matches!(
        restarted
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Applied { .. }
    ));
}

#[test]
fn exact_transport_replay_is_idempotent_and_conflicts_fail_closed() {
    let temp = TempDir::new().unwrap();
    let gate = request("gate-replay", "session-replay");
    let binding = transport(&gate);
    let ledger = store(&temp);
    ledger
        .create_pending_transported(gate.clone(), binding.clone())
        .unwrap();
    assert_eq!(
        ledger.attach_transport_binding(binding.clone()).unwrap(),
        binding
    );

    let mut wrong_envelope = binding.clone();
    wrong_envelope.envelope_id = "jcode-gate-other".to_string();
    assert!(ledger.attach_transport_binding(wrong_envelope).is_err());

    let mut wrong_session = binding;
    wrong_session.binding.session_id = "session-other".to_string();
    assert!(ledger.attach_transport_binding(wrong_session).is_err());
}

#[test]
fn list_pending_transported_gates_is_sorted_and_excludes_non_awaiting_records() {
    let temp = TempDir::new().unwrap();
    let ledger = store(&temp);
    let selected_first = request("gate-b", "session-a");
    let selected_second = request("gate-a", "session-b");
    let plain = request("gate-plain", "session-a");
    let answered = request("gate-answered", "session-a");
    let applied = request("gate-applied", "session-b");
    let cancelled = request("gate-cancelled", "session-c");

    for gate in [
        &selected_first,
        &selected_second,
        &answered,
        &applied,
        &cancelled,
    ] {
        ledger
            .create_pending_transported((*gate).clone(), transport(gate))
            .unwrap();
    }
    ledger.create(plain).unwrap();
    ledger
        .record_answer(answer(&answered), timestamp())
        .unwrap();
    ledger.record_answer(answer(&applied), timestamp()).unwrap();
    ledger
        .mark_applied(&applied.gate_id, &applied.binding.session_id, timestamp())
        .unwrap();
    ledger
        .cancel(
            &cancelled.gate_id,
            &cancelled.binding.session_id,
            "operator cancelled",
            timestamp(),
        )
        .unwrap();

    let listed = store(&temp).list_pending_transported_gates().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|gate| (&gate.binding.session_id, &gate.gate_id))
            .collect::<Vec<_>>(),
        vec![
            (&selected_first.binding.session_id, &selected_first.gate_id),
            (
                &selected_second.binding.session_id,
                &selected_second.gate_id
            ),
        ]
    );
}

#[cfg(unix)]
#[test]
fn transport_listing_reuses_ledger_symlink_refusal() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("gates");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    symlink(&outside, &root).unwrap();

    let error = GateStore::at(root)
        .list_pending_transported_gates()
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing symlinked gate ledger path")
    );
    assert_eq!(std::fs::read(sentinel).unwrap(), b"untouched");
}
