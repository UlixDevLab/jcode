//! Automatic route recovery for unattended (headless) sessions.
//!
//! An interactive TUI answers a fatal route failure by arming a one-keypress
//! fallback offer: "press <key> to switch to <route> and resend". A headless
//! swarm worker has no keyboard and no human, so that offer is unreachable.
//! Before this module, such a worker simply went `failed` the moment its route
//! broke, even when a perfectly good alternative route was configured. A single
//! exhausted Codex subscription could therefore fail an entire plan while an
//! idle Anthropic login sat unused.
//!
//! This module is the unattended equivalent of that offer: it picks the same
//! next-best route the TUI would have offered, switches the agent onto it, and
//! reports whether the caller should retry the turn.
//!
//! Deliberate boundaries:
//! - Only *route* failures reroute. A tool bug, a user error, or a context
//!   overflow follows the same code path on any model, so switching routes
//!   would waste a second full turn to reach the same failure.
//! - One reroute per turn. The caller retries once; if the alternative route
//!   fails too, the worker fails normally. This cannot loop.

use crate::agent::Agent;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Whether `error` is a route-level failure that a different route could
/// plausibly survive: a broken credential or spent account capacity.
///
/// Everything else (tool errors, context-limit overflows, cancellations, bugs)
/// reproduces identically on another route, so it is not worth a second turn.
pub(super) fn error_is_route_fatal(error: &str) -> bool {
    crate::provider::error_looks_like_credential_failure(error)
        || crate::provider::error_looks_like_capacity_exhaustion(error)
}

/// A route switch that was applied to an unattended session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnattendedReroute {
    pub from_model: String,
    pub to_model: String,
}

impl UnattendedReroute {
    /// Operator-facing note explaining the switch, mirroring the wording the
    /// TUI uses when a human accepts the equivalent fallback offer.
    pub(super) fn notice(&self) -> String {
        format!(
            "⚡ Auto-switched unattended session route after a fatal route error: {} → {}. Retrying the turn.",
            self.from_model, self.to_model
        )
    }
}

/// Try to move an unattended session onto a working route after `error`.
///
/// Returns `Some` when the agent now points at a different route and the caller
/// should retry the turn, `None` when the failure is not route-fatal or no
/// usable alternative exists.
pub(super) async fn try_reroute_unattended_session(
    session_id: &str,
    agent: &Arc<Mutex<Agent>>,
    error: &str,
) -> Option<UnattendedReroute> {
    if !error_is_route_fatal(error) {
        return None;
    }

    let mut guard = agent.lock().await;
    try_reroute_locked_agent(session_id, &mut guard, error)
}

/// Reuse an existing turn reservation instead of attempting to lock it again.
pub(super) fn try_reroute_locked_agent(
    session_id: &str,
    guard: &mut Agent,
    error: &str,
) -> Option<UnattendedReroute> {
    if !error_is_route_fatal(error) {
        return None;
    }

    let from_model = guard.provider_model();
    let routes = guard.model_routes();
    let current_provider = guard.provider_name();
    let current_api_method = guard.session_route_api_method().unwrap_or_default();

    // Same exclusion rules the interactive offer uses: a broken credential or
    // an exhausted quota invalidates every route behind that login, not just
    // the failed model.
    let options = crate::provider::FallbackPickOptions {
        credential_failure: crate::provider::error_looks_like_credential_failure(error),
        capacity_exhausted: crate::provider::error_looks_like_capacity_exhaustion(error),
    };
    let index = crate::provider::pick_next_fallback_route_with_options(
        &routes,
        &from_model,
        &current_provider,
        &current_api_method,
        options,
    )?;
    let route = routes[index].clone();

    let selection = crate::provider::RouteSelection::from_model_route(&route);
    if let Err(switch_error) = guard.set_route_selection(&selection) {
        crate::logging::warn(&format!(
            "Unattended reroute for {session_id} could not select route '{}': {switch_error}",
            route.model
        ));
        return None;
    }
    let to_model = guard.provider_model();

    crate::logging::info(&format!(
        "Unattended reroute for {session_id}: {from_model} -> {to_model} (reason: {error})"
    ));
    Some(UnattendedReroute {
        from_model,
        to_model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_and_capacity_failures_are_route_fatal() {
        assert!(error_is_route_fatal(
            "OpenAI rejected the access token; run /login to re-authenticate"
        ));
        assert!(error_is_route_fatal(
            "Rate limited: The usage limit has been reached. Resets in 4h 12m."
        ));
    }

    #[test]
    fn ordinary_failures_do_not_reroute() {
        // These reproduce identically on any route, so a second full turn on a
        // different model would only waste tokens and time.
        assert!(!error_is_route_fatal("tool 'edit' failed: file not found"));
        assert!(!error_is_route_fatal("context length exceeded"));
        assert!(!error_is_route_fatal("turn cancelled by user"));
        // A transient burst limit is retried in the provider loop, not by
        // abandoning the route the operator chose.
        assert!(!error_is_route_fatal("429 Too Many Requests"));
    }

    #[test]
    fn notice_names_both_routes() {
        let notice = UnattendedReroute {
            from_model: "gpt-5.5".to_string(),
            to_model: "claude-sonnet-5".to_string(),
        }
        .notice();
        assert!(notice.contains("gpt-5.5"));
        assert!(notice.contains("claude-sonnet-5"));
        assert!(notice.contains("Retrying"));
    }
}
