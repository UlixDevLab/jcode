use jcode_desktop_core::{
    ApplicationMetadata, ConnectionStatus, DeliveryState, DesktopEnvelope, DesktopEvent,
    DesktopIntent, DesktopPatch, DesktopState, MessageView,
};
use jcode_desktop_runtime::{DesktopRuntime, EffectPort, RuntimeError, map_sdk_event};

#[derive(Default)]
struct RecordingPort {
    sent: Vec<String>,
    resyncs: Vec<(u64, u64)>,
}

struct FailingPort;

struct ResyncFailingPort;

impl EffectPort for RecordingPort {
    fn send_message(&mut self, text: &str, _client_message_id: &str) -> Result<(), RuntimeError> {
        self.sent.push(text.into());
        Ok(())
    }

    fn request_resync(&mut self, stream_epoch: u64, revision: u64) -> Result<(), RuntimeError> {
        self.resyncs.push((stream_epoch, revision));
        Ok(())
    }
}

impl EffectPort for FailingPort {
    fn send_message(&mut self, _: &str, _: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("daemon connection closed"))
    }

    fn request_resync(&mut self, _: u64, _: u64) -> Result<(), RuntimeError> {
        Ok(())
    }
}

impl EffectPort for ResyncFailingPort {
    fn send_message(&mut self, _: &str, _: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn request_resync(&mut self, _: u64, _: u64) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("resync transport closed"))
    }
}

#[test]
fn runtime_emits_ordered_envelopes_and_requests_resync_for_a_gap() {
    let state = DesktopState::bootstrap(ApplicationMetadata::demo());
    let mut runtime = DesktopRuntime::new(state, RecordingPort::default());
    let bootstrap = runtime.bootstrap();
    assert_eq!(bootstrap.revision, 0);

    let sent = runtime.dispatch(DesktopIntent::SendMessage {
        client_message_id: "message_demo_0001".into(),
        text: "explain the harness API handshake".into(),
    });
    assert_eq!(sent.envelopes[0].revision, 1);

    let gap = runtime.apply_event(DesktopEvent::IncomingPatch {
        envelope: DesktopEnvelope::patch(
            1,
            3,
            DesktopPatch::Connection {
                connection: jcode_desktop_core::ConnectionStatus::Connected,
            },
        ),
    });
    assert_eq!(gap.envelopes[0].revision, 2);
    assert_eq!(
        gap.envelopes[0].payload,
        DesktopPatch::ResyncRequired {
            expected_revision: 2,
            observed_revision: 3,
        }
    );
    assert_eq!(runtime.port().resyncs, vec![(1, 1)]);
}

#[test]
fn sdk_events_are_filtered_by_session_before_reduction() {
    let event = jcode_sdk::ApiEvent::TextDelta {
        session_id: "session_demo_0000".into(),
        text: "The client opens the socket".into(),
    };
    assert_eq!(
        map_sdk_event("session_demo_0000", &event),
        Some(DesktopEvent::AssistantDelta {
            message_id: "assistant_live".into(),
            text: "The client opens the socket".into(),
        })
    );
    assert_eq!(map_sdk_event("another_session", &event), None);
}

#[test]
fn attached_events_never_project_raw_session_ids_for_missing_or_blank_titles() {
    for title in [None, Some("   ")] {
        let session_id = "daemon-session-9c3ff11a";
        let event = jcode_sdk::ApiEvent::Attached {
            session: jcode_sdk::SessionInfo {
                session_id: session_id.into(),
                working_dir: Some("/safe/project".into()),
                title: title.map(str::to_owned),
                status: "idle".into(),
                transcript_bytes: None,
                archived: false,
                archived_at_ms: None,
            },
        };

        let Some(DesktopEvent::SessionAttached {
            session: Some(session),
        }) = map_sdk_event(session_id, &event)
        else {
            panic!("attached event must map to a safe session projection");
        };

        assert_eq!(session.display_title, "Untitled chat");
        assert_ne!(session.display_title, session_id);
        assert!(session.model.is_none());
    }
}

#[test]
fn failed_effects_are_reported_and_mark_the_connection_offline() {
    let state = DesktopState::bootstrap(ApplicationMetadata::demo());
    let mut runtime = DesktopRuntime::new(state, FailingPort);
    let output = runtime.dispatch(DesktopIntent::SendMessage {
        client_message_id: "message_demo_0001".into(),
        text: "explain the harness API handshake".into(),
    });

    assert_eq!(
        output.errors,
        vec![RuntimeError::new("daemon connection closed")]
    );
    assert_eq!(
        output.envelopes,
        vec![
            DesktopEnvelope::patch(
                1,
                1,
                DesktopPatch::Message {
                    message: MessageView::user(
                        "message_demo_0001",
                        "explain the harness API handshake",
                        DeliveryState::Sending,
                    ),
                },
            ),
            DesktopEnvelope::patch(
                1,
                2,
                DesktopPatch::Connection {
                    connection: ConnectionStatus::Offline,
                },
            ),
        ]
    );
    assert_eq!(
        runtime.state().snapshot().connection,
        ConnectionStatus::Offline
    );
    assert_eq!(runtime.state().snapshot().messages.len(), 1);
    assert_eq!(
        runtime.state().snapshot().messages[0].delivery,
        DeliveryState::Sending
    );
}

#[test]
fn failed_resync_effect_emits_each_revision_before_marking_offline() {
    let state = DesktopState::bootstrap(ApplicationMetadata::demo());
    let mut runtime = DesktopRuntime::new(state, ResyncFailingPort);

    let output = runtime.apply_event(DesktopEvent::IncomingPatch {
        envelope: DesktopEnvelope::patch(
            1,
            2,
            DesktopPatch::Connection {
                connection: ConnectionStatus::Connected,
            },
        ),
    });

    assert_eq!(
        output.errors,
        vec![RuntimeError::new("resync transport closed")]
    );
    assert_eq!(
        output.envelopes,
        vec![
            DesktopEnvelope::patch(
                1,
                1,
                DesktopPatch::ResyncRequired {
                    expected_revision: 1,
                    observed_revision: 2,
                },
            ),
            DesktopEnvelope::patch(
                1,
                2,
                DesktopPatch::Connection {
                    connection: ConnectionStatus::Offline,
                },
            ),
        ]
    );
}
