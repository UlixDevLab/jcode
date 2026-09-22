use super::*;
use crate::jcode_control::{AuthenticatedPrincipal, ControlAction, ReceiptKey};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn config() -> HermesControlConfig {
    HermesControlConfig {
        api_base: "http://example.test/".to_string(),
        api_key: "key".to_string(),
        node: "node-a".to_string(),
        principal: "kitt-a".to_string(),
        root_id: "root-a".to_string(),
        root: std::env::current_dir().unwrap().canonicalize().unwrap(),
        route_alias: "build".to_string(),
        route_model: "model-a".to_string(),
        route_api_method: None,
        route_effort: None,
        poll_interval: Duration::from_secs(3),
    }
}

fn record(action: serde_json::Value) -> HermesControlRecord {
    serde_json::from_value(json!({
        "id":"request-a",
        "authenticated_principal":"kitt-a",
        "idempotency_key":"idem-a",
        "submission":action,
        "state":"pending",
        "created_at":"2026-01-01T00:00:00Z"
    }))
    .unwrap()
}

#[test]
fn maps_only_exact_configured_create_route_without_debug_payload_leaks() {
    let control = record(json!({
        "schema":"hermes.jcode_control/v1",
        "idempotency_key":"idem-a",
        "action":"create_session",
        "envelope_id":"env-a",
        "target_node":"node-a",
        "root_id":"root-a",
        "route_alias":"build",
        "initial_prompt":"secret prompt"
    }));
    let (_, request) = control.map_request(&config()).unwrap();
    assert!(matches!(
        request.action,
        ControlAction::CreateSession { .. }
    ));
    assert!(!format!("{control:?}").contains("secret prompt"));
    assert!(!format!("{request:?}").contains("secret prompt"));
}

#[test]
fn rejects_wrong_node_principal_root_route_and_unknown_action_fields() {
    for change in [
        json!({"target_node":"other"}),
        json!({"authenticated_principal":"other"}),
        json!({"root_id":"other"}),
        json!({"route_alias":"other"}),
    ] {
        let mut action = json!({
            "schema":"hermes.jcode_control/v1",
            "idempotency_key":"idem-a",
            "action":"create_session",
            "envelope_id":"env-a",
            "target_node":"node-a",
            "root_id":"root-a",
            "route_alias":"build"
        });
        for (key, value) in change.as_object().unwrap() {
            if key != "authenticated_principal" {
                action[key] = value.clone();
            }
        }
        let mut envelope = json!({
            "id":"request-a",
            "authenticated_principal":"kitt-a",
            "idempotency_key":"idem-a",
            "submission":action,
            "state":"pending",
            "created_at":"2026-01-01T00:00:00Z"
        });
        if let Some(value) = change.get("authenticated_principal") {
            envelope["authenticated_principal"] = value.clone();
        }
        assert!(
            serde_json::from_value::<HermesControlRecord>(envelope)
                .unwrap()
                .map_request(&config())
                .is_err()
        );
    }
    let unknown = serde_json::from_value::<HermesControlRecord>(json!({
        "id":"request-a",
        "authenticated_principal":"kitt-a",
        "idempotency_key":"idem-a",
        "state":"pending",
        "created_at":"now",
        "submission":{
            "schema":"hermes.jcode_control/v1",
            "idempotency_key":"idem-a",
            "action":"nope",
            "envelope_id":"env-a",
            "target_node":"node-a"
        }
    }))
    .unwrap();
    assert!(unknown.map_request(&config()).is_err());
    assert!(
        serde_json::from_value::<HermesSubmission>(json!({
            "schema":"hermes.jcode_control/v1",
            "idempotency_key":"idem-a",
            "action":"create_session",
            "envelope_id":"env-a",
            "target_node":"node-a",
            "root_id":"root-a",
            "route_alias":"build",
            "model":"forbidden"
        }))
        .is_err()
    );
}

#[test]
fn deterministic_session_identity_is_stable_per_authenticated_receipt_key() {
    let first = ReceiptKey::new(AuthenticatedPrincipal::new("kitt-a").unwrap(), "idem-a").unwrap();
    let replay = ReceiptKey::new(AuthenticatedPrincipal::new("kitt-a").unwrap(), "idem-a").unwrap();
    let changed =
        ReceiptKey::new(AuthenticatedPrincipal::new("kitt-a").unwrap(), "idem-b").unwrap();
    assert_eq!(
        executor::deterministic_session_id(&first),
        executor::deterministic_session_id(&replay)
    );
    assert_ne!(
        executor::deterministic_session_id(&first),
        executor::deterministic_session_id(&changed)
    );
}

#[test]
fn explicit_config_requires_existing_canonical_root() {
    assert!(
        PathBuf::from("/definitely/missing/jcode-control-root")
            .canonicalize()
            .is_err()
    );
}

async fn fake_hermes(
    responses: Vec<serde_json::Value>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for response_body in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 8192];
            let used = stream.read(&mut bytes).await.unwrap();
            requests.push(String::from_utf8_lossy(&bytes[..used]).into_owned());
            let body = response_body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        requests
    });
    (format!("http://{address}/"), task)
}

fn wire_record(action: &str) -> serde_json::Value {
    let mut submission = json!({
        "schema":"hermes.jcode_control/v1",
        "idempotency_key":"idem-a",
        "action":action,
        "envelope_id":"env-a",
        "target_node":"node-a"
    });
    match action {
        "message_existing" => {
            submission["jcode_session_id"] = json!("session-a");
            submission["message"] = json!("do not log this");
        }
        "create_session" => {
            submission["root_id"] = json!("root-a");
            submission["route_alias"] = json!("build");
            submission["initial_prompt"] = json!("do not log this either");
        }
        _ => unreachable!(),
    }
    json!({
        "id":"request-a",
        "authenticated_principal":"kitt-a",
        "idempotency_key":"idem-a",
        "submission":submission,
        "state":"claimed",
        "claimed_by":"jcode-a",
        "created_at":"2026-01-01T00:00:00Z",
        "claimed_at":"2026-01-01T00:00:01Z"
    })
}

async fn assert_fake_transport(action: &str) {
    let wire = wire_record(action);
    let (api_base, fake) = fake_hermes(vec![json!([wire.clone()]), wire.clone(), wire]).await;
    let mut configured = config();
    configured.api_base = api_base;
    let client = HermesControlClient::new(configured);
    let pending = client.pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    let claimed = client.claim(&pending[0].id).await.unwrap();
    assert!(matches!(claimed.map_request(&client.config), Ok(_)));
    client
        .resolve(
            &claimed.id,
            HermesResult {
                target_node: &client.config.node,
                status: "failed",
                session_id: None,
                error_code: Some("rejected"),
            },
        )
        .await
        .unwrap();
    let requests = fake.await.unwrap();
    assert!(requests[0].starts_with("GET /jcode-control/requests?"));
    assert!(requests[0].contains("target_node=node-a"));
    assert!(requests[1].starts_with("POST /jcode-control/requests/request-a/claim"));
    assert!(requests[2].starts_with("POST /jcode-control/requests/request-a/result"));
    assert!(!requests.join("\n").contains("do not log this"));
}

#[tokio::test]
async fn fake_hermes_transport_handles_message_existing_without_prompt_logging() {
    assert_fake_transport("message_existing").await;
}

#[tokio::test]
async fn fake_hermes_transport_handles_create_session_without_prompt_logging() {
    assert_fake_transport("create_session").await;
}
