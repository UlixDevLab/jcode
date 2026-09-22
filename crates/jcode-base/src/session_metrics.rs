//! Lock-free per-session runtime metrics.
//!
//! These metrics are tracked in a process-global registry rather than on the
//! `Agent` struct itself. That is deliberate: callers such as `swarm list`
//! read per-agent stats while the agent may be actively processing a turn and
//! holding its own `Mutex<Agent>` lock. Anything stored behind that lock is
//! unavailable (`try_lock` fails) exactly when an agent is busiest, which is
//! when churn/turn data is most interesting. Keeping these counters in a
//! separate registry lets us observe live activity without contending on the
//! agent lock.
//!
//! The registry stores a small ring of recent token-usage samples per session
//! so we can report a "tokens churned over the last N seconds" rate, plus a
//! cumulative turn counter.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an individual token sample stays in the rolling window.
const SAMPLE_WINDOW: Duration = Duration::from_secs(60);

/// Maximum samples retained per session to bound memory. At one sample per
/// provider response this comfortably covers the rolling window.
const MAX_SAMPLES: usize = 256;

#[derive(Clone, Copy)]
struct TokenSample {
    at: Instant,
    /// Total tokens (input + output + cache) observed in this sample.
    total: u64,
    /// Output tokens only, the best proxy for "work produced".
    output: u64,
}

#[derive(Default)]
struct SessionMetrics {
    samples: Vec<TokenSample>,
    turns: u64,
    cumulative_total_tokens: u64,
    cumulative_output_tokens: u64,
    /// Cumulative *new* work: output plus input the provider actually had to
    /// read fresh, excluding cache hits.
    ///
    /// `cumulative_total_tokens` re-counts the whole conversation on every
    /// turn, because each turn resends it. That makes it a fine "how big is
    /// this session" display figure and a terrible spend meter: a worker
    /// re-reading a cached 120K context blows a 5M budget in ~41 turns while
    /// doing almost nothing new. Budgets must use this field instead.
    cumulative_new_tokens: u64,
    /// Last observed productive activity: token usage, turn start, provider
    /// stream events, or tool start/finish. Used to answer "is this
    /// agent actually doing something right now" independently of lifecycle
    /// status transitions, which can stay unchanged for minutes mid-turn.
    last_activity: Option<Instant>,
}

impl SessionMetrics {
    fn prune(&mut self, now: Instant) {
        let cutoff = now.checked_sub(SAMPLE_WINDOW);
        self.samples.retain(|sample| match cutoff {
            Some(cutoff) => sample.at >= cutoff,
            None => true,
        });
        if self.samples.len() > MAX_SAMPLES {
            let overflow = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(0..overflow);
        }
    }
}

static REGISTRY: Mutex<Option<HashMap<String, SessionMetrics>>> = Mutex::new(None);

fn with_registry<R>(f: impl FnOnce(&mut HashMap<String, SessionMetrics>) -> R) -> Option<R> {
    let mut guard = REGISTRY.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    Some(f(map))
}

/// Record a token-usage sample for a session. Called from the streaming turn
/// loop whenever the provider reports usage.
///
/// `total_tokens` is the whole billed turn (resent context included) and drives
/// display. `new_tokens` is output plus freshly-read input, and is what spend
/// budgets must charge; see [`SessionMetrics::cumulative_new_tokens`].
pub fn record_token_usage_detailed(
    session_id: &str,
    total_tokens: u64,
    output_tokens: u64,
    new_tokens: u64,
) {
    if session_id.is_empty() || (total_tokens == 0 && output_tokens == 0) {
        return;
    }
    let now = Instant::now();
    with_registry(|map| {
        let entry = map.entry(session_id.to_string()).or_default();
        entry.samples.push(TokenSample {
            at: now,
            total: total_tokens,
            output: output_tokens,
        });
        entry.cumulative_total_tokens = entry.cumulative_total_tokens.saturating_add(total_tokens);
        entry.cumulative_output_tokens =
            entry.cumulative_output_tokens.saturating_add(output_tokens);
        entry.cumulative_new_tokens = entry.cumulative_new_tokens.saturating_add(new_tokens);
        entry.last_activity = Some(now);
        entry.prune(now);
    });
}

/// Back-compat shim for callers that cannot separate cached from fresh input.
/// Charges the whole turn as new, which is the conservative reading.
pub fn record_token_usage(session_id: &str, total_tokens: u64, output_tokens: u64) {
    record_token_usage_detailed(session_id, total_tokens, output_tokens, total_tokens);
}

/// Record that a session completed (or started) a turn.
pub fn record_turn(session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    with_registry(|map| {
        let entry = map.entry(session_id.to_string()).or_default();
        entry.turns = entry.turns.saturating_add(1);
        entry.last_activity = Some(Instant::now());
    });
}

/// Record a generic productive activity mark for a session (tool start/finish
/// or throttled provider streaming progress). Cheap: one
/// registry lock and one `Instant` store, so it is safe to call from hot
/// paths as long as callers throttle per-token call sites.
pub fn record_activity(session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    with_registry(|map| {
        let entry = map.entry(session_id.to_string()).or_default();
        entry.last_activity = Some(Instant::now());
    });
}

/// Seconds since the session's last recorded activity, or `None` if the
/// session has never recorded any activity.
pub fn last_activity_age_secs(session_id: &str) -> Option<u64> {
    last_activity_age(session_id).map(|age| age.as_secs())
}

/// Precise duration since the session's last recorded productive activity.
///
/// Worker watchdogs use the sub-second form in tests and to avoid rounding a
/// just-active session up to a whole second. Swarm bookkeeping heartbeats are
/// deliberately not activity; only provider events, token usage, turns, and
/// tool start/finish marks should reset this clock.
pub fn last_activity_age(session_id: &str) -> Option<Duration> {
    with_registry(|map| {
        map.get(session_id)
            .and_then(|entry| entry.last_activity)
            .map(|at| at.elapsed())
    })
    .flatten()
}

/// Snapshot of a session's recent activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionMetricsSnapshot {
    /// Total tokens observed within the lookback window.
    pub recent_total_tokens: u64,
    /// Output tokens observed within the lookback window.
    pub recent_output_tokens: u64,
    /// Cumulative total tokens for the session lifetime.
    pub cumulative_total_tokens: u64,
    /// Cumulative output tokens for the session lifetime.
    pub cumulative_output_tokens: u64,
    /// Cumulative *new* work (output + freshly-read input, cache hits
    /// excluded). This is the figure spend budgets must charge; see
    /// `record_token_usage_detailed`.
    pub cumulative_new_tokens: u64,
    /// Number of turns recorded for the session.
    pub turns: u64,
    /// Seconds since the last observed productive activity (tokens, turns,
    /// provider stream events, or tool events). `None` when no activity was
    /// ever recorded.
    pub last_activity_age_secs: Option<u64>,
}

impl SessionMetricsSnapshot {
    pub fn has_activity(&self) -> bool {
        self.recent_total_tokens > 0 || self.cumulative_total_tokens > 0 || self.turns > 0
    }
}

/// Read a snapshot of a session's metrics, summing token samples within the
/// given lookback window. Returns `None` if the session has no recorded
/// metrics.
pub fn snapshot(session_id: &str, lookback: Duration) -> Option<SessionMetricsSnapshot> {
    let now = Instant::now();
    with_registry(|map| {
        let entry = map.get_mut(session_id)?;
        entry.prune(now);
        let cutoff = now.checked_sub(lookback);
        let mut recent_total = 0u64;
        let mut recent_output = 0u64;
        for sample in &entry.samples {
            let in_window = match cutoff {
                Some(cutoff) => sample.at >= cutoff,
                None => true,
            };
            if in_window {
                recent_total = recent_total.saturating_add(sample.total);
                recent_output = recent_output.saturating_add(sample.output);
            }
        }
        Some(SessionMetricsSnapshot {
            recent_total_tokens: recent_total,
            recent_output_tokens: recent_output,
            cumulative_total_tokens: entry.cumulative_total_tokens,
            cumulative_output_tokens: entry.cumulative_output_tokens,
            cumulative_new_tokens: entry.cumulative_new_tokens,
            turns: entry.turns,
            last_activity_age_secs: entry
                .last_activity
                .map(|at| now.saturating_duration_since(at).as_secs()),
        })
    })
    .flatten()
}

/// Remove a session's metrics, called when the session leaves the swarm or
/// disconnects, to avoid unbounded growth.
pub fn forget(session_id: &str) {
    with_registry(|map| {
        map.remove(session_id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_snapshots_token_usage() {
        let sid = "session_metrics_test_basic";
        forget(sid);
        record_token_usage(sid, 100, 40);
        record_token_usage(sid, 50, 20);
        let snap = snapshot(sid, Duration::from_secs(10)).expect("snapshot");
        assert_eq!(snap.recent_total_tokens, 150);
        assert_eq!(snap.recent_output_tokens, 60);
        assert_eq!(snap.cumulative_total_tokens, 150);
        assert_eq!(snap.cumulative_output_tokens, 60);
        forget(sid);
    }

    #[test]
    fn cached_context_rereads_do_not_burn_the_spend_budget() {
        // The regression this guards: a worker resends its whole conversation
        // every turn, so cumulative_total_tokens re-counts the same cached
        // context each time. Charging a budget against that figure killed
        // workers after ~41 turns of a 120K context while they produced almost
        // nothing new. cumulative_new_tokens must track only real new work.
        let sid = "session_metrics_test_cached_reads";
        forget(sid);
        // 10 turns: 118K cached context re-read, 2K fresh input, 1K output.
        for _ in 0..10 {
            record_token_usage_detailed(sid, 121_000, 1_000, 3_000);
        }
        let snap = snapshot(sid, Duration::MAX).expect("snapshot");
        assert_eq!(snap.cumulative_total_tokens, 1_210_000, "display figure");
        assert_eq!(snap.cumulative_new_tokens, 30_000, "budget figure");
        assert!(
            snap.cumulative_new_tokens * 40 < snap.cumulative_total_tokens,
            "new-token accounting must be dramatically cheaper than resent context"
        );
        forget(sid);
    }

    #[test]
    fn legacy_callers_charge_the_whole_turn() {
        // record_token_usage has no cache breakdown, so it must stay
        // conservative and charge everything rather than silently under-count.
        let sid = "session_metrics_test_legacy_charge";
        forget(sid);
        record_token_usage(sid, 100, 40);
        let snap = snapshot(sid, Duration::MAX).expect("snapshot");
        assert_eq!(snap.cumulative_new_tokens, 100);
        forget(sid);
    }

    #[test]
    fn counts_turns() {
        let sid = "session_metrics_test_turns";
        forget(sid);
        record_turn(sid);
        record_turn(sid);
        record_turn(sid);
        let snap = snapshot(sid, Duration::from_secs(10)).expect("snapshot");
        assert_eq!(snap.turns, 3);
        forget(sid);
    }

    #[test]
    fn ignores_empty_and_zero() {
        let sid = "session_metrics_test_zero";
        forget(sid);
        record_token_usage(sid, 0, 0);
        record_token_usage("", 100, 40);
        assert!(snapshot(sid, Duration::from_secs(10)).is_none());
        forget(sid);
    }

    #[test]
    fn forget_clears_state() {
        let sid = "session_metrics_test_forget";
        forget(sid);
        record_turn(sid);
        assert!(snapshot(sid, Duration::from_secs(10)).is_some());
        forget(sid);
        assert!(snapshot(sid, Duration::from_secs(10)).is_none());
    }

    #[test]
    fn records_last_activity_age() {
        let sid = "session_metrics_test_activity";
        forget(sid);
        assert_eq!(last_activity_age_secs(sid), None);
        record_activity(sid);
        let age = last_activity_age_secs(sid).expect("activity age");
        assert!(age <= 1, "fresh activity should be ~0s old, got {age}");
        let snap = snapshot(sid, Duration::from_secs(10)).expect("snapshot");
        assert!(snap.last_activity_age_secs.is_some());
        forget(sid);
        assert_eq!(last_activity_age_secs(sid), None);
    }

    #[test]
    fn token_usage_and_turns_mark_activity() {
        let sid = "session_metrics_test_activity_sources";
        forget(sid);
        record_token_usage(sid, 10, 5);
        assert!(last_activity_age_secs(sid).is_some());
        forget(sid);
        record_turn(sid);
        assert!(last_activity_age_secs(sid).is_some());
        forget(sid);
    }

    #[test]
    fn record_activity_ignores_empty_session() {
        record_activity("");
        assert_eq!(last_activity_age_secs(""), None);
    }
}
