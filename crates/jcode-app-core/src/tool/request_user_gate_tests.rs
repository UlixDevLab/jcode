use super::request_user_gate::{
    PACKET_REVISION, RequestUserGateTool, deterministic_gate_id, packet_hash,
};
use super::{Tool, ToolContext, ToolExecutionMode};
use chrono::{Duration, Utc};
use jcode_base::gates::{GateDecision, GateStore, GateSubject};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::time::Instant;

struct JcodeHomeGuard {
    previous: Option<OsString>,
}

impl JcodeHomeGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("JCODE_HOME");
        // Tests hold `jcode_base::storage::lock_test_env` while changing this
        // process-global environment variable.
        unsafe { std::env::set_var("JCODE_HOME", path) };
        Self { previous }
    }
}

impl Drop for JcodeHomeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => unsafe { std::env::set_var("JCODE_HOME", previous) },
            None => unsafe { std::env::remove_var("JCODE_HOME") },
        }
    }
}

fn context(tool_call_id: &str) -> ToolContext {
    ToolContext {
        session_id: "session-request-gate-test".to_string(),
        message_id: "message-request-gate-test".to_string(),
        tool_call_id: tool_call_id.to_string(),
        working_dir: None,
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        structural_review_authority: None,
        execution_mode: ToolExecutionMode::Direct,
    }
}

fn input() -> Value {
    json!({
        "question": "Which rollout should we use?",
        "context": {
            "problem": "The new flow needs a safe release path.",
            "impact": "A wrong choice could delay the launch."
        },
        "options": [
            {"label": "phased", "description": "Enable for a small cohort first."},
            {"label": "full", "description": "Enable for everyone immediately."}
        ],
        "recommendation": "Use the phased rollout.",
        "narrows": "R1",
        "affects_constraints": ["R1"],
        "allowed_decisions": ["approve", "request_changes", "reject"],
        "expires_at": (Utc::now() + Duration::hours(1)).to_rfc3339(),
        "continuation": {
            "on_approve": "Proceed with the recommendation.",
            "on_request_changes": "Revise the rollout proposal.",
            "on_reject": "Keep the current rollout unchanged.",
            "on_expiry": "Escalate the unresolved rollout decision."
        },
        "intent": "Ask the owner to choose the rollout."
    })
}

#[test]
fn schema_exposes_only_high_level_fields_and_the_registry_adds_intent() {
    let tool = RequestUserGateTool::new();
    let schema = tool.parameters_schema();
    let properties = schema["properties"].as_object().expect("properties");
    for forbidden in [
        "session_id",
        "binding",
        "packet_hash",
        "mission_id",
        "transport",
        "gate_id",
        "request_id",
    ] {
        assert!(
            !properties.contains_key(forbidden),
            "schema exposed {forbidden}"
        );
    }
    for expected in [
        "question",
        "context",
        "options",
        "recommendation",
        "narrows",
        "affects_constraints",
        "allowed_decisions",
        "expires_at",
        "continuation",
    ] {
        assert!(
            properties.contains_key(expected),
            "schema omitted {expected}"
        );
    }
    assert!(!properties.contains_key("intent"));
    let required = schema["required"].as_array().expect("required fields");
    for expected in ["narrows", "affects_constraints"] {
        assert!(
            required.iter().any(|value| value == expected),
            "schema did not require {expected}"
        );
    }
    assert!(
        tool.to_definition().input_schema["properties"]
            .get("intent")
            .is_some()
    );
}

#[path = "request_user_gate_decision_tests.rs"]
mod decision_tests;

#[test]
fn deterministic_ids_and_packet_hash_are_versioned_and_content_sensitive() {
    let tool_call = "tool-call-123";
    let gate_id = deterministic_gate_id("session-a", tool_call);
    assert_eq!(gate_id, deterministic_gate_id("session-a", tool_call));
    assert_ne!(gate_id, deterministic_gate_id("session-b", tool_call));
    assert_ne!(gate_id, deterministic_gate_id("session-a", "tool-call-456"));
    assert!(gate_id.starts_with("request-user-gate-v1-"));

    let created_at = Utc::now();
    let expires_at = created_at + Duration::hours(1);
    let first = packet_hash(&gate_id, "session-a", &input(), created_at, expires_at)
        .expect("first packet hash");
    let second = packet_hash(&gate_id, "session-a", &input(), created_at, expires_at)
        .expect("second packet hash");
    let mut changed_input = input();
    changed_input["question"] = json!("a different question");
    let changed = packet_hash(
        &gate_id,
        "session-a",
        &changed_input,
        created_at,
        expires_at,
    )
    .expect("changed packet hash");
    let mut changed_narrowing = input();
    changed_narrowing["narrows"] = json!("C1");
    let changed_narrowing = packet_hash(
        &gate_id,
        "session-a",
        &changed_narrowing,
        created_at,
        expires_at,
    )
    .expect("narrowing-bound packet hash");
    let mut changed_constraints = input();
    changed_constraints["affects_constraints"] = json!(["R1", "C1"]);
    let changed_constraints = packet_hash(
        &gate_id,
        "session-a",
        &changed_constraints,
        created_at,
        expires_at,
    )
    .expect("constraint-bound packet hash");
    let changed_expiry = packet_hash(
        &gate_id,
        "session-a",
        &input(),
        created_at,
        expires_at + Duration::seconds(1),
    )
    .expect("expiry-bound packet hash");
    assert_eq!(first, second);
    assert_ne!(first, changed);
    assert_ne!(first, changed_narrowing);
    assert_ne!(first, changed_constraints);
    assert_ne!(first, changed_expiry);
    assert!(first.starts_with("sha256:v1:"));
    assert_eq!(PACKET_REVISION, 1);
}

#[tokio::test]
async fn validates_empty_duplicate_options_disallowed_decisions_and_expiry() {
    let tool = RequestUserGateTool::new();
    for (mut invalid, expected) in [
        (json!({"options": []}), "at least one"),
        (
            json!({
                "options": [
                    {"label": "same", "description": "first"},
                    {"label": "same", "description": "second"}
                ]
            }),
            "duplicate",
        ),
        (
            json!({"options": [{"label": "valid", "description": " "}]}),
            "invalid gate option description",
        ),
        (json!({"allowed_decisions": []}), "at least one"),
        (
            json!({"allowed_decisions": ["approve", "approve"]}),
            "duplicate",
        ),
        (
            json!({"allowed_decisions": ["not_a_decision"]}),
            "unknown variant",
        ),
        (
            json!({"expires_at": (Utc::now() - Duration::minutes(1)).to_rfc3339()}),
            "future",
        ),
        (
            json!({"expires_at": (Utc::now() + Duration::days(31)).to_rfc3339()}),
            "30 days",
        ),
    ] {
        let mut request = input();
        for (key, value) in invalid.as_object_mut().expect("object") {
            request[key] = value.take();
        }
        let error = tool
            .execute(request, context(&format!("invalid-{expected}")))
            .await
            .expect_err("invalid request must fail closed");
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
}

#[tokio::test]
async fn rejects_untrusted_identity_binding_and_transport_input_fields() {
    let tool = RequestUserGateTool::new();
    for forbidden in [
        "session_id",
        "binding",
        "packet_hash",
        "mission_id",
        "transport",
        "gate_id",
        "request_id",
    ] {
        let mut request = input();
        request[forbidden] = json!("model-controlled");
        let error = tool
            .execute(request, context(&format!("forbidden-{forbidden}")))
            .await
            .expect_err("trusted field must be rejected");
        assert!(
            format!("{error:#}").contains(forbidden),
            "error did not name rejected {forbidden}: {error:#}"
        );
    }
}

#[tokio::test]
async fn persists_a_pending_transported_session_decision_and_returns_waiting_metadata() {
    let _env = jcode_base::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temporary JCODE_HOME");
    let _home = JcodeHomeGuard::set(home.path());
    let request = input();
    let ctx = context("persisted-call");
    let started = Instant::now();
    let output = RequestUserGateTool::new()
        .execute(request.clone(), ctx.clone())
        .await
        .expect("create gate");
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let metadata = output.metadata.expect("waiting metadata");
    assert_eq!(metadata["status"], "waiting");
    assert_eq!(metadata["continuation_pending"], true);
    assert_eq!(metadata["request_id"], metadata["gate_id"]);

    let gate_id = metadata["gate_id"].as_str().expect("gate id");
    let stored = GateStore::from_jcode_home()
        .expect("reopen store")
        .load(gate_id, &ctx.session_id)
        .expect("stored gate remains visible after reopening store");
    assert!(stored.transport_binding.is_some());
    assert_eq!(stored.question.question, request["question"]);
    assert_eq!(stored.binding.session_id, ctx.session_id);
    assert!(matches!(
        stored.binding.subject,
        GateSubject::SessionDecision {
            packet_revision: PACKET_REVISION,
            ..
        }
    ));
    assert_eq!(
        stored.allowed_decisions,
        vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject
        ]
    );
}

#[tokio::test]
async fn exact_replay_is_idempotent_but_changed_content_conflicts() {
    let _env = jcode_base::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temporary JCODE_HOME");
    let _home = JcodeHomeGuard::set(home.path());
    let tool = RequestUserGateTool::new();
    let ctx = context("replay-call");
    let request = input();
    let first = tool
        .execute(request.clone(), ctx.clone())
        .await
        .expect("first execution");
    let replay = tool
        .execute(request.clone(), ctx.clone())
        .await
        .expect("exact replay");
    assert_eq!(first.metadata, replay.metadata);

    let mut changed = request;
    changed["recommendation"] = json!("Use the full rollout.");
    let error = tool
        .execute(changed, ctx)
        .await
        .expect_err("changed replay must not overwrite the durable gate");
    assert!(
        error
            .to_string()
            .contains("conflicting request_user_gate replay")
    );
}
