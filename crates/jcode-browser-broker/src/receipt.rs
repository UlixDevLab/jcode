//! Redacted action receipts.
//!
//! `ActionReceipt` is audit output only. It is **never** accepted as
//! input and never authorizes a later action. The serialized form
//! contains only origin, action class, action id, navigation epoch, and
//! an optional [`BoundaryTrip`] reason; URL paths, queries, fragments,
//! headers, request/response bodies, credentials, and capability
//! material are stripped.

use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;

use crate::origin::LocalOrigin;
use crate::refs::NavigationEpoch;

/// The set of actions supported by the `local_browser` tool.
///
/// Each variant carries exactly the metadata needed to describe the
/// action class (target ref id, action kind, key / viewport dimensions,
/// etc.). Any payload that could leak user input, page content, headers,
/// or credentials is omitted from the serialized form via
/// `#[serde(skip)]` or a redacted projection.
#[derive(Clone, PartialEq, Eq)]
pub enum LocalBrowserAction {
    /// `status`: probe a live broker.
    Status,
    /// `open`: spawn a broker for the given (already validated) origin.
    Open {
        /// Pinned origin (redaction-safe).
        origin: LocalOrigin,
    },
    /// `close`: shut down a broker.
    Close,
    /// `snapshot`: take a structural snapshot of the current page.
    Snapshot,
    /// `find`: locate an element by selector.
    Find {
        /// Opaque ref id assigned to the located element.
        target: String,
    },
    /// `click`: click an element.
    Click {
        /// Opaque ref id of the element to click.
        target: String,
    },
    /// `type`: type into an element.
    Type {
        /// Opaque ref id of the target element.
        target: String,
        /// Sensitive: not serialized.
        text: String,
    },
    /// `fill_form`: fill multiple typed fields.
    FillForm {
        /// Number of fields filled (counts only, never values).
        field_count: usize,
    },
    /// `select`: pick an `<option>` from a `<select>`.
    Select {
        /// Opaque ref id of the `<select>`.
        target: String,
    },
    /// `press`: keyboard key event.
    Press {
        /// Key chord (e.g. `Enter`, `Tab`).
        key: String,
    },
    /// `wait`: wait for a condition.
    Wait,
    /// `screenshot`: capture a viewport image.
    Screenshot {
        /// Image width in pixels (never the image contents).
        width: u32,
        /// Image height in pixels (never the image contents).
        height: u32,
    },
    /// `resize`: resize the viewport.
    Resize {
        /// New viewport width.
        width: u32,
        /// New viewport height.
        height: u32,
    },
    /// `navigate` (M3): broker-internal navigation event.
    Navigate {
        /// Sensitive: not serialized.
        from: String,
        /// Sensitive: not serialized.
        to: String,
    },
}

impl std::fmt::Debug for LocalBrowserAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBrowserAction")
            .field("class", &self.class())
            .finish()
    }
}

impl LocalBrowserAction {
    /// Short class name used in receipts and logs (e.g. `click`,
    /// `fill_form`).
    pub fn class(&self) -> &'static str {
        match self {
            LocalBrowserAction::Status => "status",
            LocalBrowserAction::Open { .. } => "open",
            LocalBrowserAction::Close => "close",
            LocalBrowserAction::Snapshot => "snapshot",
            LocalBrowserAction::Find { .. } => "find",
            LocalBrowserAction::Click { .. } => "click",
            LocalBrowserAction::Type { .. } => "type",
            LocalBrowserAction::FillForm { .. } => "fill_form",
            LocalBrowserAction::Select { .. } => "select",
            LocalBrowserAction::Press { .. } => "press",
            LocalBrowserAction::Wait => "wait",
            LocalBrowserAction::Screenshot { .. } => "screenshot",
            LocalBrowserAction::Resize { .. } => "resize",
            LocalBrowserAction::Navigate { .. } => "navigate",
        }
    }

    /// Redaction-safe target ref id (when the action addresses one).
    pub fn target(&self) -> Option<&str> {
        match self {
            LocalBrowserAction::Find { target }
            | LocalBrowserAction::Click { target }
            | LocalBrowserAction::Type { target, .. }
            | LocalBrowserAction::Select { target } => Some(target.as_str()),
            _ => None,
        }
    }
}

/// Boundary-trip reason codes.
///
/// Recorded in [`ActionReceipt::boundary_trip`] when the broker
/// rejected a network / navigation attempt that would have crossed the
/// pinned origin. The list is intentionally narrow: every variant is a
/// redacted reason code, never a URL or hostname.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// A redirect targeted a non-pinned origin.
    RedirectToPublicOrigin,
    /// A subresource (image / script / stylesheet / font / fetch /
    /// XHR / EventSource / WebSocket) targeted a non-pinned origin.
    CrossOriginSubresource,
    /// A cross-origin popup or iframe was opened.
    CrossOriginFrame,
    /// A WebSocket handshake targeted a non-pinned origin.
    CrossOriginWebSocket,
    /// The dev server that originally pinned the broker has stopped
    /// answering or has been replaced by another listener.
    DevServerGone,
    /// A service worker registration was attempted and blocked.
    ServiceWorkerBlocked,
    /// The URL scheme is not supported (e.g. `file:`, `data:`).
    UnsupportedScheme,
    /// The capability presented on the IPC socket was missing or
    /// wrong.
    CapabilityRejected,
}

/// A single boundary-trip event recorded in an [`ActionReceipt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BoundaryTrip {
    /// Redacted reason code.
    pub reason: Reason,
}

/// A redacted audit record for one local-browser action.
///
/// `ActionReceipt` implements `Serialize` so audit pipelines can
/// persist it. The serialization contract deliberately omits every
/// field that could leak user input, page content, headers, bodies,
/// credentials, or capability material.
///
/// It deliberately does not implement `Deserialize`, because a receipt is
/// output-only evidence and must never become an authorization input.
///
/// ```compile_fail
/// use jcode_browser_broker::receipt::ActionReceipt;
/// fn require_deserialize<T: serde::de::DeserializeOwned>() {}
/// require_deserialize::<ActionReceipt>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionReceipt {
    /// Unique id for this receipt.
    pub action_id: Uuid,
    /// Short action class name (e.g. `click`, `fill_form`).
    pub action: String,
    /// Target ref id, when the action addressed one (redacted to `ref`).
    pub target: Option<String>,
    /// Pinned loopback origin for this broker.
    pub origin: LocalOrigin,
    /// Navigation epoch under which the action executed.
    pub navigation_epoch: NavigationEpoch,
    /// Optional boundary-trip event observed during the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_trip: Option<BoundaryTrip>,
}

impl ActionReceipt {
    /// Build a receipt from raw action data, applying redaction.
    pub fn new(
        action_id: Uuid,
        action: LocalBrowserAction,
        origin: LocalOrigin,
        navigation_epoch: NavigationEpoch,
        boundary_trip: Option<BoundaryTrip>,
    ) -> Self {
        let target = action.target().map(|_| "ref".to_string());
        Self {
            action_id,
            action: action.class().to_string(),
            target,
            origin,
            navigation_epoch,
            boundary_trip,
        }
    }

    /// Redact a URL path component down to a shape-safe placeholder.
    ///
    /// The redactor accepts any input that starts with `/` and returns
    /// the constant `/*` so no caller can leak the original path,
    /// query, or fragment through this projection. Empty input or
    /// input that does not start with `/` is rejected — those cannot
    /// have been produced by a well-formed URL path.
    pub fn redact_url_path(raw: &str) -> Result<&'static str, RedactionError> {
        if raw.is_empty() || !raw.starts_with('/') {
            return Err(RedactionError::InvalidShape);
        }
        Ok("/*")
    }
}

/// Errors returned by [`ActionReceipt::redact_url_path`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RedactionError {
    /// The input was empty or did not start with `/`.
    #[error("path did not match the redactor contract (must be non-empty and start with `/`)")]
    InvalidShape,
}
