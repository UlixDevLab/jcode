use super::*;
use crate::gates::{
    DeliveryAcknowledgement, GateContinuation, GateOption, GateQuestion, GateState,
    GateTransportBinding,
};
use chrono::Duration as ChronoDuration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn gate(binding: GateBinding) -> GateRequest {
    let gate_id = "gate_hermes_ingress".to_string();
    let transport = GateTransportBinding::for_gate(&gate_id, binding.clone()).unwrap();
    GateRequest {
        schema_version: 2,
        gate_id,
        binding,
        question: GateQuestion {
            question: "Proceed?".to_string(),
            problem: "A decision is needed.".to_string(),
            impact: "The next step is gated.".to_string(),
            options: vec![GateOption {
                label: "Approve".to_string(),
                description: "Continue.".to_string(),
            }],
            recommendation: "Approve.".to_string(),
        },
        allowed_decisions: vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject,
        ],
        continuation: GateContinuation {
            on_approve: "continue".to_string(),
            on_request_changes: "change".to_string(),
            on_reject: "stop".to_string(),
            on_expiry: "expire".to_string(),
        },
        created_at: now(),
        expires_at: now() + ChronoDuration::minutes(10),
        delivery: DeliveryAcknowledgement::NotAttempted,
        state: GateState::Pending,
        transport_binding: Some(transport),
        resume_directive: None,
    }
}

fn receipt_for(gate: &GateRequest) -> serde_json::Value {
    let transport = gate.transport_binding.as_ref().unwrap();
    serde_json::json!({
        "envelope_id": transport.envelope_id,
        "gate_id": gate.gate_id,
        "jcode_session_id": gate.binding.session_id,
        "subject": gate.binding.subject,
        "decision": "approve",
        "comment": null,
        "answered_at": now(),
        "receipt_id": "receipt-1",
        "idempotency_key": "idempotency-1"
    })
}

async fn fake_server(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 8192];
        let len = stream.read(&mut request).await.unwrap();
        let request = String::from_utf8_lossy(&request[..len]).to_string();
        let response = format!(
            "HTTP/1.1 {status} test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (format!("http://{address}"), task)
}

fn client(api_base: String) -> HermesGateAnswerClient {
    HermesGateAnswerClient::new(HermesGateIngressConfig {
        api_base: normalize_api_base(&api_base),
        api_key: "test-hermes-key".to_string(),
        poll_interval: DEFAULT_POLL_INTERVAL,
    })
}

#[tokio::test]
async fn valid_session_decision_receipt_records_an_unconsumed_directive() {
    let temp = tempfile::tempdir().unwrap();
    let store = GateStore::at(temp.path());
    let gate = gate(GateBinding::session_decision(
        "session_hermes",
        4,
        "packet-hash",
    ));
    store
        .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
        .unwrap();
    let (base, server) = fake_server(200, receipt_for(&gate).to_string()).await;

    let recorded = client(base).poll_once(&store, now()).await;
    assert_eq!(recorded.len(), 1);
    let loaded = store.load(&gate.gate_id, &gate.binding.session_id).unwrap();
    assert!(matches!(loaded.state, GateState::Answered { .. }));
    assert!(loaded.resume_directive.is_some());
    let request = server.await.unwrap();
    assert!(request.starts_with("GET /envelopes/jcode-gate-"));
    assert!(request.contains("/gate-answer HTTP/1.1"));
    assert!(
        request.contains("x-hermes-key: test-hermes-key")
            || request.contains("X-Hermes-Key: test-hermes-key")
    );
}

#[tokio::test]
async fn valid_mission_plan_receipt_records_once_and_404_is_a_noop() {
    let temp = tempfile::tempdir().unwrap();
    let store = GateStore::at(temp.path());
    let gate = gate(GateBinding::mission_plan(
        "session_mission",
        "mission_hermes",
        7,
        "plan-hash",
    ));
    store
        .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
        .unwrap();
    let (base, server) = fake_server(404, String::new()).await;
    assert!(client(base).poll_once(&store, now()).await.is_empty());
    server.await.unwrap();
    assert!(matches!(
        store
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Pending
    ));

    let (base, server) = fake_server(200, receipt_for(&gate).to_string()).await;
    assert_eq!(client(base).poll_once(&store, now()).await.len(), 1);
    server.await.unwrap();
    assert!(matches!(
        store
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Answered { .. }
    ));
}

#[tokio::test]
async fn auth_server_network_and_expiry_failures_leave_the_gate_pending_or_expired() {
    let temp = tempfile::tempdir().unwrap();
    let store = GateStore::at(temp.path());
    let gate = gate(GateBinding::session_decision(
        "session_failure",
        2,
        "failure-hash",
    ));
    store
        .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
        .unwrap();

    let (base, server) = fake_server(401, "unauthorized".to_string()).await;
    assert!(client(base).poll_once(&store, now()).await.is_empty());
    server.await.unwrap();
    assert!(matches!(
        store
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Pending
    ));
    assert!(
        client("http://127.0.0.1:1".to_string())
            .poll_once(&store, now())
            .await
            .is_empty()
    );

    let (base, server) = fake_server(200, receipt_for(&gate).to_string()).await;
    assert!(
        client(base)
            .poll_once(&store, now() + ChronoDuration::minutes(11))
            .await
            .is_empty()
    );
    server.await.unwrap();
    assert!(matches!(
        store
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .state,
        GateState::Expired { .. }
    ));
}

#[test]
fn mismatched_or_invalid_receipts_never_become_answers() {
    let gate = gate(GateBinding::session_decision(
        "session_exact",
        3,
        "hash-exact",
    ));
    let base = receipt_for(&gate);
    for (field, replacement) in [
        ("envelope_id", serde_json::json!("wrong-envelope")),
        ("gate_id", serde_json::json!("wrong-gate")),
        ("jcode_session_id", serde_json::json!("wrong-session")),
        (
            "subject",
            serde_json::json!({"kind":"session_decision","packet_revision":99,"packet_hash":"hash-exact"}),
        ),
        (
            "subject",
            serde_json::json!({"kind":"session_decision","packet_revision":3,"packet_hash":"wrong-hash"}),
        ),
        ("decision", serde_json::json!("unknown")),
        ("comment", serde_json::json!("\u{0000}")),
    ] {
        let mut receipt = base.clone();
        receipt[field] = replacement;
        let parsed: Result<HermesGateAnswerReceipt, _> = serde_json::from_value(receipt);
        assert!(
            parsed
                .ok()
                .and_then(|receipt| answer_for_gate(&gate, receipt))
                .is_none()
        );
    }
}
