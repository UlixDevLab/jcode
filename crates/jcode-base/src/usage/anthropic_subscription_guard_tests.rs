//! Focused tests for the concurrent Anthropic OAuth subscription monitor.
//! No test calls the real usage endpoint or reads credentials.
#![cfg(test)]

use super::*;
use anyhow::anyhow;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn temp_home() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "jcode_claude_monitor_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
struct Sandbox {
    home: PathBuf,
    previous_home: Option<std::ffi::OsString>,
}
impl Sandbox {
    fn new() -> Self {
        Self {
            home: temp_home(),
            previous_home: std::env::var_os("JCODE_HOME"),
        }
    }
    fn install(&self) {
        crate::env::set_var("JCODE_HOME", &self.home);
    }
    fn state(&self) -> PathBuf {
        self.home.join("anthropic_subscription_guard/state.json")
    }
    fn events(&self) -> PathBuf {
        self.home.join("anthropic_subscription_guard/events.jsonl")
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(previous) = self.previous_home.take() {
            crate::env::set_var("JCODE_HOME", previous);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    crate::storage::lock_test_env()
}
fn config() -> Arc<GuardConfig> {
    Arc::new(GuardConfig {
        enabled: true,
        threshold_pct: 5.0,
        max_preflight_pct: 95.0,
        fail_closed: true,
        allowed_models: vec![],
    })
}
fn fetcher(samples: Vec<Result<UsageMeterSnapshot>>) -> TestFetcher {
    let samples = Arc::new(Mutex::new(samples));
    Arc::new(move |_| {
        let samples = samples.clone();
        Box::pin(async move { samples.lock().unwrap().remove(0) })
    })
}
fn sample(pct: f32) -> Result<UsageMeterSnapshot> {
    Ok(UsageMeterSnapshot {
        five_hour_pct: pct,
        five_hour_resets_at: Some("window-1".to_string()),
    })
}

#[tokio::test]
async fn parallel_acquire_does_not_hold_lock_across_meter_fetch() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    let slow = Arc::new(|_: &str| {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            sample(10.0)
        })
            as std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<UsageMeterSnapshot>> + Send>,
            >
    });
    set_test_fetcher(Some(Arc::new(move |token| slow(token))));
    let started = Instant::now();
    let (first, second) = tokio::join!(
        acquire_anthropic_subscription_guard("not-a-token", "claude-sonnet-5", "one"),
        acquire_anthropic_subscription_guard("not-a-token", "claude-opus-5", "two")
    );
    assert!(first.is_ok() && second.is_ok());
    assert!(
        started.elapsed() < Duration::from_millis(210),
        "meter work must overlap rather than serialize"
    );
    let state = read_state(&sandbox.state()).unwrap();
    assert_eq!(state.active_calls.len(), 2);
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[test]
fn monotonic_sample_merge_counts_overlapping_movement_once() {
    let mut state = GuardState::default();
    let cfg = config();
    apply_sample(
        &mut state,
        &UsageMeterSnapshot {
            five_hour_pct: 10.0,
            five_hour_resets_at: Some("w".into()),
        },
        &cfg,
    );
    apply_sample(
        &mut state,
        &UsageMeterSnapshot {
            five_hour_pct: 14.0,
            five_hour_resets_at: Some("w".into()),
        },
        &cfg,
    );
    // An out-of-order 12% post sample cannot add another interval delta.
    apply_sample(
        &mut state,
        &UsageMeterSnapshot {
            five_hour_pct: 12.0,
            five_hour_resets_at: Some("w".into()),
        },
        &cfg,
    );
    assert_eq!(state.latest_five_hour_pct, Some(14.0));
    assert_eq!(state.aggregate_observed_movement_pct, 4.0);
    assert!(!state.approval_pending);
}

#[tokio::test]
async fn threshold_pauses_future_calls_but_returns_completed_call() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    set_test_fetcher(Some(fetcher(vec![
        sample(10.0),
        sample(16.0),
        sample(16.0),
    ])));
    let lease = acquire_anthropic_subscription_guard("not-a-token", "claude-haiku-5", "completed")
        .await
        .unwrap();
    assert_eq!(
        lease.finish("not-a-token").await.unwrap(),
        FinishOutcome::Killed
    );
    let state = read_state(&sandbox.state()).unwrap();
    assert!(state.approval_pending);
    assert!(state.active_calls.is_empty());
    assert!(matches!(
        acquire_anthropic_subscription_guard("not-a-token", "claude-haiku-5", "next").await,
        Err(AdmissionError::ApprovalRequired { .. })
    ));
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[tokio::test]
async fn approval_resets_baseline_to_latest_verified_percent() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    let mut state = GuardState::default();
    state.window_baseline_pct = Some(10.0);
    state.latest_five_hour_pct = Some(16.0);
    state.aggregate_observed_movement_pct = 6.0;
    state.approval_pending = true;
    write_state(&sandbox.state(), &state).unwrap();
    let approved = control_claude_monitor(ClaudeMonitorCommand::Approve).unwrap();
    assert!(!approved.approval_pending);
    assert_eq!(approved.window_baseline_pct, Some(16.0));
    assert_eq!(approved.aggregate_observed_movement_pct, 0.0);
}

#[tokio::test]
async fn off_blocks_new_calls_and_on_does_not_silently_approve() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    set_test_fetcher(Some(fetcher(vec![sample(20.0)])));
    control_claude_monitor(ClaudeMonitorCommand::Off).unwrap();
    assert!(matches!(
        acquire_anthropic_subscription_guard("not-a-token", "claude-sonnet-5", "off").await,
        Err(AdmissionError::OperatorDisabled)
    ));
    let mut state = claude_monitor_status().unwrap();
    state.approval_pending = true;
    write_state(&sandbox.state(), &state).unwrap();
    let on = control_claude_monitor(ClaudeMonitorCommand::On).unwrap();
    assert!(on.operator_enabled && on.approval_pending);
    assert!(matches!(
        acquire_anthropic_subscription_guard("not-a-token", "claude-sonnet-5", "pending").await,
        Err(AdmissionError::ApprovalRequired { .. })
    ));
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[tokio::test]
async fn records_per_session_counters_and_nonadditive_interval_delta() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    set_test_fetcher(Some(fetcher(vec![sample(30.0), sample(32.0)])));
    let lease = acquire_anthropic_subscription_guard("not-a-token", "claude-sonnet-5", "telemetry")
        .await
        .unwrap();
    lease.finish("not-a-token").await.unwrap();
    let state = read_state(&sandbox.state()).unwrap();
    let telemetry = state.sessions.get("unknown-session").unwrap();
    assert_eq!(telemetry.started_calls, 1);
    assert_eq!(telemetry.finished_calls, 1);
    assert_eq!(telemetry.last_interval_delta_pct, Some(2.0));
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[tokio::test]
async fn meter_failure_warns_and_does_not_create_durable_pause() {
    let _guard = env_lock();
    // This test means "no meter reading is available at all". The meter cache is
    // process-global, so a reading left by another test would otherwise be
    // served here and mask the failure path under test.
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    set_test_fetcher(Some(fetcher(vec![Err(anyhow!(
        "deliberate test meter failure"
    ))])));
    let lease =
        acquire_anthropic_subscription_guard("not-a-token", "claude-sonnet-5", "meter-failure")
            .await
            .unwrap();
    let state = read_state(&sandbox.state()).unwrap();
    assert!(!state.approval_pending);
    assert!(state.last_warning.unwrap().contains("meter unavailable"));
    drop(lease);
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[test]
fn model_family_allows_claude_models_but_never_admits_fable_by_wildcard() {
    let cfg = config();
    assert!(cfg.model_allowed("claude-sonnet-5"));
    assert!(cfg.model_allowed("claude-haiku-5"));
    assert!(!cfg.model_allowed("gpt-5"));
    // `config()` uses the `claude-*` wildcard: it must not reach Fable.
    assert!(!cfg.model_allowed("claude-fable-5"));
}

#[test]
fn fable_requires_an_exact_allowlist_entry() {
    let exact = GuardConfig {
        enabled: true,
        threshold_pct: 5.0,
        max_preflight_pct: 95.0,
        fail_closed: true,
        allowed_models: vec!["claude-fable-5".to_string()],
    };
    assert!(
        exact.model_allowed("claude-fable-5"),
        "an operator naming Fable exactly must be able to select it"
    );
    // Naming Fable exactly does not widen the list to the rest of the family.
    assert!(!exact.model_allowed("claude-sonnet-5"));

    let empty = GuardConfig {
        allowed_models: Vec::new(),
        ..exact.clone()
    };
    assert!(
        !empty.model_allowed("claude-fable-5"),
        "an empty allowlist must not admit Fable"
    );
}

#[test]
fn window_reset_clears_pending_threshold_pause() {
    let mut state = GuardState {
        window_reset_id: "old".into(),
        approval_pending: true,
        window_baseline_pct: Some(10.0),
        latest_five_hour_pct: Some(16.0),
        aggregate_observed_movement_pct: 6.0,
        ..GuardState::default()
    };
    assert!(reconcile_window_reset(&mut state, "new"));
    assert!(!state.approval_pending);
    assert_eq!(state.window_baseline_pct, None);
    assert_eq!(state.aggregate_observed_movement_pct, 0.0);
}

#[test]
fn schema_one_kill_migrates_to_approval_pending_without_credentials() {
    let _guard = env_lock();
    let sandbox = Sandbox::new();
    sandbox.install();
    std::fs::create_dir_all(sandbox.state().parent().unwrap()).unwrap();
    std::fs::write(sandbox.state(), r#"{"version":1,"window_reset_id":"w","post_pct":44.0,"kill":{"reason":"old","killed_at_unix_ms":1,"pre_pct":1,"post_pct":2,"single_delta_pct":1,"cumulative_delta_pct":1,"window_resets_at":null}}"#).unwrap();
    let state = read_state(&sandbox.state()).unwrap();
    assert_eq!(state.version, STATE_SCHEMA_VERSION);
    assert!(state.approval_pending);
    assert_eq!(state.latest_five_hour_pct, Some(44.0));
}

#[tokio::test]
async fn state_and_events_redact_bearer_token() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let sandbox = Sandbox::new();
    sandbox.install();
    set_test_guard_config_override(Some(config()));
    set_test_fetcher(Some(fetcher(vec![sample(1.0)])));
    let secret = "token-that-must-never-persist";
    let _lease = acquire_anthropic_subscription_guard(secret, "claude-sonnet-5", "redaction")
        .await
        .unwrap();
    let state = std::fs::read_to_string(sandbox.state()).unwrap();
    let events = std::fs::read_to_string(sandbox.events()).unwrap();
    assert!(!state.contains(secret));
    assert!(!events.contains(secret));
    assert!(events.contains("sample") && events.contains("start"));
    set_test_fetcher(None);
    set_test_guard_config_override(None);
}

#[test]
fn cli_control_events_are_durable_and_status_is_provider_free() {
    let _guard = env_lock();
    let sandbox = Sandbox::new();
    sandbox.install();
    control_claude_monitor(ClaudeMonitorCommand::Off).unwrap();
    control_claude_monitor(ClaudeMonitorCommand::On).unwrap();
    control_claude_monitor(ClaudeMonitorCommand::Approve).unwrap();
    let status = claude_monitor_status().unwrap();
    assert!(status.operator_enabled);
    let events = std::fs::read_to_string(sandbox.events()).unwrap();
    assert!(
        events.contains("\"off\"") && events.contains("\"on\"") && events.contains("\"approve\"")
    );
}

/// The meter endpoint reports rolling five-hour/seven-day windows, so two
/// fetches a few milliseconds apart return the same thing. Before caching,
/// every guarded call spent two requests (preflight + post-call), which is what
/// rate-limited the endpoint and left the guard unmetered.
#[tokio::test]
async fn meter_reading_is_reused_within_the_cache_window() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let calls = Arc::new(Mutex::new(0usize));
    let seen = calls.clone();
    set_test_fetcher(Some(Arc::new(move |_| {
        let seen = seen.clone();
        Box::pin(async move {
            *seen.lock().unwrap() += 1;
            sample(42.0)
        })
    })));

    let first = fetch_meter_cached("token").await.expect("first reading");
    let second = fetch_meter_cached("token").await.expect("cached reading");

    assert_eq!(first.five_hour_pct, 42.0);
    assert_eq!(second.five_hour_pct, 42.0);
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "second read inside the TTL must not hit the network"
    );
    set_test_fetcher(None);
    reset_meter_cache_for_tests();
}

/// A 429 is caused by asking too often, so the response to one is to stop
/// asking and keep serving the last good reading. Reporting `None` here would
/// tell the guard it is unmetered when we actually know the usage.
#[tokio::test]
async fn rate_limited_meter_serves_the_last_good_reading_instead_of_nothing() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let calls = Arc::new(Mutex::new(0usize));
    let seen = calls.clone();
    set_test_fetcher(Some(Arc::new(move |_| {
        let seen = seen.clone();
        Box::pin(async move {
            let mut n = seen.lock().unwrap();
            *n += 1;
            if *n == 1 {
                sample(55.0)
            } else {
                Err(anyhow!("usage api returned 429 Too Many Requests"))
            }
        })
    })));

    let good = fetch_meter_cached("token").await.expect("first reading");
    assert_eq!(good.five_hour_pct, 55.0);

    // Force the TTL to lapse so the next call really attempts a fetch.
    expire_meter_cache_for_tests();

    let after_429 = fetch_meter_cached("token").await;
    assert_eq!(
        after_429.map(|s| s.five_hour_pct),
        Some(55.0),
        "a rate limit must fall back to the cached reading, not report unmetered"
    );

    // The backoff must now suppress further fetches entirely.
    let before = *calls.lock().unwrap();
    let _ = fetch_meter_cached("token").await;
    assert_eq!(
        *calls.lock().unwrap(),
        before,
        "inside the backoff window we must not spend another request"
    );
    set_test_fetcher(None);
    reset_meter_cache_for_tests();
}

/// Retrying into an active rate limit is what produced the limit, so the
/// post-call path must give up immediately on a 429 rather than looping for
/// the full POST_FETCH_TIMEOUT.
#[tokio::test]
async fn post_meter_does_not_retry_into_a_rate_limit() {
    let _guard = env_lock();
    reset_meter_cache_for_tests();
    let calls = Arc::new(Mutex::new(0usize));
    let seen = calls.clone();
    set_test_fetcher(Some(Arc::new(move |_| {
        let seen = seen.clone();
        Box::pin(async move {
            *seen.lock().unwrap() += 1;
            Err(anyhow!("usage api returned 429 Too Many Requests"))
        })
    })));

    let started = Instant::now();
    let result = fetch_post_meter("token").await;

    assert!(result.is_err());
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "a 429 must stop the retry loop after one attempt"
    );
    assert!(
        started.elapsed() < post_fetch_timeout_for_tests(),
        "must return immediately rather than burning the retry budget"
    );
    set_test_fetcher(None);
    reset_meter_cache_for_tests();
}
