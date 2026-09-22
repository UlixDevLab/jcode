use super::*;
use crate::tool::ToolExecutionMode;
use serde_json::json;
use tokio::sync::mpsc;

fn operation() -> ProtectedOperation {
    ProtectedOperation::new(
        "bash",
        "credentialed network request",
        "https://example.test",
    )
}

fn browser_operation() -> ClassifiedOperation {
    protected_operation("browser", &json!({"action": "click"}))
        .expect("browser operation is classified")
}

fn desktop_input_operation() -> ClassifiedOperation {
    protected_operation("macos_computer_use", &json!({"action": "type"}))
        .expect("desktop input is classified")
}

fn context(stdin_request_tx: Option<mpsc::UnboundedSender<StdinInputRequest>>) -> ToolContext {
    ToolContext {
        session_id: "session".to_string(),
        message_id: "message".to_string(),
        tool_call_id: "tool-call".to_string(),
        working_dir: None,
        stdin_request_tx,
        graceful_shutdown_signal: None,
        structural_review_authority: None,
        execution_mode: ToolExecutionMode::AgentTurn,
    }
}

#[test]
fn classification_covers_protected_and_safe_paths() {
    assert!(protected_operation("browser", &json!({"action": "status"})).is_none());
    assert!(protected_operation("browser", &json!({"action": "open"})).is_some());
    let url = json!({"target": "https://example.test/path?token=secret"});
    assert!(protected_operation("open", &url).is_some());
    assert!(protected_operation("open", &json!({"target": "/Applications/Arc.app"})).is_some());
    assert!(protected_operation("open", &json!({"target": "Google Chrome"})).is_some());
    assert_eq!(
        safe_url_summary("https://alice:secret@example.test/path?token=secret"),
        "https://example.test/path"
    );
    assert_eq!(
        safe_url_summary("mailto:person@example.test"),
        "mailto:<redacted>"
    );
    assert_eq!(truncate_safe("line\n\u{0000}text", 120), "linetext");
    assert!(protected_operation("open", &json!({"target": "/tmp/report.txt"})).is_some());
    assert!(protected_operation("macos_computer_use", &json!({"action": "screenshot"})).is_some());
    assert!(protected_operation("macos_computer_use", &json!({"action": "setup"})).is_some());
    assert!(
        protected_operation("macos_computer_use", &json!({"action": "find_element"})).is_some()
    );
    for action in [
        "type",
        "key",
        "press",
        "set_value",
        "select_menu",
        "activate_app",
        "set_clipboard",
        "run_applescript",
        "run_jxa",
    ] {
        assert!(
            protected_operation("macos_computer_use", &json!({"action": action})).is_some(),
            "{action} must not bypass native consent"
        );
    }
    assert!(
        protected_operation(
            "macos_computer_use",
            &json!({"action": "check_permissions"})
        )
        .is_none()
    );
    assert!(protected_operation("macos_computer_use", &json!({"action": "discover"})).is_none());
    let safari = json!({"action": "activate_app", "app": "Safari"});
    assert!(protected_operation("macos_computer_use", &safari).is_some());
}

#[path = "consent_navigation_tests.rs"]
mod navigation_tests;

#[path = "consent_boundary_tests.rs"]
mod boundary_tests;

#[tokio::test]
async fn denial_blocks_equivalent_routes_until_next_direct_user_turn() {
    let session_id = crate::id::new_id("consent-test-session");
    begin_user_turn(&session_id);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut denied_ctx = context(Some(tx));
    denied_ctx.session_id = session_id.clone();
    let denied_operation = browser_operation();
    let denied_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(
            &denied_operation,
            &denied_ctx,
            Duration::from_secs(1),
        )
        .await
    });
    rx.recv()
        .await
        .expect("consent request")
        .response_tx
        .send("deny".to_string())
        .unwrap();
    assert!(
        denied_task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("denied")
    );

    let mut bypass_ctx = context(None);
    bypass_ctx.session_id = session_id.clone();
    let bypass = browser_operation();
    let error =
        request_classified_operation_with_timeout(&bypass, &bypass_ctx, Duration::from_millis(1))
            .await
            .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("blocked until the next direct user message")
    );

    begin_user_turn(&session_id);
    let (tx, mut rx) = mpsc::unbounded_channel();
    bypass_ctx.stdin_request_tx = Some(tx);
    let retry_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&bypass, &bypass_ctx, Duration::from_secs(1))
            .await
    });
    rx.recv()
        .await
        .expect("fresh prompt on next direct user turn")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(retry_task.await.unwrap().is_ok());
}

#[tokio::test]
async fn technical_consent_failures_do_not_create_denial_cooldowns() {
    let no_client_session = crate::id::new_id("consent-no-client-session");
    begin_user_turn(&no_client_session);
    let mut no_client = context(None);
    no_client.session_id = no_client_session.clone();
    let error = request_classified_operation_with_timeout(
        &browser_operation(),
        &no_client,
        Duration::from_millis(1),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("no current TUI client"));

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut recovered = context(Some(tx));
    recovered.session_id = no_client_session;
    let retry = browser_operation();
    let task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&retry, &recovered, Duration::from_secs(1)).await
    });
    rx.recv()
        .await
        .expect("recovered approval route receives a fresh prompt")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(task.await.unwrap().is_ok());

    let timeout_session = crate::id::new_id("consent-timeout-session");
    begin_user_turn(&timeout_session);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut timing_out = context(Some(tx));
    timing_out.session_id = timeout_session.clone();
    let timed_operation = desktop_input_operation();
    let timed_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(
            &timed_operation,
            &timing_out,
            Duration::from_millis(10),
        )
        .await
    });
    let request = rx.recv().await.expect("consent request before timeout");
    let error = timed_task.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("timed out"));
    drop(request);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut recovered = context(Some(tx));
    recovered.session_id = timeout_session;
    let retry = desktop_input_operation();
    let task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&retry, &recovered, Duration::from_secs(1)).await
    });
    rx.recv()
        .await
        .expect("timeout must not block a recovered route until another user turn")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(task.await.unwrap().is_ok());
}

#[tokio::test]
async fn pending_prompt_blocks_concurrent_equivalent_route_without_second_prompt() {
    let session_id = crate::id::new_id("consent-test-session");
    begin_user_turn(&session_id);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut first_ctx = context(Some(tx));
    first_ctx.session_id = session_id.clone();
    let first = desktop_input_operation();
    let first_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&first, &first_ctx, Duration::from_secs(1)).await
    });
    let first_request = rx.recv().await.expect("first consent request");

    let mut bypass_ctx = context(None);
    bypass_ctx.session_id = session_id;
    let bypass = desktop_input_operation();
    let error =
        request_classified_operation_with_timeout(&bypass, &bypass_ctx, Duration::from_millis(1))
            .await
            .unwrap_err();
    assert!(error.to_string().contains("approval is already pending"));
    assert!(
        rx.try_recv().is_err(),
        "equivalent route must not open a second prompt"
    );

    first_request
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(first_task.await.unwrap().is_ok());
}

#[tokio::test]
async fn consent_fails_closed_without_client() {
    let error = request_current_user_consent_with_timeout(
        &operation(),
        &context(None),
        Duration::from_millis(1),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("no current TUI client"));
}

#[tokio::test]
async fn consent_accepts_only_affirmative_tui_response() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let ctx = context(Some(tx));
    let task = tokio::spawn(async move {
        request_current_user_consent_with_timeout(&operation(), &ctx, Duration::from_secs(1)).await
    });
    let request = rx.recv().await.expect("consent request");
    assert!(request.prompt.starts_with(CONSENT_PROMPT_PREFIX));
    assert!(
        request
            .prompt
            .contains("action=credentialed network request")
    );
    request.response_tx.send("approve".to_string()).unwrap();
    assert!(task.await.unwrap().is_ok());
}

#[tokio::test]
async fn consent_rejects_negative_and_timeout_responses() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let ctx = context(Some(tx));
    let task = tokio::spawn(async move {
        request_current_user_consent_with_timeout(&operation(), &ctx, Duration::from_secs(1)).await
    });
    rx.recv()
        .await
        .expect("consent request")
        .response_tx
        .send("deny".to_string())
        .unwrap();
    assert!(
        task.await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("denied")
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let ctx = context(Some(tx));
    let task = tokio::spawn(async move {
        request_current_user_consent_with_timeout(&operation(), &ctx, Duration::from_millis(1))
            .await
    });
    let _request = rx.recv().await.expect("consent request");
    assert!(
        task.await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}

/// The `permission` hook is the ONE honest "jcode needs a human" signal, so
/// what matters is not that it fires but that it fires in a MATCHED pair: a
/// supervisor arms a waiting indicator on "waiting" and can only disarm it on
/// "resolved". A denial or a timeout is exactly the case where a naive
/// implementation returns early and leaves the indicator stuck on, so the
/// denial path is the one asserted here rather than the happy path.
///
/// The hook is configured by ENV, which is process-global, so the sibling
/// consent tests (plain #[tokio::test], no env lock) can post to the same
/// script while this one runs. The recorded lines are therefore keyed to a
/// tool name unique to this test rather than assumed to be the only ones.
#[cfg(unix)]
#[tokio::test]
async fn permission_hook_pairs_waiting_with_resolved_even_when_denied() {
    let _guard = crate::storage::lock_test_env();
    let temp = tempfile::TempDir::new().expect("temp dir");
    let record = temp.path().join("events.txt");
    // Delay the first hook process. The old independent observer dispatches
    // the resolved process immediately after it, making the inversion
    // deterministic. The ordered dispatcher must still let the denied
    // consent return immediately while preserving the file's lifecycle order.
    let script_path = temp.path().join("permission.sh");
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\nif [ \"$JCODE_HOOK_STATE\" = waiting ]; then sleep 0.2; fi\nprintf '%s:%s:%s\\n' \"$JCODE_HOOK_STATE\" \"$JCODE_HOOK_TOOL_NAME\" \"$JCODE_HOOK_OUTCOME\" >> {}\n",
            jcode_base::terminal_launch::sh_escape(&record.to_string_lossy())
        ),
    )
    .expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");
    }

    let prev = std::env::var_os("JCODE_HOOK_PERMISSION");
    jcode_base::env::set_var(
        "JCODE_HOOK_PERMISSION",
        script_path.to_string_lossy().to_string(),
    );
    crate::config::invalidate_config_cache();

    // A tool name only this test uses, so concurrent sibling consent tests
    // sharing the env-configured script cannot be mistaken for our pair.
    let marked = ProtectedOperation::new(
        "permission-hook-probe",
        "credentialed network request",
        "https://example.test",
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let ctx = context(Some(tx));
    let task = tokio::spawn(async move {
        request_current_user_consent_with_timeout(&marked, &ctx, Duration::from_secs(1)).await
    });
    let request = rx.recv().await.expect("consent request");
    let response_started = std::time::Instant::now();
    request.response_tx.send("deny".to_string()).unwrap();
    let outcome = task.await.unwrap();
    let consent_resolved_without_hook_delay =
        response_started.elapsed() < std::time::Duration::from_millis(100);

    let mut ours: Vec<String> = Vec::new();
    for _ in 0..100 {
        if let Ok(data) = std::fs::read_to_string(&record) {
            ours = data
                .lines()
                .filter(|line| line.contains("permission-hook-probe"))
                .map(str::to_owned)
                .collect();
            if ours.len() >= 2 {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    match prev {
        Some(value) => jcode_base::env::set_var("JCODE_HOOK_PERMISSION", value),
        None => jcode_base::env::remove_var("JCODE_HOOK_PERMISSION"),
    }
    crate::config::invalidate_config_cache();

    assert!(
        outcome.is_err(),
        "a denied consent must still fail the call"
    );
    assert!(
        consent_resolved_without_hook_delay,
        "a delayed observer hook must not block the user's denied response"
    );
    assert_eq!(ours.len(), 2, "expected exactly one pair, got {ours:?}");
    // "waiting" carries no outcome yet — the wait has not ended.
    assert_eq!(ours[0], "waiting:permission-hook-probe:");
    assert_eq!(ours[1], "resolved:permission-hook-probe:denied");
}
