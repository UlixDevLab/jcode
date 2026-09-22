use super::*;
use crate::gates::{
    GateBinding, GateContinuation, GateOption, GateQuestion, GateState, GateTransportBinding,
};
use chrono::Duration as ChronoDuration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn gate(gate_id: &str, binding: GateBinding) -> GateRequest {
    let transport = GateTransportBinding::for_gate(gate_id, binding.clone()).unwrap();
    GateRequest {
        schema_version: 2,
        gate_id: gate_id.to_string(),
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
        allowed_decisions: vec![GateDecision::Approve, GateDecision::Reject],
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

fn client(api_base: String) -> HermesGateRequestClient {
    HermesGateRequestClient::new(HermesGateEgressConfig {
        api_base: normalize_api_base(&api_base),
        api_key: "test-hermes-key".to_string(),
        poll_interval: DEFAULT_POLL_INTERVAL,
        request_timeout: REQUEST_TIMEOUT,
    })
}

fn persisted_response(client: &HermesGateRequestClient, gate: &GateRequest) -> String {
    let envelope = client.envelope_for(gate).unwrap();
    serde_json::json!({
        "id": envelope.id,
        "created_by": envelope.created_by,
        "title": envelope.title,
        "task_title": envelope.task_title,
        "target_executor": TARGET_EXECUTOR,
        "status": AWAITING_CONFIRM,
        "gate_request": envelope.gate_request,
    })
    .to_string()
}

async fn fake_server(status: u16, body: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 16 * 1024];
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

fn assert_v2_post(request: &str, gate: &GateRequest) {
    assert!(request.starts_with("POST /envelopes HTTP/1.1"));
    assert!(
        request.contains("x-hermes-key: test-hermes-key")
            || request.contains("X-Hermes-Key: test-hermes-key")
    );
    let body = request.split("\r\n\r\n").nth(1).unwrap();
    let envelope: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        envelope
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "created_by",
            "gate_request",
            "id",
            "status",
            "target_executor",
            "task_title",
            "title"
        ]
    );
    assert_eq!(envelope["created_by"], "jcode");
    assert_eq!(envelope["target_executor"], TARGET_EXECUTOR);
    assert_eq!(envelope["status"], AWAITING_CONFIRM);
    let request = &envelope["gate_request"];
    assert_eq!(request["schema"], "hermes.gate_request/v2");
    assert_eq!(request["gate_id"], gate.gate_id);
    assert_eq!(request["jcode_session_id"], gate.binding.session_id);
    assert_eq!(request["question"], gate.question.question);
    assert_eq!(request["problem"], gate.question.problem);
    assert_eq!(request["recommendation"], gate.question.recommendation);
    assert_eq!(
        request["idempotency_key"],
        gate.transport_binding.as_ref().unwrap().envelope_id
    );
    assert_eq!(
        request["created_at"],
        serde_json::to_value(gate.created_at).unwrap()
    );
    assert_eq!(
        request["expires_at"],
        serde_json::to_value(gate.expires_at).unwrap()
    );
    assert_eq!(
        request["subject"],
        serde_json::to_value(&gate.binding.subject).unwrap()
    );
    assert_eq!(
        request["options"][0],
        serde_json::json!({"decision":"approve","label":"Approve"})
    );
    assert_eq!(
        request["options"][1],
        serde_json::json!({"decision":"reject","label":"Reject"})
    );
}

#[tokio::test]
async fn publishes_exact_v2_envelopes_for_session_and_mission_subjects_without_answering() {
    let temp = tempfile::tempdir().unwrap();
    let store = GateStore::at(temp.path());
    for (gate_id, binding) in [
        (
            "gate_egress_session",
            GateBinding::session_decision("session_egress", 4, "packet-hash"),
        ),
        (
            "gate_egress_mission",
            GateBinding::mission_plan("session_mission", "mission_egress", 7, "plan-hash"),
        ),
    ] {
        let gate = gate(gate_id, binding);
        store
            .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
            .unwrap();
        let provisional = client("http://127.0.0.1:1".to_string());
        let (base, server) = fake_server(201, persisted_response(&provisional, &gate)).await;
        let client = client(base);

        client.publish_once(&store, now()).await;
        let loaded = store.load(&gate.gate_id, &gate.binding.session_id).unwrap();
        assert!(matches!(
            loaded.delivery,
            DeliveryAcknowledgement::Acknowledged { .. }
        ));
        assert!(matches!(loaded.state, GateState::Pending));
        assert!(loaded.resume_directive.is_none());
        assert_v2_post(&server.await.unwrap(), &gate);

        // The one-shot server is gone. A second delivery attempt would turn the
        // acknowledgement into unknown, so retaining it proves zero repeat POSTs.
        client.publish_once(&store, now()).await;
        assert!(matches!(
            store
                .load(&gate.gate_id, &gate.binding.session_id)
                .unwrap()
                .delivery,
            DeliveryAcknowledgement::Acknowledged { .. }
        ));
    }
}

#[tokio::test]
async fn unconfirmed_or_malformed_or_network_responses_become_unknown_without_retries() {
    for (name, status, body) in [
        ("auth", 401, "{}"),
        ("conflict", 409, "{}"),
        ("server", 500, "{}"),
        ("malformed", 201, "not-json"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = GateStore::at(temp.path());
        let gate = gate(
            &format!("gate_egress_{name}"),
            GateBinding::session_decision(format!("session_{name}"), 1, "packet-hash"),
        );
        store
            .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
            .unwrap();
        let (base, server) = fake_server(status, body.to_string()).await;
        let client = client(base);

        client.publish_once(&store, now()).await;
        assert!(matches!(
            store
                .load(&gate.gate_id, &gate.binding.session_id)
                .unwrap()
                .delivery,
            DeliveryAcknowledgement::DeliveryUnknown { .. }
        ));
        assert!(matches!(
            store
                .load(&gate.gate_id, &gate.binding.session_id)
                .unwrap()
                .state,
            GateState::Pending
        ));
        server.await.unwrap();

        client.publish_once(&store, now()).await;
        assert!(matches!(
            store
                .load(&gate.gate_id, &gate.binding.session_id)
                .unwrap()
                .delivery,
            DeliveryAcknowledgement::DeliveryUnknown { .. }
        ));
    }

    let temp = tempfile::tempdir().unwrap();
    let store = GateStore::at(temp.path());
    let gate = gate(
        "gate_egress_network",
        GateBinding::session_decision("session_network", 1, "packet-hash"),
    );
    store
        .create_pending_transported(gate.clone(), gate.transport_binding.clone().unwrap())
        .unwrap();
    client("http://127.0.0.1:1".to_string())
        .publish_once(&store, now())
        .await;
    assert!(matches!(
        store
            .load(&gate.gate_id, &gate.binding.session_id)
            .unwrap()
            .delivery,
        DeliveryAcknowledgement::DeliveryUnknown { .. }
    ));
}

#[test]
fn disabled_when_url_or_key_is_absent() {
    assert!(HermesGateEgressConfig::from_values(None, Some("key".to_string())).is_none());
    assert!(
        HermesGateEgressConfig::from_values(Some("http://example.test".to_string()), None)
            .is_none()
    );
    assert!(
        HermesGateEgressConfig::from_values(
            Some("http://example.test".to_string()),
            Some("   ".to_string()),
        )
        .is_none()
    );
}
