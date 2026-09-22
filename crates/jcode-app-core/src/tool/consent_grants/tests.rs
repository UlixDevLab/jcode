use super::tool::ConsentGrantTool;
use super::{
    BatchGrantManifest, BatchGrantStore, GrantMatcher, RegisteredGrantScope, registered_scope,
};
use crate::tool::consent::request_batch_grant_approval;
use crate::tool::{StdinInputRequest, Tool, ToolContext, ToolExecutionMode};
use chrono::{Duration, Utc};
use tokio::sync::mpsc;

fn scope(now: chrono::DateTime<Utc>, expires_at: chrono::DateTime<Utc>) -> RegisteredGrantScope {
    RegisteredGrantScope::for_test(
        "test-provider",
        "account-1",
        ["mutate"],
        ["resource-1"],
        2,
        now,
        expires_at,
    )
    .expect("typed test scope")
}

fn context(stdin_request_tx: Option<mpsc::UnboundedSender<StdinInputRequest>>) -> ToolContext {
    ToolContext {
        session_id: "session-a".to_string(),
        message_id: "message".to_string(),
        tool_call_id: "tool-call".to_string(),
        working_dir: None,
        stdin_request_tx,
        graceful_shutdown_signal: None,
        structural_review_authority: None,
        execution_mode: ToolExecutionMode::AgentTurn,
    }
}

async fn approved(manifest: BatchGrantManifest) -> crate::tool::consent::NativeBatchGrantApproval {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let task =
        tokio::spawn(
            async move { request_batch_grant_approval(&manifest, &context(Some(tx))).await },
        );
    let request = rx.recv().await.expect("native grant request");
    for expected in [
        "provider=test-provider",
        "principal=account-1",
        "actions=mutate",
        "resources=resource-1",
        "resource_count=1",
        "expires_at=",
        "manifest_digest=",
    ] {
        assert!(
            request.prompt.contains(expected),
            "grant prompt lacks {expected}"
        );
    }
    request
        .response_tx
        .send("approve".to_string())
        .expect("approve prompt");
    task.await.expect("approval task").expect("native approval")
}

#[tokio::test]
async fn persisted_grant_lifecycle_requires_exact_scope_and_native_approval() {
    let now = Utc::now();
    let scope = scope(now, now + Duration::minutes(60));
    let manifest = BatchGrantManifest::from_registered_scope(&scope).expect("validated manifest");
    assert!(
        request_batch_grant_approval(&manifest, &context(None))
            .await
            .is_err()
    );
    let store = BatchGrantStore::for_test().expect("private test store");

    let grant = store
        .create_after_native_approval(
            approved(manifest.clone()).await,
            "session-a",
            manifest.clone(),
        )
        .expect("approved grant persists");
    let reopened = store.reopen().expect("fresh process store");
    let matcher = GrantMatcher::from_registered_scope(&scope).expect("validated matcher");

    assert_eq!(
        reopened
            .list("session-a")
            .expect("fresh session read")
            .len(),
        1
    );

    reopened
        .consume("session-a", &matcher, now)
        .expect("first exact use allowed");
    assert!(reopened.consume("session-b", &matcher, now).is_err());
    reopened
        .consume("session-a", &matcher, now)
        .expect("second exact use allowed");
    assert!(reopened.consume("session-a", &matcher, now).is_err());
    assert_eq!(grant.remaining_uses, 2);
}

#[tokio::test]
async fn scope_mismatch_expiry_revocation_and_audit_redaction_fail_closed() {
    let now = Utc::now();
    let store = BatchGrantStore::for_test().expect("private test store");
    let active_scope = scope(now, now + Duration::minutes(60));
    let manifest = BatchGrantManifest::from_registered_scope(&active_scope).expect("manifest");
    let grant = store
        .create_after_native_approval(approved(manifest.clone()).await, "session-a", manifest)
        .expect("grant");
    store
        .consume(
            "session-a",
            &GrantMatcher::from_registered_scope(&active_scope).expect("active matcher"),
            now,
        )
        .expect("audit an accepted use");
    let wrong_scope = RegisteredGrantScope::for_test(
        "test-provider",
        "account-1",
        ["mutate"],
        ["resource-2"],
        2,
        now,
        now + Duration::minutes(60),
    )
    .expect("different scope");
    assert!(
        store
            .consume(
                "session-a",
                &GrantMatcher::from_registered_scope(&wrong_scope).expect("matcher"),
                now
            )
            .is_err()
    );
    store.revoke("session-a", &grant.id, now).expect("revoke");
    assert!(
        store
            .consume(
                "session-a",
                &GrantMatcher::from_registered_scope(&active_scope).expect("matcher"),
                now
            )
            .is_err()
    );

    let expired_scope = scope(now, now + Duration::minutes(1));
    let expired = BatchGrantManifest::from_registered_scope(&expired_scope)
        .expect("expiring manifest validates");
    store
        .create_after_native_approval(approved(expired.clone()).await, "session-a", expired)
        .expect("expiring grant persists for audited denial");
    assert!(
        store
            .consume(
                "session-a",
                &GrantMatcher::from_registered_scope(&expired_scope).expect("expired matcher"),
                now + Duration::hours(1)
            )
            .is_err()
    );

    let audit = store.audit_text().expect("audit text");
    assert!(
        audit.contains("created")
            && audit.contains("used")
            && audit.contains("revoked")
            && audit.contains("expired_denied")
    );
    for secret in ["resource-1", "resource-2", "account-1"] {
        assert!(!audit.contains(secret), "audit leaked {secret}");
    }
}

#[test]
fn typed_validation_permissions_and_empty_registration_deny_prohibited_families() {
    let now = Utc::now();
    assert!(
        RegisteredGrantScope::for_test(
            "test-provider",
            "account-1",
            ["mutate"],
            ["resource\nforged"],
            1,
            now,
            now + Duration::minutes(1)
        )
        .is_err()
    );
    for prohibited in [
        "gmail",
        "mcp",
        "browser",
        "bash",
        "macos_computer_use",
        "unknown",
    ] {
        assert!(
            registered_scope(prohibited).is_none(),
            "{prohibited} must not register a durable grant"
        );
    }
}

#[tokio::test]
async fn model_visible_consent_tool_rejects_unregistered_and_prohibited_requests() {
    let tool = ConsentGrantTool::new();
    for registration in [
        "gmail",
        "mcp",
        "browser",
        "bash",
        "macos_computer_use",
        "unknown",
    ] {
        let error = tool
            .execute(
                serde_json::json!({"action": "propose", "registration": registration}),
                context(None),
            )
            .await
            .expect_err("unregistered scope must not prompt or create a grant");
        assert!(error.to_string().contains("no durable batch-grant scope"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn persisted_files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let now = Utc::now();
    let store = BatchGrantStore::for_test().expect("store");
    let scope = scope(now, now + Duration::minutes(60));
    let manifest = BatchGrantManifest::from_registered_scope(&scope).expect("manifest");
    store
        .create_after_native_approval(approved(manifest.clone()).await, "session-a", manifest)
        .expect("grant");
    let (grants, audit) = store.storage_paths();
    assert_eq!(
        std::fs::metadata(grants)
            .expect("grant metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(audit)
            .expect("audit metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
