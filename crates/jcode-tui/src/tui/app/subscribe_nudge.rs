//! Hosted-model nudge shown after a provider rate limit.
//!
//! Surfaces (never a blocking screen):
//!   - Trigger A (value prop): the provider just rate-limited the user. The
//!     rate-limit system line gains a one-line "get more tokens" suffix.
//!
//! Delivery rules:
//!   - At most once per week across all sessions (persisted timestamp), and
//!     at most once per session.
//!   - Never while onboarding is active, never for users who already hold
//!     jcode account credentials, never in replay/test runtimes.
//!
//! `/subscribe` remains a compatibility alias for the hosted-model pitch.

use super::{App, AppRuntimeMode, DisplayMessage};
use std::path::PathBuf;
use std::time::Duration;

/// Minimum gap between two nudges, across sessions.
const NUDGE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Copy appended to the rate-limit system message (trigger A).
pub(super) const RATE_LIMIT_NUDGE_LINE: &str =
    "Avoid provider rate limits with Jcode hosted models: /hosted";
/// Persisted nudge bookkeeping (one small JSON file under the jcode dir).
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct NudgeState {
    /// Unix seconds when a nudge was last shown, across all sessions.
    #[serde(default)]
    last_shown_unix: u64,
    /// Which trigger produced the last nudge (diagnostic only).
    #[serde(default)]
    last_trigger: String,
}

fn state_path() -> Option<PathBuf> {
    crate::storage::jcode_dir()
        .ok()
        .map(|dir| dir.join("subscribe-nudge.json"))
}

fn load_state() -> NudgeState {
    let Some(path) = state_path() else {
        return NudgeState::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn store_state(state: &NudgeState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Ok(raw) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(path, raw);
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|dur| dur.as_secs())
        .unwrap_or(0)
}

/// Whether the persisted weekly gate allows another nudge at `now_unix`.
fn weekly_gate_allows(last_shown_unix: u64, now_unix: u64) -> bool {
    now_unix.saturating_sub(last_shown_unix) >= NUDGE_INTERVAL.as_secs()
}

/// The full hosted-model pitch. Reuses the live curated catalog so models
/// never drift from the account status surface.
pub(super) fn subscribe_pitch_markdown() -> String {
    let mut message = String::from("Jcode hosted models\n\n");
    message.push_str("No subscription. Set a monthly spending limit and pay only for usage:\n\n");
    message.push_str("  - You control the monthly limit and can change it from your account\n");
    message.push_str("  - Usage milestones send warnings without slowing your requests\n");
    let model_names: Vec<&str> = crate::subscription_catalog::curated_models()
        .iter()
        .map(|model| model.display_name)
        .collect();
    if !model_names.is_empty() {
        message.push_str(&format!(
            "  - Curated catalog: {}\n",
            model_names.join(", ")
        ));
    }
    message.push_str("  - Sign in once: Jcode saves the hosted API key and routes the rest\n");
    message.push_str("  - Automatic failover routing when a provider has a bad day\n");
    message
        .push_str("  - Billing starts at $20 of usage, then uses progressively larger tranches\n");
    message.push_str("  - Any remaining usage is billed at your limit or at month end\n");
    message.push_str("  - Funds Jcode development while the software stays open source\n");

    message.push_str("\nStart: /login jcode (browser approval, no key pasted into the terminal)\n");
    message.push_str("Usage anytime: /usage");
    message
}

/// Per-session state for the subscribe nudge.
#[derive(Default)]
pub(super) struct SubscribeNudgeState {
    /// Whether the nudge already fired this session.
    shown_this_session: bool,
}

impl App {
    /// Returns true when the rate-limit nudge may be
    /// shown, and records the claim (session flag + weekly file) so a `true`
    /// must be followed by actually showing it.
    pub(super) fn claim_subscribe_nudge(&mut self) -> bool {
        // Deterministic playback and unit-test harnesses must not grow
        // marketing lines (golden transcripts assert exact output).
        if cfg!(test) || !matches!(self.runtime_mode, AppRuntimeMode::RemoteClient) {
            return false;
        }
        if self.subscribe_nudge.shown_this_session {
            return false;
        }
        // Never pitch during onboarding: the user has gotten zero value yet.
        if self.onboarding_flow_active() {
            return false;
        }
        // Never pitch existing jcode account holders.
        if crate::subscription_catalog::has_credentials() {
            return false;
        }
        let now = now_unix();
        let mut state = load_state();
        if !weekly_gate_allows(state.last_shown_unix, now) {
            return false;
        }
        state.last_shown_unix = now;
        state.last_trigger = "rate_limited".to_string();
        store_state(&state);
        self.subscribe_nudge.shown_this_session = true;
        true
    }

    /// Render the `/subscribe` pitch into the transcript.
    pub(super) fn show_subscribe_pitch(&mut self) {
        self.push_display_message(DisplayMessage::system(subscribe_pitch_markdown()));
        self.set_status_notice("Hosted models: /login jcode to start");
    }
}

impl App {
    /// Rate-limit notice line, with the weekly-gated subscribe nudge appended
    /// when the gate allows (user is blocked on tokens right now).
    pub(super) fn rate_limit_notice_with_nudge(&mut self, reset_secs: u64) -> String {
        let mut line = format!(
            "⏳ Rate limit hit. Will auto-retry in {} seconds...",
            reset_secs
        );
        if self.claim_subscribe_nudge() {
            line.push_str(&format!("\n{}", RATE_LIMIT_NUDGE_LINE));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_gate_blocks_within_a_week_and_allows_after() {
        let week = NUDGE_INTERVAL.as_secs();
        // Fresh state (never shown, last_shown 0) allows immediately.
        assert!(weekly_gate_allows(0, week));
        assert!(weekly_gate_allows(0, u64::MAX));
        assert!(weekly_gate_allows(1_000, 1_000 + week));
        assert!(!weekly_gate_allows(1_000, 1_000 + week - 1));
        // `now` before `last_shown` must saturate rather than wrap into
        // "a week has passed" (clock skew / restored backup).
        assert!(!weekly_gate_allows(1_000, 0));
    }

    #[test]
    #[test]
    fn rate_limit_copy_leads_with_the_hosted_model_value_prop() {
        assert!(RATE_LIMIT_NUDGE_LINE.starts_with("Avoid provider rate limits"));
        assert!(RATE_LIMIT_NUDGE_LINE.contains("/hosted"));
    }

    #[test]
    fn pitch_explains_limits_warnings_tranches_and_next_step() {
        let pitch = subscribe_pitch_markdown();
        assert!(pitch.contains("No subscription"));
        assert!(pitch.contains("monthly spending limit"));
        assert!(pitch.contains("warnings without slowing"));
        assert!(pitch.contains("$20"));
        assert!(pitch.contains("progressively larger tranches"));
        assert!(pitch.contains("month end"));
        assert!(pitch.contains("open source"));
        assert!(pitch.contains("/login jcode"));
        assert!(pitch.contains("/usage"));
        for model in crate::subscription_catalog::curated_models() {
            assert!(pitch.contains(model.display_name));
        }
    }
}
