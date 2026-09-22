use super::session_activity_context_snapshot;
use crate::protocol::SessionActivitySnapshot;
use chrono::{TimeZone, Utc};

#[test]
fn session_activity_context_snapshot_uses_safe_workspace_and_bounded_label() {
    let snapshot = session_activity_context_snapshot(
        Some("/Users/example/private-project"),
        Utc.with_ymd_and_hms(2026, 9, 2, 11, 48, 49).unwrap(),
        Some(&SessionActivitySnapshot {
            is_processing: true,
            current_tool_name: Some("mcp__private_server__retrieve_secret".to_string()),
        }),
    )
    .expect("safe activity context");

    assert_eq!(snapshot.repository, "private-project");
    assert_eq!(snapshot.started_at, "2026-09-02T11:48:49+00:00");
    assert_eq!(snapshot.activity_label, "Working");

    let encoded = serde_json::to_string(&snapshot).expect("serialize activity context");
    assert!(!encoded.contains("/Users/example"));
    assert!(!encoded.contains("private_server"));
    assert!(!encoded.contains("retrieve_secret"));
}

#[test]
fn session_activity_context_snapshot_hides_missing_or_unsafe_workspaces() {
    let started_at = Utc.with_ymd_and_hms(2026, 9, 2, 11, 48, 49).unwrap();

    assert_eq!(
        session_activity_context_snapshot(None, started_at, None),
        None
    );
    assert_eq!(
        session_activity_context_snapshot(
            Some("/Users/example/private workspace"),
            started_at,
            None
        ),
        None
    );
}
