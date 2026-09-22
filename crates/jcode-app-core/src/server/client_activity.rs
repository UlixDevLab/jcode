use super::ClientConnectionInfo;
use crate::protocol::{SessionActivityContextSnapshot, SessionActivitySnapshot};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(super) fn session_activity_context_snapshot(
    working_dir: Option<&str>,
    started_at: chrono::DateTime<chrono::Utc>,
    activity: Option<&SessionActivitySnapshot>,
) -> Option<SessionActivityContextSnapshot> {
    let repository = safe_workspace_basis(working_dir)?;
    let activity_label = match activity.and_then(|snapshot| snapshot.current_tool_name.as_deref()) {
        Some("bash") => "Running command",
        Some("read") => "Reading files",
        Some("write" | "edit" | "multiedit" | "apply_patch") => "Editing files",
        Some("browser") => "Browsing",
        Some("imagegen") => "Generating image",
        Some(_) => "Working",
        None if activity.is_some_and(|snapshot| snapshot.is_processing) => "Thinking",
        None => "Waiting",
    };

    Some(SessionActivityContextSnapshot {
        repository,
        started_at: started_at.to_rfc3339(),
        activity_label: activity_label.to_string(),
    })
}

fn safe_workspace_basis(working_dir: Option<&str>) -> Option<String> {
    let basis = std::path::Path::new(working_dir?)
        .file_name()?
        .to_str()?
        .trim();
    (1..=64)
        .contains(&basis.len())
        .then_some(basis)
        .filter(|basis| {
            basis
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .map(str::to_string)
}

pub(super) async fn session_activity_snapshot(
    client_connections: &Arc<RwLock<HashMap<String, ClientConnectionInfo>>>,
    session_id: &str,
    fallback_processing: bool,
) -> Option<SessionActivitySnapshot> {
    let snapshot = {
        let connections = client_connections.read().await;
        let mut processing_without_tool = false;
        let mut tool_name = None;
        for info in connections.values() {
            if info.session_id != session_id || !info.is_processing {
                continue;
            }
            if let Some(current_tool_name) = info.current_tool_name.clone() {
                tool_name = Some(current_tool_name);
                break;
            }
            processing_without_tool = true;
        }

        tool_name
            .map(|current_tool_name| SessionActivitySnapshot {
                is_processing: true,
                current_tool_name: Some(current_tool_name),
            })
            .or_else(|| {
                processing_without_tool.then_some(SessionActivitySnapshot {
                    is_processing: true,
                    current_tool_name: None,
                })
            })
    };

    snapshot.or_else(|| {
        fallback_processing.then_some(SessionActivitySnapshot {
            is_processing: true,
            current_tool_name: None,
        })
    })
}
