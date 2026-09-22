use super::*;
use async_trait::async_trait;
use std::sync::Arc;

struct HarmlessBrowserTool;

#[async_trait]
impl crate::tool::Tool for HarmlessBrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "test-only browser consumer"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: crate::tool::ToolContext,
    ) -> anyhow::Result<crate::tool::ToolOutput> {
        Ok(crate::tool::ToolOutput::new("harmless browser consumer"))
    }
}

#[tokio::test]
async fn approved_browser_boundary_suppresses_second_real_registry_prompt() {
    let registry = crate::tool::Registry::empty();
    registry.tools.write().await.insert(
        "browser".to_string(),
        Arc::new(HarmlessBrowserTool) as Arc<dyn crate::tool::Tool>,
    );
    let session_id = crate::id::new_id("consent-boundary-session");
    begin_user_turn(&session_id);

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut first_ctx = context(Some(tx));
    first_ctx.session_id = session_id.clone();
    let first_registry = registry.clone();
    let first_task = tokio::spawn(async move {
        first_registry
            .execute(
                "browser",
                json!({"action": "click", "intent": "test browser boundary"}),
                first_ctx,
            )
            .await
    });
    let request = rx.recv().await.expect("first browser boundary prompt");
    assert!(request.prompt.contains("class=browser"));
    assert!(request.prompt.contains("scope=session"));
    assert!(request.prompt.contains("expires_at="));
    request.response_tx.send("approve".to_string()).unwrap();
    assert!(first_task.await.unwrap().is_ok());

    begin_user_turn(&session_id);
    let mut second_ctx = context(None);
    second_ctx.session_id = session_id;
    assert!(
        registry
            .execute(
                "browser",
                json!({"action": "click", "intent": "same class must reuse"}),
                second_ctx,
            )
            .await
            .is_ok(),
        "the real registry consumer must reuse a genuine unexpired browser boundary"
    );
    assert!(rx.try_recv().is_err(), "reuse must not fire another prompt");
}

#[tokio::test]
async fn expiry_unknown_class_and_session_scope_never_reuse_another_approval() {
    let session_id = crate::id::new_id("consent-expiry-session");
    begin_user_turn(&session_id);
    let operation = browser_operation();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut first_ctx = context(Some(tx));
    first_ctx.session_id = session_id.clone();
    let first_operation = operation.clone();
    let first_task = tokio::spawn(async move {
        request_classified_operation_with_timeout_and_boundary_ttl(
            &first_operation,
            &first_ctx,
            Duration::from_secs(1),
            Duration::from_millis(1),
        )
        .await
    });
    rx.recv()
        .await
        .expect("genuine browser approval")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(first_task.await.unwrap().is_ok());

    tokio::time::sleep(Duration::from_millis(5)).await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut expired_ctx = context(Some(tx));
    expired_ctx.session_id = session_id.clone();
    let expired_operation = operation.clone();
    let expired_task = tokio::spawn(async move {
        request_classified_operation_with_timeout_and_boundary_ttl(
            &expired_operation,
            &expired_ctx,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .await
    });
    rx.recv()
        .await
        .expect("expired boundary must require a fresh prompt")
        .response_tx
        .send("deny".to_string())
        .unwrap();
    assert!(expired_task.await.unwrap().is_err());

    begin_user_turn(&session_id);
    let mut other_session = context(None);
    other_session.session_id = crate::id::new_id("other-session");
    assert!(
        request_classified_operation_with_timeout(
            &operation,
            &other_session,
            Duration::from_millis(1)
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("no current TUI client"),
        "a session approval must not cross to another session"
    );

    let unknown =
        ClassifiedOperation::from_native(ProtectedOperation::new("mcp", "mystery", "test"));
    let unknown_session = crate::id::new_id("unknown-consent-session");
    begin_user_turn(&unknown_session);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut unknown_ctx = context(Some(tx));
    unknown_ctx.session_id = unknown_session.clone();
    let unknown_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&unknown, &unknown_ctx, Duration::from_secs(1))
            .await
    });
    rx.recv()
        .await
        .expect("unknown operation needs genuine one-shot approval")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(unknown_task.await.unwrap().is_ok());
    begin_user_turn(&unknown_session);
    let mut unknown_reuse_ctx = context(None);
    unknown_reuse_ctx.session_id = unknown_session;
    let unknown_again =
        ClassifiedOperation::from_native(ProtectedOperation::new("mcp", "mystery", "test"));
    assert!(
        request_classified_operation_with_timeout(
            &unknown_again,
            &unknown_reuse_ctx,
            Duration::from_millis(1),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("no current TUI client"),
        "unknown classes must never acquire reusable authority"
    );
}

#[tokio::test]
async fn approved_browser_boundary_does_not_authorize_desktop_input() {
    let session_id = crate::id::new_id("consent-cross-class-session");
    begin_user_turn(&session_id);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser_ctx = context(Some(tx));
    browser_ctx.session_id = session_id.clone();
    let browser = browser_operation();
    let browser_task = tokio::spawn(async move {
        request_classified_operation_with_timeout(&browser, &browser_ctx, Duration::from_secs(1))
            .await
    });
    rx.recv()
        .await
        .expect("browser approval prompt")
        .response_tx
        .send("approve".to_string())
        .unwrap();
    assert!(browser_task.await.unwrap().is_ok());

    let mut desktop_ctx = context(None);
    desktop_ctx.session_id = session_id;
    assert!(
        request_classified_operation_with_timeout(
            &desktop_input_operation(),
            &desktop_ctx,
            Duration::from_millis(1),
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("no current TUI client"),
        "browser approval must not silently broaden to desktop-input"
    );
}
