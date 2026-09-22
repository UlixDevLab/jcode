use super::consent::ConsentOutcome;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
enum FamilyState {
    Pending,
    Denied,
    Approved { expires_at: DateTime<Utc> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Reservation {
    Prompt { generation: u64 },
    Reused { expires_at: DateTime<Utc> },
}

#[derive(Debug, Default)]
struct SessionConsentState {
    generation: u64,
    families: HashMap<String, FamilyState>,
}

static SESSION_CONSENT: LazyLock<Mutex<HashMap<String, SessionConsentState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn begin_turn(session_id: &str) {
    let mut sessions = states();
    let state = sessions.entry(session_id.to_string()).or_default();
    state.generation = state.generation.wrapping_add(1);
    // Denials and pending requests are scoped to one direct user turn. A
    // genuine capability-boundary approval is session-scoped and retains its
    // original expiry rather than being refreshed by later tool calls.
    state
        .families
        .retain(|_, value| matches!(value, FamilyState::Approved { .. }));
}

pub(super) fn clear_session(session_id: &str) {
    states().remove(session_id);
}

pub(super) fn reserve(
    session_id: &str,
    family: &str,
    allows_session_reuse: bool,
    now: DateTime<Utc>,
) -> Result<Reservation> {
    let mut sessions = states();
    let state = sessions.entry(session_id.to_string()).or_default();
    if let Some(FamilyState::Approved { expires_at }) = state.families.get(family).cloned() {
        if allows_session_reuse && now < expires_at {
            return Ok(Reservation::Reused { expires_at });
        }
        state.families.remove(family);
    }
    if let Some(existing) = state.families.get(family) {
        let reason = match existing {
            FamilyState::Pending => "approval is already pending for an equivalent operation",
            FamilyState::Denied => {
                "equivalent operations are blocked until the next direct user message"
            }
            FamilyState::Approved { .. } => unreachable!("expired approvals are removed above"),
        };
        crate::logging::event_warn(
            "CONSENT_GUARDRAIL",
            vec![
                ("decision".to_string(), "block".to_string()),
                ("session_id".to_string(), session_id.to_string()),
                ("class".to_string(), family.to_string()),
                ("reason".to_string(), reason.to_string()),
            ],
        );
        return Err(anyhow::anyhow!(reason));
    }
    state
        .families
        .insert(family.to_string(), FamilyState::Pending);
    Ok(Reservation::Prompt {
        generation: state.generation,
    })
}

pub(super) fn finish(
    session_id: &str,
    family: &str,
    generation: u64,
    outcome: ConsentOutcome,
    expires_at: Option<DateTime<Utc>>,
) {
    let mut sessions = states();
    let Some(state) = sessions.get_mut(session_id) else {
        return;
    };
    if state.generation != generation {
        return;
    }
    match outcome {
        ConsentOutcome::Approved => {
            if let Some(expires_at) = expires_at {
                state
                    .families
                    .insert(family.to_string(), FamilyState::Approved { expires_at });
            } else {
                state.families.remove(family);
            }
        }
        ConsentOutcome::Denied => {
            state
                .families
                .insert(family.to_string(), FamilyState::Denied);
        }
        ConsentOutcome::Unavailable => {
            // A disconnected approval route or timeout is not a user refusal.
            // Clear the pending reservation so a later call with a recovered
            // current TUI route can request real consent in this same turn.
            state.families.remove(family);
        }
    }
    crate::logging::event_info(
        "CONSENT_GUARDRAIL",
        vec![
            (
                "decision".to_string(),
                match outcome {
                    ConsentOutcome::Approved => "approved",
                    ConsentOutcome::Denied => "denied",
                    ConsentOutcome::Unavailable => "unavailable",
                }
                .to_string(),
            ),
            ("session_id".to_string(), session_id.to_string()),
            ("class".to_string(), family.to_string()),
        ],
    );
}

fn states() -> std::sync::MutexGuard<'static, HashMap<String, SessionConsentState>> {
    SESSION_CONSENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
