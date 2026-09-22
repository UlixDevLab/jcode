//! Hermes-specific credential resolution for local control-plane clients.

use super::{ExternalAuthSource, extract_api_key, source_allowed};
use serde_json::Value;

/// Load the API key which Hermes itself uses to authenticate its local HTTP
/// API. This reads only Hermes's trusted credential store and prefers its
/// configured active provider. Callers must not log or persist the value.
pub fn load_hermes_api_key() -> Option<String> {
    let source = ExternalAuthSource::Hermes;
    if !source_allowed(source) {
        return None;
    }

    let path = crate::storage::validate_external_auth_file(&source.path().ok()?).ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let store: Value = serde_json::from_str(&raw).ok()?;
    let active_provider = store.get("active_provider").and_then(Value::as_str);
    let pool = store.get("credential_pool").and_then(Value::as_object)?;

    let active_entry = active_provider
        .and_then(|provider| pool.get(provider))
        .and_then(Value::as_array)
        .and_then(|entries| entries.first());
    let hermes_entry = pool
        .get("hermes")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first());

    active_entry
        .or(hermes_entry)
        .and_then(|entry| extract_api_key(source, entry))
}

#[cfg(test)]
#[path = "hermes_tests.rs"]
mod tests;
