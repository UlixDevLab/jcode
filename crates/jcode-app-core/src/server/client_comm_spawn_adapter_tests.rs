use crate::protocol::{MissionDispatchBinding, UntrustedDispatchIntent, UntrustedDispatchScope};

fn mission_binding() -> MissionDispatchBinding {
    MissionDispatchBinding {
        mission_id: "mission-3a".to_string(),
        revision: 7,
        artifact_hash: "sha256:artifact".to_string(),
        plan_hash: "sha256:plan".to_string(),
        scope_digest: "sha256:scope".to_string(),
        wave_id: "wave-3".to_string(),
        task_id: "task-a".to_string(),
        write_set: vec!["crates/jcode-app-core/src/server/comm_session.rs".to_string()],
        read_set: vec!["crates/jcode-app-core/src/server/client_lifecycle.rs".to_string()],
        entity_ids: vec!["entity-3a".to_string()],
        role: "forge".to_string(),
        spawn_mode: "headless".to_string(),
    }
}

fn dispatch_intent() -> UntrustedDispatchIntent {
    UntrustedDispatchIntent {
        wave_id: Some("wave-3".to_string()),
        task_id: Some("task-a".to_string()),
        role: Some("forge".to_string()),
        profile: Some("build".to_string()),
        requested_read_scopes: vec![UntrustedDispatchScope {
            path: "crates/jcode-app-core/src/server/client_lifecycle.rs".to_string(),
            entity_id: "reader-3a".to_string(),
        }],
        requested_write_scopes: vec![UntrustedDispatchScope {
            path: "crates/jcode-app-core/src/server/comm_session.rs".to_string(),
            entity_id: "writer-3a".to_string(),
        }],
    }
}

#[test]
fn normal_adapter_forwards_optional_dispatch_metadata_exactly() {
    let mission_binding = mission_binding();
    let dispatch_intent = dispatch_intent();

    let metadata = super::super::super::comm_session::SpawnMetadata::new(
        Some(mission_binding.clone()),
        Some(dispatch_intent.clone()),
    );

    assert_eq!(metadata.mission_binding, Some(mission_binding));
    assert_eq!(metadata.dispatch_intent, Some(dispatch_intent));
}

#[test]
fn lightweight_adapter_forwards_optional_dispatch_metadata_exactly() {
    let mission_binding = mission_binding();
    let dispatch_intent = dispatch_intent();

    let metadata = super::super::super::comm_session::SpawnMetadata::new(
        Some(mission_binding.clone()),
        Some(dispatch_intent.clone()),
    );

    assert_eq!(metadata.mission_binding, Some(mission_binding));
    assert_eq!(metadata.dispatch_intent, Some(dispatch_intent));
}

#[test]
fn legacy_adapters_preserve_omitted_dispatch_metadata() {
    let normal = super::super::super::comm_session::SpawnMetadata::new(None, None);
    let lightweight = super::super::super::comm_session::SpawnMetadata::new(None, None);

    assert_eq!(normal.mission_binding, None);
    assert_eq!(normal.dispatch_intent, None);
    assert_eq!(lightweight.mission_binding, None);
    assert_eq!(lightweight.dispatch_intent, None);
}
