use jcode_desktop_core::{
    ApplicationMetadata, ConnectionStatus, DeliveryState, DesktopEffect, DesktopEnvelope,
    DesktopEvent, DesktopIntent, DesktopPatch, DesktopState, MessageRole, MessageView,
    SessionMetadata, reduce,
};

fn demo_state() -> DesktopState {
    DesktopState::bootstrap(ApplicationMetadata::demo())
}

#[test]
fn bootstrap_and_send_are_deterministic() {
    let state = demo_state();
    assert_eq!(state.snapshot().connection, ConnectionStatus::Connected);
    assert!(state.snapshot().messages.is_empty());

    let (state, effects) = reduce(
        state,
        DesktopEvent::Intent(DesktopIntent::SendMessage {
            client_message_id: "message_demo_0001".into(),
            text: "explain the harness API handshake".into(),
        }),
    );

    assert_eq!(state.revision(), 1);
    assert_eq!(state.snapshot().messages[0].role, MessageRole::User);
    assert_eq!(
        state.snapshot().messages[0].delivery,
        DeliveryState::Sending
    );
    assert_eq!(
        effects,
        vec![DesktopEffect::SendMessage {
            client_message_id: "message_demo_0001".into(),
            text: "explain the harness API handshake".into(),
        }]
    );
}

#[test]
fn bootstrap_projection_uses_human_safe_labels() {
    let snapshot = DesktopState::bootstrap(ApplicationMetadata::demo()).snapshot_envelope();
    let encoded = serde_json::to_string(&snapshot).expect("serialize public snapshot");

    assert_eq!(
        snapshot.payload.application.account_label.as_deref(),
        Some("demo@jcode.dev")
    );
    assert_eq!(
        snapshot
            .payload
            .session
            .as_ref()
            .expect("demo session")
            .display_title,
        "Demo chat"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).expect("inspect public snapshot")["payload"]
            ["session"]["model"],
        serde_json::json!({ "label": "Sonnet" })
    );
    assert!(!encoded.contains("provider"));
    assert!(!encoded.contains("anthropic"));
    assert!(!encoded.contains("session_demo_0000"));
}

#[test]
fn history_projection_advances_epoch_and_discards_late_patches() {
    let (state, _) = reduce(
        demo_state(),
        DesktopEvent::HistoryLoaded {
            messages: vec![
                MessageView::user(
                    "message_demo_0001",
                    "explain the harness API handshake",
                    DeliveryState::Delivered,
                ),
                MessageView::assistant(
                    "assistant_demo_0001",
                    "The client opens the socket and sends a `hello` frame carrying its supported version range.",
                    DeliveryState::Delivered,
                ),
            ],
        },
    );
    assert_eq!(state.revision(), 1);
    assert_eq!(state.snapshot().messages.len(), 2);
    assert!(state.snapshot().messages[1].document.is_some());

    let (state, effects) = reduce(
        state,
        DesktopEvent::SessionAttached {
            session: Some(SessionMetadata::demo()),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(state.stream_epoch(), 2);
    assert_eq!(state.revision(), 0);
    assert!(state.snapshot().messages.is_empty());

    let (state, effects) = reduce(
        state,
        DesktopEvent::IncomingPatch {
            envelope: DesktopEnvelope::patch(
                1,
                2,
                DesktopPatch::Connection {
                    connection: ConnectionStatus::Offline,
                },
            ),
        },
    );
    assert!(effects.is_empty());
    assert_eq!(state.revision(), 0);
    assert_eq!(state.snapshot().connection, ConnectionStatus::Connected);
}
