//! Framework-neutral state, view, intent, event, and effect contracts for Jcode desktop clients.
//!
//! This crate deliberately contains no daemon client, window, or renderer adapter. A Tauri,
//! GPUI, or headless client can all reduce the same [`DesktopEvent`] values and consume the
//! same versioned [`DesktopEnvelope`] stream.

mod model;
mod reducer;

pub use model::{
    ApplicationMetadata, ConnectionStatus, DESKTOP_SCHEMA_VERSION, DeliveryState, DesktopEffect,
    DesktopEnvelope, DesktopIntent, DesktopPatch, DesktopSnapshot, DesktopState, MessageRole,
    MessageView, ModelView, SessionMetadata,
};
pub use reducer::{DesktopEvent, reduce};
