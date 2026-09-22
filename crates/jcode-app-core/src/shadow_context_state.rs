//! Local-only context-selection observations. This module never changes a provider request.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    message::{ContentBlock, Role},
    session::Session,
};

const FLAG: &str = "JCODE_SHADOW_CONTEXT_STATE";
const MAX_SIGNALS: usize = 12;
const MAX_ARTIFACT_REFS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmissionOutcome {
    Disabled,
    Unchanged,
    Written,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineContext {
    pub message_count: usize,
    pub char_estimate: usize,
}

impl BaselineContext {
    fn from_session(session: &Session) -> Self {
        Self {
            message_count: session.messages.len(),
            char_estimate: serde_json::to_vec(&session.messages).map_or(0, |json| json.len()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowContextState {
    pub schema_version: u8,
    pub session_id: String,
    pub task: TaskEnvelope,
    pub decisions: Vec<SignalPointer>,
    pub failed_approaches: Vec<SignalPointer>,
    pub artifact_references: Vec<ArtifactReference>,
    pub context_manifest: ContextManifest,
    pub baseline_context: BaselineContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEnvelope {
    pub current_task: SignalPointer,
    pub user_intent: SignalPointer,
    pub active_step_cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalPointer {
    pub category: String,
    pub message_index: usize,
    pub message_id: String,
    pub char_estimate: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReference {
    pub path_hash: String,
    pub file_name: Option<String>,
    pub source_message_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextManifest {
    pub shadow_char_estimate: usize,
    pub categories: Vec<ManifestCategory>,
    pub retention: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCategory {
    pub category: String,
    pub included: bool,
    pub char_estimate: usize,
}

pub fn enabled() -> bool {
    match std::env::var(FLAG) {
        Ok(value) => {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

/// Emit one bounded snapshot per session when the experiment is explicitly enabled.
pub fn maybe_emit_for_completed_turn(session: &Session) -> Result<EmissionOutcome> {
    if !enabled() {
        return Ok(EmissionOutcome::Disabled);
    }

    let state = build_state(session);
    let path = artifact_path(&session.id)?;
    crate::storage::ensure_dir(path.parent().expect("shadow artifact parent"))?;
    if crate::storage::read_json::<ShadowContextState>(&path)
        .is_ok_and(|existing| existing == state)
    {
        return Ok(EmissionOutcome::Unchanged);
    }
    crate::storage::write_json_secret(&path, &state)?;
    Ok(EmissionOutcome::Written)
}

pub fn build_state(session: &Session) -> ShadowContextState {
    let latest_user = latest_signal(session, "user_intent", "user_intent");
    let task = latest_signal(session, "current_task", "current_task");
    let decisions = collect_signals(session, &["decision:", "decided:", "decision made"]);
    let failed_approaches = collect_failure_signals(session);
    let artifact_references = collect_artifact_references(session);
    let excluded_prompt_chars = text_chars(session);
    let excluded_tool_chars = tool_payload_chars(session);
    let included_chars = serde_json::to_vec(&(
        &task,
        &latest_user,
        &decisions,
        &failed_approaches,
        &artifact_references,
    ))
    .map_or(0, |json| json.len());
    ShadowContextState {
        schema_version: 1,
        session_id: session.id.clone(),
        task: TaskEnvelope {
            current_task: task,
            user_intent: latest_user,
            active_step_cursor: session.messages.len(),
        },
        decisions,
        failed_approaches,
        artifact_references,
        context_manifest: ContextManifest {
            shadow_char_estimate: included_chars,
            categories: vec![
                category("task_and_intent_pointers", true, included_chars),
                category("prompt_bodies", false, excluded_prompt_chars),
                category("tool_inputs_and_outputs", false, excluded_tool_chars),
                category("reasoning_and_child_transcripts", false, 0),
            ],
            retention: "one bounded snapshot per session".to_string(),
        },
        baseline_context: BaselineContext::from_session(session),
    }
}

pub fn artifact_path(session_id: &str) -> Result<std::path::PathBuf> {
    Ok(crate::storage::jcode_dir()?
        .join("shadow-context-state")
        .join(format!("{}.json", sanitize_session_id(session_id))))
}

fn latest_signal(session: &Session, category: &str, fallback: &str) -> SignalPointer {
    session
        .messages
        .iter()
        .enumerate()
        .rev()
        .find(|(_, message)| message.role == Role::User && message.display_role.is_none())
        .map(|(index, message)| pointer(category, index, message))
        .unwrap_or_else(|| SignalPointer {
            category: fallback.to_string(),
            message_index: 0,
            message_id: String::new(),
            char_estimate: 0,
        })
}

fn collect_signals(session: &Session, markers: &[&str]) -> Vec<SignalPointer> {
    session
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::Assistant)
        .filter(|(_, message)| {
            message.content.iter().any(|block| {
                matches!(block, ContentBlock::Text { text, .. }
                if markers.iter().any(|marker| text.to_ascii_lowercase().contains(marker)))
            })
        })
        .take(MAX_SIGNALS)
        .map(|(index, message)| pointer("decision", index, message))
        .collect()
}

fn collect_failure_signals(session: &Session) -> Vec<SignalPointer> {
    session
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.content.iter().any(|block| match block {
                ContentBlock::ToolResult { is_error, .. } => is_error.unwrap_or(false),
                ContentBlock::Text { text, .. } => {
                    let lower = text.to_ascii_lowercase();
                    lower.contains("failed:") || lower.contains("failure:")
                }
                _ => false,
            })
        })
        .take(MAX_SIGNALS)
        .map(|(index, message)| pointer("failed_approach", index, message))
        .collect()
}

fn collect_artifact_references(session: &Session) -> Vec<ArtifactReference> {
    let mut references = Vec::new();
    for (index, message) in session.messages.iter().enumerate() {
        for block in &message.content {
            if let ContentBlock::ToolUse { input, .. } = block {
                collect_paths(input, index, &mut references);
            }
        }
    }
    references.sort_by(|left, right| left.path_hash.cmp(&right.path_hash));
    references.dedup_by(|left, right| left.path_hash == right.path_hash);
    references.truncate(MAX_ARTIFACT_REFS);
    references
}

fn collect_paths(value: &serde_json::Value, index: usize, output: &mut Vec<ArtifactReference>) {
    match value {
        serde_json::Value::String(value) if looks_like_safe_path(value) => {
            let file_name = std::path::Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string);
            output.push(ArtifactReference {
                path_hash: stable_hash(value),
                file_name,
                source_message_index: index,
            });
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_paths(value, index, output);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_paths(value, index, output);
            }
        }
        _ => {}
    }
}

fn looks_like_safe_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (value.starts_with('/') || value.starts_with("./") || value.contains('/'))
        && value.len() <= 240
        && !lower.contains(".env")
        && !["secret", "token", "password", "credential", "private_key"]
            .iter()
            .any(|marker| lower.contains(marker))
}

fn stable_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn pointer(category: &str, index: usize, message: &crate::session::StoredMessage) -> SignalPointer {
    SignalPointer {
        category: category.to_string(),
        message_index: index,
        message_id: message.id.clone(),
        char_estimate: message_chars(message),
    }
}

fn message_chars(message: &crate::session::StoredMessage) -> usize {
    serde_json::to_vec(&message.content).map_or(0, |json| json.len())
}

fn text_chars(session: &Session) -> usize {
    session
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text, .. } => Some(text.len()),
            _ => None,
        })
        .sum()
}

fn tool_payload_chars(session: &Session) -> usize {
    session
        .messages
        .iter()
        .flat_map(|message| &message.content)
        .map(|block| match block {
            ContentBlock::ToolUse { input, .. } => input.to_string().len(),
            ContentBlock::ToolResult { content, .. } => content.len(),
            _ => 0,
        })
        .sum()
}

fn category(category: &str, included: bool, char_estimate: usize) -> ManifestCategory {
    ManifestCategory {
        category: category.to_string(),
        included,
        char_estimate,
    }
}

fn sanitize_session_id(session_id: &str) -> String {
    session_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
