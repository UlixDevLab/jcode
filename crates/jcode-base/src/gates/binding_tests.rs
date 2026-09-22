use super::tests::{answer, request, store, timestamp};
use super::*;
use tempfile::TempDir;

#[test]
fn typed_binding_decodes_v1_mission_plan_and_serializes_v2_subject() {
    let v1 = serde_json::json!({
        "session_id": "session-v1",
        "mission_id": "mission-v1",
        "plan_revision": 3,
        "plan_hash": "b3:legacy-plan"
    });
    let decoded: GateBinding = serde_json::from_value(v1).unwrap();
    assert_eq!(
        decoded,
        GateBinding::mission_plan("session-v1", "mission-v1", 3, "b3:legacy-plan")
    );

    let v2 = serde_json::to_value(decoded).unwrap();
    assert_eq!(v2["subject"]["kind"], "mission_plan");
    assert!(v2.get("mission_id").is_none());
    assert!(v2.get("plan_hash").is_none());
}

#[test]
fn typed_binding_v2_session_decision_round_trips_without_mission_fields() {
    let binding = GateBinding::session_decision("session-decision", 9, "b3:packet");
    let serialized = serde_json::to_value(&binding).unwrap();
    assert_eq!(serialized["subject"]["kind"], "session_decision");
    assert_eq!(serialized["subject"]["packet_hash"], "b3:packet");
    assert!(serialized.get("mission_id").is_none());
    assert!(serialized.get("plan_hash").is_none());
    assert_eq!(
        serde_json::from_value::<GateBinding>(serialized).unwrap(),
        binding
    );
}

#[test]
fn typed_binding_v1_request_and_answer_decode_to_mission_plan() {
    let gate = request();
    let mut request_v1 = serde_json::to_value(&gate).unwrap();
    request_v1["schema_version"] = serde_json::json!(1);
    let subject = request_v1
        .as_object_mut()
        .unwrap()
        .remove("subject")
        .unwrap();
    for field in ["mission_id", "plan_revision", "plan_hash"] {
        request_v1[field] = subject[field].clone();
    }
    let decoded_request: GateRequest = serde_json::from_value(request_v1).unwrap();
    decoded_request.validate().unwrap();
    assert!(matches!(
        decoded_request.binding.subject,
        GateSubject::MissionPlan { .. }
    ));

    let mut answer_v1 = serde_json::to_value(answer(&gate, GateDecision::Approve)).unwrap();
    answer_v1["schema_version"] = serde_json::json!(1);
    let subject = answer_v1
        .as_object_mut()
        .unwrap()
        .remove("subject")
        .unwrap();
    for field in ["mission_id", "plan_revision", "plan_hash"] {
        answer_v1[field] = subject[field].clone();
    }
    let decoded_answer: AnswerRecord = serde_json::from_value(answer_v1).unwrap();
    decoded_answer.validate().unwrap();
    assert_eq!(decoded_answer.binding, decoded_request.binding);
}

#[test]
fn typed_binding_v2_session_decision_persists_across_restart_and_answer() {
    let temp = TempDir::new().unwrap();
    let mut gate = request();
    gate.binding = GateBinding::session_decision("session-456", 7, "b3:packet");
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    assert_eq!(
        store(&temp)
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap(),
        gate
    );

    let approved = answer(&gate, GateDecision::Approve);
    assert_eq!(
        ledger.ingest_answer(approved.clone(), timestamp()).unwrap(),
        AnswerIngestOutcome::Answered
    );
    assert!(matches!(
        store(&temp)
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Answered { answer } if answer == approved
    ));
}

#[test]
fn typed_binding_rejects_missing_or_ambiguous_subjects() {
    for invalid in [
        serde_json::json!({ "session_id": "session-missing" }),
        serde_json::json!({
            "session_id": "session-ambiguous",
            "subject": {
                "kind": "mission_plan",
                "mission_id": "mission-ambiguous",
                "plan_revision": 1,
                "plan_hash": "b3:plan"
            },
            "mission_id": "mission-legacy",
            "plan_revision": 1,
            "plan_hash": "b3:legacy-plan"
        }),
        serde_json::json!({
            "session_id": "session-packet-as-plan",
            "subject": {
                "kind": "session_decision",
                "packet_revision": 1,
                "plan_hash": "b3:packet"
            }
        }),
        serde_json::json!({
            "session_id": "session-no-mission",
            "plan_revision": 1,
            "plan_hash": "b3:plan"
        }),
    ] {
        assert!(serde_json::from_value::<GateBinding>(invalid).is_err());
    }
    assert!(
        GateBinding::mission_plan("session-empty", "", 1, "b3:plan")
            .validate()
            .is_err()
    );

    let mut gate = serde_json::to_value(request()).unwrap();
    gate["unknown_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<GateRequest>(gate).is_err());
}

#[test]
fn typed_binding_requires_exact_subject_for_answers_and_denies_cross_subject_replay() {
    let temp = TempDir::new().unwrap();
    let mut gate = request();
    gate.binding = GateBinding::session_decision("session-456", 7, "b3:packet");
    let ledger = store(&temp);
    ledger.create(gate.clone()).unwrap();
    let exact = answer(&gate, GateDecision::Approve);
    gate.validate_answer(&exact).unwrap();

    let mut cross_subject = exact;
    cross_subject.binding = GateBinding::mission_plan("session-456", "mission-789", 7, "b3:packet");
    assert!(gate.validate_answer(&cross_subject).is_err());
    assert!(ledger.ingest_answer(cross_subject, timestamp()).is_err());
    assert!(matches!(
        ledger
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Pending
    ));
}
