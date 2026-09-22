use super::*;
use crate::tool::{ToolContext, ToolExecutionMode};
use std::ffi::OsString;

struct JcodeHomeGuard {
    previous: Option<OsString>,
}

impl JcodeHomeGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", path);
        Self { previous }
    }
}

impl Drop for JcodeHomeGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            crate::env::set_var("JCODE_HOME", previous);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

#[tokio::test]
async fn explicit_control_plane_session_hides_and_rejects_bash_but_keeps_coordination_tools() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut session = Session::create(None, None);
    session.tool_policy_mode = crate::config::SessionToolPolicyMode::ControlPlaneOnly;
    let session_id = session.id.clone();
    let mut agent = Agent::new_with_session(provider, registry, session, None);

    let names: HashSet<String> = agent
        .tool_definitions()
        .await
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(names.contains("swarm"));
    assert!(names.contains("todo"));
    assert!(!names.contains("bash"));
    assert!(agent.validate_tool_allowed("swarm").is_ok());
    assert!(agent.validate_tool_allowed("todo").is_ok());
    assert!(agent.validate_tool_allowed("bash").is_err());

    let result = agent
        .registry()
        .execute(
            "bash",
            serde_json::json!({"command": "true"}),
            ToolContext {
                session_id: session_id.clone(),
                message_id: "test".to_string(),
                tool_call_id: "test".to_string(),
                working_dir: Some(std::env::temp_dir()),
                stdin_request_tx: None,
                graceful_shutdown_signal: None,
                structural_review_authority: None,
                execution_mode: ToolExecutionMode::Direct,
            },
        )
        .await;
    crate::tool::clear_session_tool_policy(&session_id);
    assert!(
        result.is_err(),
        "hidden bash must be rejected by the registry too"
    );
}

#[tokio::test]
async fn normal_session_policy_preserves_bash() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let session = Session::create(None, None);
    let mut agent = Agent::new_with_session(
        provider,
        registry,
        session,
        Some(HashSet::from([
            "bash".to_string(),
            "swarm".to_string(),
            "todo".to_string(),
        ])),
    );

    let names: HashSet<String> = agent
        .tool_definitions()
        .await
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(names.contains("bash"));
    agent
        .validate_tool_allowed("bash")
        .expect("normal sessions must retain their allowed bash capability");
}

#[tokio::test]
async fn worker_constructor_forces_normal_policy_and_retains_bash() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new_worker_with_initial_working_dir(provider, registry, None);

    assert_eq!(
        agent.session_tool_policy_mode(),
        crate::config::SessionToolPolicyMode::Normal
    );
    assert!(
        agent
            .tool_definitions()
            .await
            .into_iter()
            .any(|tool| tool.name == "bash")
    );
}

#[tokio::test]
async fn context_scout_luna_profile_allows_only_bounded_read_only_discovery() {
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let working_dir = tempfile::TempDir::new().expect("working directory");
    let working_dir = working_dir.path().display().to_string();
    let mut agent = Agent::new_context_scout_luna_worker_with_initial_working_dir(
        provider,
        registry,
        Some(&working_dir),
    );
    let session_id = agent.session_id().to_string();

    assert_eq!(agent.working_dir(), Some(working_dir.as_str()));
    let names: HashSet<String> = agent
        .tool_definitions()
        .await
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    for allowed in [
        "read",
        "agentgrep",
        "ls",
        "jcode_docs",
        "webfetch",
        "websearch",
        "conversation_search",
        "session_search",
    ] {
        assert!(
            names.contains(allowed),
            "context scout must expose {allowed}"
        );
        assert!(agent.validate_tool_allowed(allowed).is_ok());
    }
    for denied in [
        "bash",
        "write",
        "edit",
        "multiedit",
        "patch",
        "apply_patch",
        "browser",
        "macos_computer_use",
        "consent_grant",
        "selfdev",
        "swarm",
        "schedule",
        "todo",
        "initiative",
        "future_unknown_tool",
    ] {
        assert!(!names.contains(denied), "context scout must hide {denied}");
        assert!(agent.validate_tool_allowed(denied).is_err());
    }

    let result = agent
        .registry()
        .execute(
            "bash",
            serde_json::json!({"command": "true"}),
            ToolContext {
                session_id: session_id.clone(),
                message_id: "test".to_string(),
                tool_call_id: "test".to_string(),
                working_dir: Some(std::env::temp_dir()),
                stdin_request_tx: None,
                graceful_shutdown_signal: None,
                structural_review_authority: None,
                execution_mode: ToolExecutionMode::Direct,
            },
        )
        .await;
    crate::tool::clear_session_tool_policy(&session_id);
    assert!(
        result.is_err(),
        "hidden bash must be rejected by the registry too"
    );
}

#[tokio::test]
async fn restored_control_plane_session_keeps_bash_hidden() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temporary JCODE_HOME");
    let _home = JcodeHomeGuard::set(temp_home.path());
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let mut session = Session::create(None, None);
    session.tool_policy_mode = crate::config::SessionToolPolicyMode::ControlPlaneOnly;
    let session_id = session.id.clone();
    // Upstream intentionally does not persist untouched empty sessions.
    // This test exercises restoration of a deliberately saved session.
    session.saved = true;
    session.save().expect("save control-plane session");

    agent
        .restore_session(&session.id)
        .expect("restore control-plane session");

    assert_eq!(
        agent.session_tool_policy_mode(),
        crate::config::SessionToolPolicyMode::ControlPlaneOnly
    );

    let names: HashSet<String> = agent
        .tool_definitions()
        .await
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(names.contains("swarm"));
    assert!(names.contains("todo"));
    assert!(!names.contains("bash"));
    assert!(agent.validate_tool_allowed("swarm").is_ok());
    assert!(agent.validate_tool_allowed("todo").is_ok());
    assert!(agent.validate_tool_allowed("bash").is_err());

    let result = agent
        .registry()
        .execute(
            "bash",
            serde_json::json!({"command": "true"}),
            ToolContext {
                session_id: session_id.clone(),
                message_id: "test".to_string(),
                tool_call_id: "test".to_string(),
                working_dir: Some(std::env::temp_dir()),
                stdin_request_tx: None,
                graceful_shutdown_signal: None,
                structural_review_authority: None,
                execution_mode: ToolExecutionMode::Direct,
            },
        )
        .await;
    crate::tool::clear_session_tool_policy(&session_id);
    assert!(
        result.is_err(),
        "restored control-plane session must keep rejecting bash at the registry too"
    );
}

#[tokio::test]
async fn restored_normal_session_retains_bash() {
    let _guard = crate::storage::lock_test_env();
    let temp_home = tempfile::TempDir::new().expect("temporary JCODE_HOME");
    let _home = JcodeHomeGuard::set(temp_home.path());
    let provider: Arc<dyn Provider> = Arc::new(NativeAutoCompactionProvider);
    let registry = Registry::new(provider.clone()).await;
    let mut agent = Agent::new(provider, registry);
    let mut session = Session::create(None, None);
    session.saved = true;
    session.save().expect("save normal session");

    agent
        .restore_session(&session.id)
        .expect("restore normal session");

    assert_eq!(
        agent.session_tool_policy_mode(),
        crate::config::SessionToolPolicyMode::Normal
    );
    assert!(
        agent
            .tool_definitions()
            .await
            .into_iter()
            .any(|tool| tool.name == "bash")
    );
}
