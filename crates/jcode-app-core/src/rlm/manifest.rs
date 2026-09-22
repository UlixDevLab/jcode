use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::RlmMode,
    message::Message,
    rlm::{CounterfactualSelection, select_counterfactual},
    session::Session,
    shadow_context_state::{ShadowContextState, build_state},
};

const SCHEMA_VERSION: u8 = 1;
const MAX_MESSAGE_HASHES: usize = 1024;
const COUNTERFACTUAL_TOKEN_BUDGET: usize = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationOutcome {
    Disabled,
    Unchanged,
    Written(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallManifest {
    pub schema_version: u8,
    pub session_id: String,
    pub call_id: String,
    pub flow: String,
    pub requested_mode: String,
    pub effective_mode: String,
    pub provider_message_count: usize,
    pub provider_char_estimate: usize,
    pub provider_token_estimate: usize,
    pub provider_context_hash: String,
    pub ordered_message_hashes: Vec<String>,
    pub message_hashes_truncated: bool,
    #[serde(default)]
    pub counterfactual: CounterfactualSelection,
    pub typed_state: ShadowContextState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RlmStatusSnapshot {
    pub mode: String,
    pub provider_message_count: usize,
    pub baseline_message_count: usize,
    pub token_estimate: usize,
    pub selected: bool,
    pub waiting: bool,
}

impl RlmStatusSnapshot {
    pub fn waiting(mode: impl Into<String>) -> Self {
        Self {
            mode: mode.into(),
            provider_message_count: 0,
            baseline_message_count: 0,
            token_estimate: 0,
            selected: false,
            waiting: true,
        }
    }

    fn from_manifest(manifest: &CallManifest) -> Self {
        let selected_count = manifest.counterfactual.included.len();
        let baseline_message_count = selected_count + manifest.counterfactual.excluded.len();
        let selected = manifest.effective_mode == RlmMode::Pilot.as_str()
            && manifest.counterfactual.integrity_error.is_none()
            && manifest.provider_message_count == selected_count;
        Self {
            mode: manifest.effective_mode.clone(),
            provider_message_count: manifest.provider_message_count,
            baseline_message_count: baseline_message_count.max(manifest.provider_message_count),
            token_estimate: manifest.provider_token_estimate,
            selected,
            waiting: false,
        }
    }
}

struct CachedStatus {
    checked_at: Instant,
    snapshot: Option<RlmStatusSnapshot>,
}

const STATUS_CACHE_TTL: Duration = Duration::from_millis(750);

fn status_cache() -> &'static Mutex<HashMap<String, CachedStatus>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedStatus>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn latest_status_snapshot(session_id: &str) -> Option<RlmStatusSnapshot> {
    if validate_session_id(session_id).is_err() {
        return None;
    }
    let now = Instant::now();
    if let Ok(cache) = status_cache().lock()
        && let Some(cached) = cache.get(session_id)
        && now.duration_since(cached.checked_at) < STATUS_CACHE_TTL
    {
        return cached.snapshot.clone();
    }
    let snapshot = load_latest_manifest(session_id)
        .ok()
        .flatten()
        .map(|manifest| RlmStatusSnapshot::from_manifest(&manifest));
    if let Ok(mut cache) = status_cache().lock() {
        cache.insert(
            session_id.to_string(),
            CachedStatus {
                checked_at: now,
                snapshot: snapshot.clone(),
            },
        );
    }
    snapshot
}

fn load_latest_manifest(session_id: &str) -> Result<Option<CallManifest>> {
    let directory = manifest_directory(session_id)?;
    if !directory.is_dir() {
        return Ok(None);
    }
    let latest = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .filter_map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified);
    Ok(latest.and_then(|(_, path)| crate::storage::read_json::<CallManifest>(&path).ok()))
}

pub fn observe_provider_request(
    session: &Session,
    messages: &[Message],
    flow: &str,
    call_index: u32,
    requested_mode: RlmMode,
    effective_mode: RlmMode,
    selection: Option<&CounterfactualSelection>,
) -> Result<ObservationOutcome> {
    if effective_mode == RlmMode::Off {
        return Ok(ObservationOutcome::Disabled);
    }
    let config = &crate::config::config().rlm;
    observe_with_policy(
        session,
        messages,
        flow,
        call_index,
        requested_mode,
        effective_mode,
        config.max_provider_calls.max(1) as usize,
        selection,
    )
}

fn observe_with_policy(
    session: &Session,
    messages: &[Message],
    flow: &str,
    call_index: u32,
    requested_mode: RlmMode,
    effective_mode: RlmMode,
    max_provider_calls: usize,
    selection: Option<&CounterfactualSelection>,
) -> Result<ObservationOutcome> {
    let serialized = serde_json::to_vec(messages)?;
    let context_hash = hash_bytes(&serialized);
    let last_message_id = session
        .messages
        .last()
        .map(|message| message.id.as_str())
        .unwrap_or_default();
    let call_id = hash_bytes(
        format!(
            "{}\0{}\0{}\0{}\0{}",
            session.id, last_message_id, flow, call_index, context_hash
        )
        .as_bytes(),
    );
    let ordered_message_hashes: Vec<String> = messages
        .iter()
        .take(MAX_MESSAGE_HASHES)
        .map(|message| serde_json::to_vec(message).map(|bytes| hash_bytes(&bytes)))
        .collect::<Result<_, _>>()?;
    let manifest = CallManifest {
        schema_version: SCHEMA_VERSION,
        session_id: session.id.clone(),
        call_id: call_id.clone(),
        flow: flow.to_string(),
        requested_mode: requested_mode.as_str().to_string(),
        effective_mode: effective_mode.as_str().to_string(),
        provider_message_count: messages.len(),
        provider_char_estimate: serialized.len(),
        provider_token_estimate: serialized.len().div_ceil(4),
        provider_context_hash: context_hash,
        ordered_message_hashes,
        message_hashes_truncated: messages.len() > MAX_MESSAGE_HASHES,
        counterfactual: selection
            .cloned()
            .unwrap_or_else(|| select_counterfactual(messages, COUNTERFACTUAL_TOKEN_BUDGET)),
        typed_state: build_state(session),
    };
    let directory = manifest_directory(&session.id)?;
    crate::storage::ensure_dir(&directory)?;
    let path = directory.join(format!("{call_id}.json"));
    if crate::storage::read_json::<CallManifest>(&path).is_ok_and(|existing| existing == manifest) {
        return Ok(ObservationOutcome::Unchanged);
    }
    crate::storage::write_json_secret(&path, &manifest)?;
    prune_manifests(&directory, max_provider_calls)?;
    Ok(ObservationOutcome::Written(path))
}

pub(super) fn load_session_manifests(session_id: &str) -> Result<Vec<CallManifest>> {
    validate_session_id(session_id)?;
    let directory = manifest_directory(session_id)?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut manifests: Vec<_> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter_map(|entry| crate::storage::read_json::<CallManifest>(&entry.path()).ok())
        .collect();
    manifests.sort_by(|left, right| left.call_id.cmp(&right.call_id));
    Ok(manifests)
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.is_empty()
        || session_id == "."
        || session_id == ".."
        || session_id.contains('/')
        || session_id.contains('\\')
    {
        anyhow::bail!("invalid RLM session id")
    }
    Ok(())
}

fn manifest_directory(session_id: &str) -> Result<PathBuf> {
    Ok(crate::storage::jcode_dir()?
        .join("rlm-manifests")
        .join(sanitize(session_id)))
}

fn prune_manifests(directory: &std::path::Path, keep: usize) -> Result<()> {
    let mut files: Vec<_> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    files.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    let remove_count = files.len().saturating_sub(keep);
    for entry in files.into_iter().take(remove_count) {
        fs::remove_file(entry.path())?;
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sanitize(value: &str) -> String {
    value
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

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
