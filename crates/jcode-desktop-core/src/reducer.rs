use crate::{
    ConnectionStatus, DeliveryState, DesktopEffect, DesktopEnvelope, DesktopIntent, DesktopPatch,
    DesktopState, MessageRole, MessageView, SessionMetadata,
};
use serde::{Deserialize, Serialize};

/// Events accepted by the pure desktop reducer. Runtime SDK mapping lives outside this crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DesktopEvent {
    Intent(DesktopIntent),
    SessionAttached {
        session: Option<SessionMetadata>,
    },
    HistoryLoaded {
        messages: Vec<MessageView>,
    },
    MessageAccepted,
    AssistantDelta {
        message_id: String,
        text: String,
    },
    TurnFinished,
    ConnectionChanged {
        connection: ConnectionStatus,
    },
    IncomingPatch {
        envelope: DesktopEnvelope<DesktopPatch>,
    },
}

/// Apply an event without performing I/O.
pub fn reduce(mut state: DesktopState, event: DesktopEvent) -> (DesktopState, Vec<DesktopEffect>) {
    let mut effects = Vec::new();
    match event {
        DesktopEvent::Intent(DesktopIntent::SendMessage {
            client_message_id,
            text,
        }) if !text.trim().is_empty() => {
            state.snapshot_mut().messages.push(MessageView::user(
                client_message_id.clone(),
                text.clone(),
                DeliveryState::Sending,
            ));
            state.advance_revision();
            effects.push(DesktopEffect::SendMessage {
                client_message_id,
                text,
            });
        }
        DesktopEvent::SessionAttached { session } => state.begin_epoch(session),
        DesktopEvent::HistoryLoaded { messages } => {
            state.snapshot_mut().messages = messages;
            state.advance_revision();
        }
        DesktopEvent::MessageAccepted => {
            if let Some(message) = state.snapshot_mut().messages.iter_mut().find(|message| {
                message.role == MessageRole::User && message.delivery == DeliveryState::Sending
            }) {
                message.delivery = DeliveryState::Delivered;
                state.advance_revision();
            }
        }
        DesktopEvent::AssistantDelta { message_id, text } => {
            let messages = &mut state.snapshot_mut().messages;
            if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
                message.markdown.push_str(&text);
                message.document = Some(jcode_render_core::parse_markdown(&message.markdown));
                message.delivery = DeliveryState::Streaming;
            } else {
                messages.push(MessageView::assistant(
                    message_id,
                    text,
                    DeliveryState::Streaming,
                ));
            }
            state.advance_revision();
        }
        DesktopEvent::TurnFinished => {
            if let Some(message) = state
                .snapshot_mut()
                .messages
                .iter_mut()
                .rev()
                .find(|message| {
                    message.role == MessageRole::Assistant
                        && message.delivery == DeliveryState::Streaming
                })
            {
                message.delivery = DeliveryState::Delivered;
                state.advance_revision();
            }
        }
        DesktopEvent::ConnectionChanged { connection } => {
            if state.snapshot().connection != connection {
                state.snapshot_mut().connection = connection;
                state.advance_revision();
            }
        }
        DesktopEvent::IncomingPatch { envelope } => {
            if envelope.schema_version != crate::DESKTOP_SCHEMA_VERSION
                || envelope.stream_epoch != state.stream_epoch()
            {
                return (state, effects);
            }
            let expected = state.revision() + 1;
            if envelope.revision > expected {
                state.snapshot_mut().connection = ConnectionStatus::Resyncing;
                state.advance_revision();
                effects.push(DesktopEffect::RequestResync {
                    stream_epoch: state.stream_epoch(),
                    revision: expected - 1,
                });
            } else if envelope.revision == expected {
                apply_patch(&mut state, envelope.payload);
                state.advance_revision();
            }
        }
        DesktopEvent::Intent(_) => {}
    }
    (state, effects)
}

fn apply_patch(state: &mut DesktopState, patch: DesktopPatch) {
    match patch {
        DesktopPatch::Session { session } => state.snapshot_mut().session = session,
        DesktopPatch::History { messages } => state.snapshot_mut().messages = messages,
        DesktopPatch::Message { message } => {
            let messages = &mut state.snapshot_mut().messages;
            if let Some(existing) = messages
                .iter_mut()
                .find(|existing| existing.id == message.id)
            {
                *existing = message;
            } else {
                messages.push(message);
            }
        }
        DesktopPatch::Connection { connection } => state.snapshot_mut().connection = connection,
        DesktopPatch::ResyncRequired { .. } => {
            state.snapshot_mut().connection = ConnectionStatus::Resyncing
        }
    }
}
