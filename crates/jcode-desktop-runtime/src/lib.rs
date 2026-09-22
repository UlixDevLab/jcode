//! Framework-neutral interpreter for desktop-core effects and SDK events.
//!
//! A UI adapter owns waking and drawing. This crate neither selects an executor nor imports a
//! window framework, so Tauri and a future GPUI adapter can host it unchanged.

use jcode_desktop_core::{
    ConnectionStatus, DesktopEffect, DesktopEnvelope, DesktopEvent, DesktopIntent, DesktopPatch,
    DesktopSnapshot, DesktopState, MessageView, SessionMetadata, reduce,
};

/// SDK/runtime failures intentionally stay transport-neutral for the UI adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub message: String,
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// I/O port interpreted by the runtime. An adapter can supply a synchronous worker or its own executor.
pub trait EffectPort {
    fn send_message(&mut self, text: &str, client_message_id: &str) -> Result<(), RuntimeError>;
    fn request_resync(&mut self, stream_epoch: u64, revision: u64) -> Result<(), RuntimeError>;
}

/// Result of one deterministic state transition and its emitted renderer envelopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOutput {
    pub envelopes: Vec<DesktopEnvelope<DesktopPatch>>,
    pub errors: Vec<RuntimeError>,
}

/// The portable runtime. It owns no thread, event loop, or window handle.
pub struct DesktopRuntime<P> {
    state: DesktopState,
    port: P,
}

impl<P: EffectPort> DesktopRuntime<P> {
    pub fn new(state: DesktopState, port: P) -> Self {
        Self { state, port }
    }

    pub fn bootstrap(&self) -> DesktopEnvelope<DesktopSnapshot> {
        self.state.snapshot_envelope()
    }

    pub fn dispatch(&mut self, intent: DesktopIntent) -> RuntimeOutput {
        self.apply_event(DesktopEvent::Intent(intent))
    }

    pub fn apply_event(&mut self, event: DesktopEvent) -> RuntimeOutput {
        let before = self.state.clone();
        let resync = match &event {
            DesktopEvent::IncomingPatch { envelope }
                if envelope.schema_version == jcode_desktop_core::DESKTOP_SCHEMA_VERSION
                    && envelope.stream_epoch == before.stream_epoch()
                    && envelope.revision > before.revision() + 1 =>
            {
                Some((before.revision() + 1, envelope.revision))
            }
            _ => None,
        };
        let (state, effects) = reduce(self.state.clone(), event);
        self.state = state;
        let mut errors = Vec::new();
        let mut envelopes = patch_between(&before, &self.state, resync)
            .into_iter()
            .collect::<Vec<_>>();
        for effect in effects {
            if let Err(error) = self.interpret(effect) {
                errors.push(error);
                let before_failure = self.state.clone();
                let (state, _) = reduce(
                    self.state.clone(),
                    DesktopEvent::ConnectionChanged {
                        connection: ConnectionStatus::Offline,
                    },
                );
                self.state = state;
                envelopes.extend(patch_between(&before_failure, &self.state, None));
            }
        }
        RuntimeOutput { envelopes, errors }
    }

    pub fn port(&self) -> &P {
        &self.port
    }

    pub fn state(&self) -> &DesktopState {
        &self.state
    }

    fn interpret(&mut self, effect: DesktopEffect) -> Result<(), RuntimeError> {
        match effect {
            DesktopEffect::SendMessage {
                client_message_id,
                text,
            } => self.port.send_message(&text, &client_message_id),
            DesktopEffect::RequestResync {
                stream_epoch,
                revision,
            } => self.port.request_resync(stream_epoch, revision),
        }
    }
}

/// Convert the subset of public SDK events required by Phase 0 into framework-neutral core events.
/// The runtime retains the raw session identifier only to filter daemon events before reduction.
pub fn map_sdk_event(
    attached_session_id: &str,
    event: &jcode_sdk::ApiEvent,
) -> Option<DesktopEvent> {
    match event {
        jcode_sdk::ApiEvent::Attached { session } => Some(DesktopEvent::SessionAttached {
            session: Some(SessionMetadata {
                display_title: session
                    .title
                    .as_deref()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or("Untitled chat")
                    .to_owned(),
                repository_root: session.working_dir.clone(),
                model: None,
            }),
        }),
        jcode_sdk::ApiEvent::History {
            session_id,
            messages,
        } if session_id == attached_session_id => Some(DesktopEvent::HistoryLoaded {
            messages: messages
                .iter()
                .enumerate()
                .map(|(index, message)| match message.role.as_str() {
                    "assistant" => MessageView::assistant(
                        format!("history_assistant_{index}"),
                        message.content.clone(),
                        jcode_desktop_core::DeliveryState::Delivered,
                    ),
                    "tool" => MessageView::notice(
                        format!("history_notice_{index}"),
                        message.content.clone(),
                    ),
                    _ => MessageView::user(
                        format!("history_user_{index}"),
                        message.content.clone(),
                        jcode_desktop_core::DeliveryState::Delivered,
                    ),
                })
                .collect(),
        }),
        jcode_sdk::ApiEvent::MessageAccepted { session_id }
            if session_id == attached_session_id =>
        {
            Some(DesktopEvent::MessageAccepted)
        }
        jcode_sdk::ApiEvent::TextDelta { session_id, text }
            if session_id == attached_session_id =>
        {
            Some(DesktopEvent::AssistantDelta {
                message_id: "assistant_live".into(),
                text: text.clone(),
            })
        }
        jcode_sdk::ApiEvent::TurnDone { session_id } if session_id == attached_session_id => {
            Some(DesktopEvent::TurnFinished)
        }
        jcode_sdk::ApiEvent::Error { message, .. } => Some(DesktopEvent::ConnectionChanged {
            connection: if message.is_empty() {
                ConnectionStatus::Connected
            } else {
                ConnectionStatus::Offline
            },
        }),
        _ => None,
    }
}

fn patch_between(
    before: &DesktopState,
    after: &DesktopState,
    resync: Option<(u64, u64)>,
) -> Option<DesktopEnvelope<DesktopPatch>> {
    if before.stream_epoch() != after.stream_epoch() {
        return Some(DesktopEnvelope::patch(
            after.stream_epoch(),
            after.revision(),
            DesktopPatch::Session {
                session: after.snapshot().session.clone(),
            },
        ));
    }
    if before.revision() == after.revision() {
        return None;
    }
    let patch = if let Some((expected_revision, observed_revision)) = resync {
        DesktopPatch::ResyncRequired {
            expected_revision,
            observed_revision,
        }
    } else if before.snapshot().connection != after.snapshot().connection {
        DesktopPatch::Connection {
            connection: after.snapshot().connection,
        }
    } else if before.snapshot().messages != after.snapshot().messages {
        after
            .snapshot()
            .messages
            .iter()
            .find(|message| {
                before
                    .snapshot()
                    .messages
                    .iter()
                    .find(|old| old.id == message.id)
                    != Some(*message)
            })
            .cloned()
            .map(|message| DesktopPatch::Message { message })
            .unwrap_or_else(|| DesktopPatch::History {
                messages: after.snapshot().messages.clone(),
            })
    } else {
        DesktopPatch::Session {
            session: after.snapshot().session.clone(),
        }
    };
    Some(DesktopEnvelope::patch(
        after.stream_epoch(),
        after.revision(),
        patch,
    ))
}
