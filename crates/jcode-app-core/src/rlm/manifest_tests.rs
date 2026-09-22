use super::*;
use crate::message::{ContentBlock, Role};

fn selection_entry(id: &str) -> crate::rlm::selector::SelectionEntry {
    crate::rlm::selector::SelectionEntry {
        id: id.to_string(),
        reason: "test".to_string(),
        content_hash: format!("hash-{id}"),
        token_estimate: 10,
    }
}

fn test_manifest() -> CallManifest {
    let session = Session::create_with_id("rlm-status".into(), None, None);
    CallManifest {
        schema_version: SCHEMA_VERSION,
        session_id: session.id.clone(),
        call_id: "call".to_string(),
        flow: "streaming".to_string(),
        requested_mode: "pilot".to_string(),
        effective_mode: "pilot".to_string(),
        provider_message_count: 0,
        provider_char_estimate: 0,
        provider_token_estimate: 0,
        provider_context_hash: "context".to_string(),
        ordered_message_hashes: Vec::new(),
        message_hashes_truncated: false,
        counterfactual: CounterfactualSelection::default(),
        typed_state: build_state(&session),
    }
}

#[test]
fn status_snapshot_reports_actual_pilot_reduction() {
    let mut manifest = test_manifest();
    manifest.effective_mode = "pilot".to_string();
    manifest.provider_message_count = 2;
    manifest.provider_token_estimate = 900;
    manifest.counterfactual.included = vec![selection_entry("a"), selection_entry("b")];
    manifest.counterfactual.excluded = vec![selection_entry("c"), selection_entry("d")];

    let status = RlmStatusSnapshot::from_manifest(&manifest);
    assert!(status.selected);
    assert_eq!(status.provider_message_count, 2);
    assert_eq!(status.baseline_message_count, 4);
    assert_eq!(status.token_estimate, 900);
}

#[test]
fn status_snapshot_marks_observe_as_baseline() {
    let mut manifest = test_manifest();
    manifest.effective_mode = "observe".to_string();
    manifest.provider_message_count = 4;
    manifest.counterfactual.included = vec![selection_entry("a"), selection_entry("b")];
    manifest.counterfactual.excluded = vec![selection_entry("c"), selection_entry("d")];

    let status = RlmStatusSnapshot::from_manifest(&manifest);
    assert!(!status.selected);
    assert_eq!(status.baseline_message_count, 4);
}

#[test]
fn observe_manifest_is_redacted_deterministic_and_does_not_mutate_messages() {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temp home");
    let previous = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", home.path());
    let mut session = Session::create_with_id("rlm-observe".into(), None, None);
    session.add_message(
        Role::User,
        vec![ContentBlock::Text {
            text: "private prompt".into(),
            cache_control: None,
        }],
    );
    let messages: Vec<Message> = session
        .messages
        .iter()
        .map(crate::session::StoredMessage::to_message)
        .collect();
    let before = serde_json::to_vec(&messages).expect("serialize before");
    let outcome = observe_with_policy(
        &session,
        &messages,
        "streaming",
        1,
        RlmMode::Observe,
        RlmMode::Observe,
        12,
        None,
    )
    .expect("observe");
    let ObservationOutcome::Written(path) = outcome else {
        panic!("expected write");
    };
    let raw = fs::read_to_string(path).expect("read manifest");
    assert!(!raw.contains("private prompt"));
    let manifest: CallManifest = serde_json::from_str(&raw).expect("parse manifest");
    assert!(
        manifest
            .counterfactual
            .included
            .iter()
            .any(|entry| entry.reason == "required-current-user")
    );
    assert!(
        manifest
            .counterfactual
            .included
            .iter()
            .all(|entry| !entry.content_hash.is_empty())
    );
    assert_eq!(
        before,
        serde_json::to_vec(&messages).expect("serialize after")
    );
    assert_eq!(
        observe_with_policy(
            &session,
            &messages,
            "streaming",
            1,
            RlmMode::Observe,
            RlmMode::Observe,
            12,
            None,
        )
        .expect("repeat observe"),
        ObservationOutcome::Unchanged
    );
    match previous {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}

#[test]
fn observe_mode_preserves_original_vectors_for_both_dispatch_flows() {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temp home");
    let previous = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", home.path());
    let session = Session::create_with_id("rlm-flow-equivalence".into(), None, None);
    let messages = vec![Message::user("same provider vector")];
    let before = serde_json::to_vec(&messages).expect("serialize before");

    for (flow, call_index) in [("streaming", 1), ("non-streaming", 2)] {
        observe_with_policy(
            &session,
            &messages,
            flow,
            call_index,
            RlmMode::Observe,
            RlmMode::Observe,
            12,
            None,
        )
        .expect("observe flow");
        assert_eq!(
            before,
            serde_json::to_vec(&messages).expect("serialize after")
        );
    }
    match previous {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}

#[test]
fn pilot_manifest_preserves_the_original_selection_plan() {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temp home");
    let previous = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", home.path());
    let session = Session::create_with_id("rlm-pilot-selection".into(), None, None);
    let messages = vec![Message::user("selected provider vector")];
    let selection = CounterfactualSelection {
        token_budget: 24_000,
        token_estimate: 777,
        integrity_error: Some("original-plan-marker".into()),
        ..CounterfactualSelection::default()
    };
    let outcome = observe_with_policy(
        &session,
        &messages,
        "non-streaming",
        1,
        RlmMode::Pilot,
        RlmMode::Pilot,
        12,
        Some(&selection),
    )
    .expect("observe pilot");
    let ObservationOutcome::Written(path) = outcome else {
        panic!("expected write");
    };
    let manifest: CallManifest = crate::storage::read_json(&path).expect("read manifest");
    assert_eq!(manifest.counterfactual, selection);
    match previous {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}

#[test]
fn manifest_loading_rejects_path_traversal() {
    assert!(load_session_manifests("../sessions").is_err());
    assert!(load_session_manifests("nested/session").is_err());
}

#[test]
fn older_observe_manifests_default_counterfactual_state() {
    let session = Session::create_with_id("legacy".into(), None, None);
    let current = CallManifest {
        schema_version: 1,
        session_id: session.id.clone(),
        call_id: "legacy-call".into(),
        flow: "streaming".into(),
        requested_mode: "observe".into(),
        effective_mode: "observe".into(),
        provider_message_count: 1,
        provider_char_estimate: 4,
        provider_token_estimate: 1,
        provider_context_hash: "hash".into(),
        ordered_message_hashes: vec!["hash".into()],
        message_hashes_truncated: false,
        counterfactual: CounterfactualSelection::default(),
        typed_state: build_state(&session),
    };
    let mut value = serde_json::to_value(current).expect("serialize current manifest");
    value
        .as_object_mut()
        .expect("manifest object")
        .remove("counterfactual");
    let manifest: CallManifest = serde_json::from_value(value).expect("legacy manifest parses");
    assert_eq!(manifest.counterfactual, CounterfactualSelection::default());
}

#[test]
fn retention_is_bounded_by_provider_call_limit() {
    let _lock = crate::storage::lock_test_env();
    let home = tempfile::tempdir().expect("temp home");
    let previous = std::env::var_os("JCODE_HOME");
    crate::env::set_var("JCODE_HOME", home.path());
    let session = Session::create_with_id("rlm-retention".into(), None, None);
    for index in 0..5 {
        let content = format!("call {index}");
        let messages = vec![Message::user(&content)];
        observe_with_policy(
            &session,
            &messages,
            "non-streaming",
            index,
            RlmMode::Observe,
            RlmMode::Observe,
            2,
            None,
        )
        .expect("observe");
    }
    assert_eq!(
        fs::read_dir(manifest_directory(&session.id).expect("directory"))
            .expect("read directory")
            .count(),
        2
    );
    match previous {
        Some(value) => crate::env::set_var("JCODE_HOME", value),
        None => crate::env::remove_var("JCODE_HOME"),
    }
}
