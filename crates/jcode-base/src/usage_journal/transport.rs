//! Task-local, metadata-only observations from the actual transport boundary.
use super::*;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Serialize)]
pub struct TransportObservation {
    pub transport: String,
    pub account_ref: Option<String>,
    pub account_aliases: Vec<String>,
    pub billing_route: Option<String>,
    pub upstream_request_id: Option<String>,
    pub model_actual: Option<String>,
}

pub(super) type Observations = Arc<Mutex<Vec<TransportObservation>>>;
tokio::task_local! { pub(super) static OBSERVATIONS: Observations; }

/// Namespace matches the existing quota observer. Hash identities, never tokens.
pub fn opaque_account_ref(identity: &str) -> Option<String> {
    let identity = identity.trim();
    (!identity.is_empty())
        .then(|| format!("account-{:x}", Sha256::digest(identity.as_bytes()))[..20].to_owned())
}

pub fn subscription_identity(
    account_id: Option<&str>,
    email: Option<&str>,
    transport: &str,
) -> TransportObservation {
    let account_ref = account_id.and_then(opaque_account_ref);
    let mut aliases = Vec::new();
    if let Some(email) = email {
        for value in [email.trim().to_owned(), email.trim().to_lowercase()] {
            if let Some(reference) = opaque_account_ref(&value) {
                if !aliases.contains(&reference) {
                    aliases.push(reference);
                }
            }
        }
    }
    TransportObservation {
        transport: transport.to_owned(),
        account_ref,
        account_aliases: aliases,
        billing_route: Some("codex_subscription".into()),
        ..Default::default()
    }
}

/// Capture now, before tokio::spawn loses the request task-local scope.
pub fn inherit<T>(future: impl Future<Output = T>) -> impl Future<Output = T> {
    let observations = OBSERVATIONS.try_with(Arc::clone).ok();
    async move {
        match observations {
            Some(observations) => OBSERVATIONS.scope(observations, future).await,
            None => future.await,
        }
    }
}

/// Return an index scoped to this one logical call. No transport means no guess.
pub fn begin(observation: TransportObservation) -> Option<usize> {
    OBSERVATIONS
        .try_with(|state| {
            let mut rows = state.lock().ok()?;
            if rows.len() >= 32 {
                return None;
            }
            let index = rows.len();
            rows.push(observation);
            Some(index)
        })
        .ok()
        .flatten()
}

pub fn response(index: Option<usize>, request_id: Option<&str>, actual_model: Option<&str>) {
    let Some(index) = index else {
        return;
    };
    let _ = OBSERVATIONS.try_with(|state| {
        let Ok(mut rows) = state.lock() else {
            return;
        };
        let Some(row) = rows.get_mut(index) else {
            return;
        };
        // Only bounded opaque header/model metadata, never arbitrary headers/body.
        if let Some(id) = request_id.filter(|v| v.len() <= 256 && !v.chars().any(char::is_control))
        {
            row.upstream_request_id = Some(id.to_owned());
        }
        if let Some(model) =
            actual_model.filter(|v| v.len() <= 160 && !v.chars().any(char::is_control))
        {
            row.model_actual = Some(model.to_owned());
        }
    });
}
