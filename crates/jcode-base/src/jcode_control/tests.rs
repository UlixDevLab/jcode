use super::*;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::new("hermes-user-42").unwrap()
}

fn message_request() -> ControlRequest {
    ControlRequest {
        schema: CONTROL_SCHEMA.to_string(),
        idempotency_key: "request-123".to_string(),
        action: ControlAction::MessageExisting {
            jcode_session_id: "session-456".to_string(),
            message: "Please continue the focused task.".to_string(),
            wake: false,
        },
    }
}

fn create_request() -> ControlRequest {
    ControlRequest {
        schema: CONTROL_SCHEMA.to_string(),
        idempotency_key: "request-789".to_string(),
        action: ControlAction::CreateSession {
            root_id: "project-main".to_string(),
            route_alias: "build".to_string(),
            initial_prompt: Some("Investigate this bounded task.".to_string()),
        },
    }
}

fn store(temp: &TempDir) -> ControlReceiptStore {
    ControlReceiptStore::at(temp.path().join("jcode-control")).unwrap()
}

#[test]
fn strict_schema_field_matrix_rejects_unknown_and_inapplicable_fields() {
    let valid_message = r#"{
        "schema":"jcode_control.v1",
        "idempotency_key":"request-123",
        "action":{"type":"message_existing","jcode_session_id":"session-456","message":"hello"}
    }"#;
    let parsed: ControlRequest = serde_json::from_str(valid_message).unwrap();
    assert!(matches!(
        parsed.action,
        ControlAction::MessageExisting { wake: false, .. }
    ));
    parsed.validate().unwrap();

    for invalid in [
        r#"{"schema":"jcode_control.v2","idempotency_key":"request-123","action":{"type":"message_existing","jcode_session_id":"session-456","message":"hello"}}"#,
        r#"{"schema":"jcode_control.v1","principal":"payload-is-forbidden","idempotency_key":"request-123","action":{"type":"message_existing","jcode_session_id":"session-456","message":"hello"}}"#,
        r#"{"schema":"jcode_control.v1","idempotency_key":"request-123","action":{"type":"message_existing","jcode_session_id":"session-456","message":"hello","root_id":"inapplicable"}}"#,
        r#"{"schema":"jcode_control.v1","idempotency_key":"request-123","action":{"type":"create_session","root_id":"project-main","route_alias":"build","wake":true}}"#,
        r#"{"schema":"jcode_control.v1","idempotency_key":"request-123","action":{"type":"shell","command":"echo nope"}}"#,
        r#"{"schema":"jcode_control.v1","idempotency_key":"request-123","action":{"type":"create_session","root_id":"../outside","route_alias":"build"}}"#,
        r#"{"schema":"jcode_control.v1","idempotency_key":"request-123","action":{"type":"create_session","root_id":"project-main","route_alias":"../../provider-key"}}"#,
    ] {
        assert!(
            serde_json::from_str::<ControlRequest>(invalid).is_err(),
            "{invalid}"
        );
    }

    let mut too_long = message_request();
    if let ControlAction::MessageExisting { message, .. } = &mut too_long.action {
        *message = "x".repeat(MAX_MESSAGE_CHARS + 1);
    }
    assert!(too_long.validate().is_err());
}

#[test]
fn receipt_replay_conflict_and_completion_are_durable_without_execution_claims() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    let principal = principal();
    let request = message_request();

    let received = store.receive(&principal, &request).unwrap();
    let receipt = match received {
        ReceiveOutcome::Received(receipt) => receipt,
        other => panic!("unexpected receive outcome: {other:?}"),
    };
    assert!(matches!(receipt.state, ControlReceiptState::Received));

    let claim = store.claim(&principal, &request, None).unwrap();
    let claimed = match claim {
        ClaimOutcome::Claimed(receipt) => receipt,
        other => panic!("unexpected claim outcome: {other:?}"),
    };
    assert!(matches!(claimed.state, ControlReceiptState::Claimed { .. }));
    assert!(claimed.execution_fingerprint.is_some());

    let replay = store.receive(&principal, &request).unwrap();
    assert!(matches!(replay, ReceiveOutcome::Replay(receipt) if receipt == claimed));

    let mut changed = request.clone();
    if let ControlAction::MessageExisting { message, .. } = &mut changed.action {
        *message = "This changed payload must conflict.".to_string();
    }
    let error = store.receive(&principal, &changed).unwrap_err();
    assert!(error.to_string().contains("conflicting idempotency replay"));

    let key = ReceiptKey::new(principal.clone(), request.idempotency_key.clone()).unwrap();
    let completed = store
        .complete(
            &key,
            ControlExecutionResult::new("queued", Some("higher layer result only".to_string()))
                .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        completed.state,
        ControlReceiptState::Completed { .. }
    ));
    assert!(matches!(
        store.receive(&principal, &request).unwrap(),
        ReceiveOutcome::Replay(receipt) if receipt == completed
    ));
}

#[test]
fn failed_result_is_durable_and_terminal_conflicts_fail_closed() {
    let temp = TempDir::new().unwrap();
    let store = store(&temp);
    let principal = principal();
    let request = message_request();
    store.claim(&principal, &request, None).unwrap();
    let key = ReceiptKey::new(principal, request.idempotency_key).unwrap();
    let failed =
        ControlExecutionResult::new("unavailable", Some("retry later".to_string())).unwrap();
    let receipt = store.fail(&key, failed.clone()).unwrap();
    assert!(matches!(receipt.state, ControlReceiptState::Failed { .. }));
    assert_eq!(store.fail(&key, failed).unwrap(), receipt);
    assert!(
        store
            .complete(&key, ControlExecutionResult::new("queued", None).unwrap())
            .is_err()
    );
}

#[test]
fn create_claim_pins_matching_root_route_and_restart_preserves_claim() {
    let temp = TempDir::new().unwrap();
    let principal = principal();
    let request = create_request();
    let receipt_store = store(&temp);
    let resolution = PinnedCreateSessionResolution::new("project-main", "build").unwrap();

    let claimed = receipt_store
        .claim(&principal, &request, Some(resolution.clone()))
        .unwrap();
    let receipt = match claimed {
        ClaimOutcome::Claimed(receipt) => receipt,
        other => panic!("unexpected claim outcome: {other:?}"),
    };
    assert!(matches!(
        receipt.state,
        ControlReceiptState::Claimed {
            pinned_create_session: Some(ref pinned)
        } if pinned == &resolution
    ));

    let restarted = store(&temp);
    assert!(matches!(
        restarted.claim(&principal, &request, Some(resolution)).unwrap(),
        ClaimOutcome::Replay(replayed) if replayed == receipt
    ));
    let changed_resolution = PinnedCreateSessionResolution::new("other-root", "build").unwrap();
    let error = restarted
        .claim(&principal, &request, Some(changed_resolution))
        .unwrap_err();
    assert!(error.to_string().contains("conflicting pinned resolution"));
}

#[test]
fn concurrent_claim_has_one_winner_and_path_or_symlink_escape_is_rejected() {
    let temp = TempDir::new().unwrap();
    let store = Arc::new(store(&temp));
    let authenticated_principal = principal();
    let request = message_request();
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let store = Arc::clone(&store);
        let principal = authenticated_principal.clone();
        let request = request.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            store.claim(&principal, &request, None).unwrap()
        }));
    }
    barrier.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::Claimed(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::Replay(_)))
            .count(),
        1
    );

    assert!(AuthenticatedPrincipal::new("../../outside").is_err());
    assert!(ReceiptKey::new(principal(), "../../outside").is_err());

    #[cfg(unix)]
    {
        let outside = TempDir::new().unwrap();
        let root = temp.path().join("symlinked-control");
        symlink(outside.path(), &root).unwrap();
        let unsafe_store = ControlReceiptStore::at(root).unwrap();
        let error = unsafe_store
            .receive(&principal(), &message_request())
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("refusing symlinked control receipt path")
        );
        assert!(!outside.path().join("hermes-user-42").exists());
    }
}

#[test]
fn debug_and_errors_redact_request_content() {
    let request = ControlRequest {
        action: ControlAction::MessageExisting {
            jcode_session_id: "session-456".to_string(),
            message: "super-secret-message".to_string(),
            wake: false,
        },
        ..message_request()
    };
    let principal = AuthenticatedPrincipal::new("private-principal").unwrap();
    let temp = TempDir::new().unwrap();
    let receipt = match store(&temp).receive(&principal, &request).unwrap() {
        ReceiveOutcome::Received(receipt) => receipt,
        ReceiveOutcome::Replay(_) => unreachable!(),
    };
    let rendered = format!("{receipt:?}");
    assert!(!rendered.contains("super-secret-message"));
    assert!(!rendered.contains("private-principal"));
    assert!(!rendered.contains("request-123"));
}
