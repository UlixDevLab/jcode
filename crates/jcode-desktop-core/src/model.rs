use jcode_render_core::{Document, parse_markdown};
use serde::{Deserialize, Serialize};

/// The wire-contract version for every desktop snapshot and patch envelope.
pub const DESKTOP_SCHEMA_VERSION: u16 = 1;

/// Metadata that is safe to render independently of a daemon connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationMetadata {
    pub version: String,
    pub account_label: Option<String>,
}

impl ApplicationMetadata {
    /// Pinned metadata ported from desktop2's deterministic capture nodes.
    pub fn demo() -> Self {
        Self {
            version: "v0.0.0-demo (0000000)".into(),
            account_label: Some("demo@jcode.dev".into()),
        }
    }
}

/// Renderer-safe session metadata. Runtime-only session IDs never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub display_title: String,
    pub repository_root: Option<String>,
    pub model: Option<ModelView>,
}

impl SessionMetadata {
    /// Fixture data ported from desktop2's `attached_empty` state node.
    pub fn demo() -> Self {
        Self {
            display_title: "Demo chat".into(),
            repository_root: Some("/home/j/jcode".into()),
            model: Some(ModelView {
                label: "Sonnet".into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelView {
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connected,
    Connecting,
    Offline,
    Resyncing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Delivered,
    Sending,
    Queued,
    Streaming,
    Failed,
}

/// A renderer-facing message projection. Markdown is parsed once through the shared semantic model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageView {
    pub id: String,
    pub role: MessageRole,
    pub markdown: String,
    pub delivery: DeliveryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<Document>,
}

impl MessageView {
    pub fn user(
        id: impl Into<String>,
        markdown: impl Into<String>,
        delivery: DeliveryState,
    ) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::User,
            markdown: markdown.into(),
            delivery,
            document: None,
        }
    }

    pub fn assistant(
        id: impl Into<String>,
        markdown: impl Into<String>,
        delivery: DeliveryState,
    ) -> Self {
        let markdown = markdown.into();
        Self {
            id: id.into(),
            role: MessageRole::Assistant,
            document: Some(parse_markdown(&markdown)),
            markdown,
            delivery,
        }
    }

    pub fn notice(id: impl Into<String>, markdown: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: MessageRole::Notice,
            markdown: markdown.into(),
            delivery: DeliveryState::Delivered,
            document: None,
        }
    }
}

/// Full state used to bootstrap a renderer. It has no raw session or socket identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopSnapshot {
    pub application: ApplicationMetadata,
    pub session: Option<SessionMetadata>,
    pub connection: ConnectionStatus,
    pub messages: Vec<MessageView>,
}

/// A typed incremental renderer update. Adapters never infer changes from raw daemon events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesktopPatch {
    Session {
        session: Option<SessionMetadata>,
    },
    History {
        messages: Vec<MessageView>,
    },
    Message {
        message: MessageView,
    },
    Connection {
        connection: ConnectionStatus,
    },
    ResyncRequired {
        expected_revision: u64,
        observed_revision: u64,
    },
}

/// Versioned stream payload. Snapshots use it at bootstrap; subsequent messages carry patches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopEnvelope<T> {
    pub schema_version: u16,
    pub stream_epoch: u64,
    pub revision: u64,
    pub payload: T,
}

impl<T> DesktopEnvelope<T> {
    pub fn new(stream_epoch: u64, revision: u64, payload: T) -> Self {
        Self {
            schema_version: DESKTOP_SCHEMA_VERSION,
            stream_epoch,
            revision,
            payload,
        }
    }
}

impl DesktopEnvelope<DesktopPatch> {
    pub fn patch(stream_epoch: u64, revision: u64, payload: DesktopPatch) -> Self {
        Self::new(stream_epoch, revision, payload)
    }
}

/// Semantic renderer input. Composer keystrokes remain adapter-local until `SendMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum DesktopIntent {
    SendMessage {
        client_message_id: String,
        text: String,
    },
}

/// Work requested by the pure reducer, interpreted by the SDK runtime or a future platform port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum DesktopEffect {
    SendMessage {
        client_message_id: String,
        text: String,
    },
    RequestResync {
        stream_epoch: u64,
        revision: u64,
    },
}

/// Product state plus transport cursor. The cursor is intentionally not part of `DesktopSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopState {
    stream_epoch: u64,
    revision: u64,
    snapshot: DesktopSnapshot,
}

impl DesktopState {
    pub fn bootstrap(application: ApplicationMetadata) -> Self {
        Self {
            stream_epoch: 1,
            revision: 0,
            snapshot: DesktopSnapshot {
                application,
                session: Some(SessionMetadata::demo()),
                connection: ConnectionStatus::Connected,
                messages: Vec::new(),
            },
        }
    }

    pub fn from_snapshot(stream_epoch: u64, revision: u64, snapshot: DesktopSnapshot) -> Self {
        Self {
            stream_epoch,
            revision,
            snapshot,
        }
    }

    pub fn stream_epoch(&self) -> u64 {
        self.stream_epoch
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn snapshot(&self) -> &DesktopSnapshot {
        &self.snapshot
    }

    pub fn snapshot_envelope(&self) -> DesktopEnvelope<DesktopSnapshot> {
        DesktopEnvelope::new(self.stream_epoch, self.revision, self.snapshot.clone())
    }

    pub(crate) fn advance_revision(&mut self) {
        self.revision += 1;
    }

    pub(crate) fn begin_epoch(&mut self, session: Option<SessionMetadata>) {
        self.stream_epoch += 1;
        self.revision = 0;
        self.snapshot.session = session;
        self.snapshot.messages.clear();
        self.snapshot.connection = ConnectionStatus::Connected;
    }

    pub(crate) fn snapshot_mut(&mut self) -> &mut DesktopSnapshot {
        &mut self.snapshot
    }
}
