use std::{ffi::OsString, sync::MutexGuard};

use crate::{
    message::{ContentBlock, Role},
    session::Session,
    shadow_context_state::{
        EmissionOutcome, ShadowContextState, artifact_path, build_state,
        maybe_emit_for_completed_turn,
    },
};

const FLAG: &str = "JCODE_SHADOW_CONTEXT_STATE";

struct TestEnv {
    _lock: MutexGuard<'static, ()>,
    _home: tempfile::TempDir,
    previous_home: Option<OsString>,
    previous_flag: Option<OsString>,
}

impl TestEnv {
    fn new(enabled: bool) -> Self {
        let lock = crate::storage::lock_test_env();
        let home = tempfile::tempdir().expect("temporary JCODE_HOME");
        let previous_home = std::env::var_os("JCODE_HOME");
        let previous_flag = std::env::var_os(FLAG);
        crate::env::set_var("JCODE_HOME", home.path());
        if enabled {
            crate::env::set_var(FLAG, "1");
        } else {
            crate::env::remove_var(FLAG);
        }
        Self {
            _lock: lock,
            _home: home,
            previous_home,
            previous_flag,
        }
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        restore_env("JCODE_HOME", self.previous_home.take());
        restore_env(FLAG, self.previous_flag.take());
    }
}

fn restore_env(key: &str, previous: Option<OsString>) {
    match previous {
        Some(value) => crate::env::set_var(key, value),
        None => crate::env::remove_var(key),
    }
}

fn completed_session(id: &str) -> Session {
    let mut session = Session::create_with_id(id.to_string(), None, Some("t".to_string()));
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "Do the task".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::Text {
            text: "Decision: use the safe path".to_string(),
            cache_control: None,
        }],
    );
    session.add_message(
        Role::User,
        vec![ContentBlock::ToolResult {
            tool_use_id: "tu1".to_string(),
            content: "failed: first attempt".to_string(),
            is_error: Some(true),
        }],
    );
    session.add_message(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: "tu2".to_string(),
            name: "read_file".to_string(),
            input: serde_json::json!({
                "path": "/tmp/data.txt",
                "secret": "./secret_token.env",
                "nested": ["/safe/path.json"]
            }),
            thought_signature: None,
        }],
    );
    session
}

#[test]
fn default_off_writes_no_artifact() {
    let _env = TestEnv::new(false);
    let session = completed_session("shadow-off");
    assert_eq!(
        maybe_emit_for_completed_turn(&session).expect("disabled emission"),
        EmissionOutcome::Disabled
    );
    assert!(!artifact_path(&session.id).expect("artifact path").exists());
}

#[test]
fn enabled_writes_a_bounded_redacted_session_artifact() {
    let _env = TestEnv::new(true);
    let session = completed_session("shadow-on");
    assert_eq!(
        maybe_emit_for_completed_turn(&session).expect("enabled emission"),
        EmissionOutcome::Written
    );
    let path = artifact_path(&session.id).expect("artifact path");
    let raw = std::fs::read_to_string(path).expect("read artifact");
    let state: ShadowContextState = serde_json::from_str(&raw).expect("parse artifact");

    assert_eq!(state.schema_version, 1);
    assert_eq!(state.session_id, "shadow-on");
    assert_eq!(state.decisions.len(), 1);
    assert_eq!(state.failed_approaches.len(), 1);
    assert!(state.artifact_references.len() <= 16);
    assert!(
        state
            .artifact_references
            .iter()
            .all(|item| item.path_hash.len() == 64)
    );
    for forbidden in [
        "Do the task",
        "Decision: use",
        "failed: first",
        "/tmp/data.txt",
        "/safe/path.json",
        "secret_token",
    ] {
        assert!(!raw.contains(forbidden), "artifact leaked {forbidden}");
    }
}

#[test]
fn session_isolation_uses_distinct_artifacts() {
    let _env = TestEnv::new(true);
    let a = completed_session("iso-a");
    let b = completed_session("iso-b");
    maybe_emit_for_completed_turn(&a).expect("emit a");
    maybe_emit_for_completed_turn(&b).expect("emit b");
    let path_a = artifact_path(&a.id).expect("path a");
    let path_b = artifact_path(&b.id).expect("path b");
    assert_ne!(path_a, path_b);
    assert!(path_a.exists() && path_b.exists());
}

#[test]
fn duplicate_write_is_suppressed_and_corruption_recovers() {
    let _env = TestEnv::new(true);
    let session = completed_session("recovery");
    assert_eq!(
        maybe_emit_for_completed_turn(&session).expect("first emit"),
        EmissionOutcome::Written
    );
    assert_eq!(
        maybe_emit_for_completed_turn(&session).expect("duplicate emit"),
        EmissionOutcome::Unchanged
    );
    let path = artifact_path(&session.id).expect("path");
    std::fs::write(&path, b"not-json").expect("corrupt artifact");
    assert_eq!(
        maybe_emit_for_completed_turn(&session).expect("recovery emit"),
        EmissionOutcome::Written
    );
    let raw = std::fs::read(&path).expect("read recovered artifact");
    serde_json::from_slice::<ShadowContextState>(&raw).expect("valid recovered artifact");
}

#[test]
fn signal_and_artifact_bounds_are_deterministic() {
    let _env = TestEnv::new(true);
    let mut session = completed_session("bounds");
    for index in 0..40 {
        session.add_message(
            Role::Assistant,
            vec![
                ContentBlock::Text {
                    text: format!("Decision: option {index}"),
                    cache_control: None,
                },
                ContentBlock::ToolUse {
                    id: format!("tool-{index}"),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": format!("/data/file-{index}.bin")}),
                    thought_signature: None,
                },
            ],
        );
    }
    let first = build_state(&session);
    let second = build_state(&session);
    assert_eq!(first, second);
    assert!(first.decisions.len() <= 12);
    assert!(first.failed_approaches.len() <= 12);
    assert!(first.artifact_references.len() <= 16);
}

#[test]
fn observation_does_not_mutate_session_or_provider_projection() {
    let _env = TestEnv::new(true);
    let session = completed_session("immutable");
    let before = serde_json::to_vec(&session.messages).expect("serialize before");
    maybe_emit_for_completed_turn(&session).expect("emit observation");
    let after = serde_json::to_vec(&session.messages).expect("serialize after");
    assert_eq!(before, after);
}
