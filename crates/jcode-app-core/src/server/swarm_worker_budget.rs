//! Deterministic runtime budgets for delegated agent turns.
//!
//! Prompt instructions are advisory. This wrapper is the server-held backstop for
//! provider hangs and runaway tool/model loops in headless swarm workers.

use std::fmt;
use std::future::Future;
use std::time::Duration;

const DEFAULT_MAX_EFFECTIVE_TOKENS: u64 = 5_000_000;
// Productive delegated work routinely takes longer than ten minutes. A hard
// wall-clock cutoff is therefore opt-in; the default safety backstop is an
// inactivity watchdog, which still stops genuinely hung providers/tools.
const DEFAULT_MAX_RUNTIME_SECS: u64 = 0;
const DEFAULT_MAX_INACTIVITY_SECS: u64 = 2 * 60 * 60;
const DEFAULT_POLL_MILLIS: u64 = 1_000;
const MAX_TOKENS_ENV: &str = "JCODE_SWARM_WORKER_MAX_EFFECTIVE_TOKENS";
const MAX_RUNTIME_ENV: &str = "JCODE_SWARM_WORKER_MAX_RUNTIME_SECS";
const MAX_INACTIVITY_ENV: &str = "JCODE_SWARM_WORKER_MAX_INACTIVITY_SECS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkerBudgetKind {
    Runtime,
    Inactivity,
    Tokens,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WorkerBudgetExceeded {
    pub kind: WorkerBudgetKind,
    pub observed: u64,
    pub limit: u64,
}

impl WorkerBudgetExceeded {
    fn runtime(observed: Duration, limit: Duration) -> Self {
        Self {
            kind: WorkerBudgetKind::Runtime,
            observed: observed.as_secs(),
            limit: limit.as_secs(),
        }
    }

    fn inactivity(observed: Duration, limit: Duration) -> Self {
        Self {
            kind: WorkerBudgetKind::Inactivity,
            observed: observed.as_secs(),
            limit: limit.as_secs(),
        }
    }

    fn tokens(observed: u64, limit: u64) -> Self {
        Self {
            kind: WorkerBudgetKind::Tokens,
            observed,
            limit,
        }
    }
}

impl fmt::Display for WorkerBudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            WorkerBudgetKind::Runtime => write!(
                f,
                "worker runtime budget exceeded: {}s >= {}s",
                self.observed, self.limit
            ),
            WorkerBudgetKind::Inactivity => write!(
                f,
                "worker inactivity budget exceeded: no productive activity for {}s >= {}s",
                self.observed, self.limit
            ),
            WorkerBudgetKind::Tokens => write!(
                f,
                "worker token budget exceeded: {} >= {} effective tokens",
                self.observed, self.limit
            ),
        }
    }
}

impl std::error::Error for WorkerBudgetExceeded {}

/// Runtime/inactivity cutoffs are transient execution failures and may be
/// retried on a fresh worker. Token exhaustion is deliberate spend control and
/// remains terminal until the coordinator explicitly changes the budget.
pub(super) fn retryable_budget_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<WorkerBudgetExceeded>()
        .map(|exceeded| {
            matches!(
                exceeded.kind,
                WorkerBudgetKind::Runtime | WorkerBudgetKind::Inactivity
            )
        })
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WorkerBudget {
    pub max_effective_tokens: u64,
    pub max_runtime: Duration,
    pub max_inactivity: Duration,
    pub poll_interval: Duration,
}

impl WorkerBudget {
    /// Build a worker budget. The token ceiling picks the first match in this
    /// order:
    ///
    /// 1. Explicit `JCODE_SWARM_WORKER_MAX_EFFECTIVE_TOKENS` env value, even
    ///    when the model is MiniMax (operator override always wins). `0` here
    ///    means "no token cap".
    /// 2. A direct MiniMax worker — model id starts with the `minimax:`
    ///    provider prefix or with `MiniMax-M*` — defaults to **no** token cap
    ///    because the operator has an unlimited-token allocation. Worker
    ///    liveness monitoring still applies.
    /// 3. Every other worker falls back to `DEFAULT_MAX_EFFECTIVE_TOKENS`.
    ///
    /// `JCODE_SWARM_WORKER_MAX_RUNTIME_SECS` is an optional hard wall-clock
    /// ceiling (`0`, the default, disables it). Hung work is bounded separately
    /// by `JCODE_SWARM_WORKER_MAX_INACTIVITY_SECS` (default two hours), which
    /// resets on real provider/tool/session activity and therefore does not kill
    /// a healthy long-running worker merely because it crossed ten minutes.
    pub fn from_env_for_model(model: Option<&str>) -> Self {
        let token_cap = token_cap_for(model);
        Self {
            max_effective_tokens: token_cap,
            max_runtime: Duration::from_secs(env_u64(MAX_RUNTIME_ENV, DEFAULT_MAX_RUNTIME_SECS)),
            max_inactivity: Duration::from_secs(env_u64(
                MAX_INACTIVITY_ENV,
                DEFAULT_MAX_INACTIVITY_SECS,
            )),
            poll_interval: Duration::from_millis(DEFAULT_POLL_MILLIS),
        }
    }
}

/// Resolve the effective token cap for `model`, honoring the env override.
pub(super) fn token_cap_for(model: Option<&str>) -> u64 {
    if let Some(value) = std::env::var(MAX_TOKENS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
    {
        // Explicit env value (including 0 = uncapped) always wins.
        return value;
    }
    if model_is_minimax(model) {
        return 0;
    }
    DEFAULT_MAX_EFFECTIVE_TOKENS
}

/// True when `model` is a MiniMax worker on MiniMax's unlimited-token tier:
/// the `minimax:` provider-prefixed id (`minimax:MiniMax-M3`), a bare
/// `MiniMax-M*` name, or a `minimax/` gateway route (`minimax/M3`,
/// `minimax/M2.7-highspeed`) such as the Stables-proxied ids Jcode Lite
/// delegates to. All three reach the same tier, so capping the gateway spelling
/// while exempting the direct one was an accident of string matching rather
/// than a policy: Lite's own default routes were silently capped.
///
/// We still do NOT match OpenAI/Anthropic-routed MiniMax models, which bill
/// against those providers' metered quotas.
fn model_is_minimax(model: Option<&str>) -> bool {
    let Some(model) = model else { return false };
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.starts_with("minimax:")
        || normalized.starts_with("minimax-m")
        || normalized.starts_with("minimax/")
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

/// Tokens charged against a worker's budget.
///
/// This is *new* work (output plus input the provider actually read fresh),
/// not `cumulative_total_tokens`. Each turn resends the whole conversation, so
/// the cumulative total re-counts the same context every turn: a worker on a
/// cached 120K context would hit a 5M cap in ~41 turns while producing almost
/// nothing, which punishes exactly the long, careful, cache-friendly work the
/// budget is supposed to allow.
fn cumulative_tokens(session_id: &str) -> u64 {
    crate::session_metrics::snapshot(session_id, Duration::MAX)
        .map(|metrics| metrics.cumulative_new_tokens)
        .unwrap_or(0)
}

pub(super) async fn run_with_model<F>(
    session_id: &str,
    future: F,
    model: Option<&str>,
) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    run_with_budget(session_id, future, WorkerBudget::from_env_for_model(model)).await
}

async fn run_with_budget<F>(session_id: &str, future: F, budget: WorkerBudget) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>>,
{
    if budget.max_effective_tokens == 0
        && budget.max_runtime.is_zero()
        && budget.max_inactivity.is_zero()
    {
        return future.await;
    }

    crate::session_metrics::record_activity(session_id);
    let initial_tokens = cumulative_tokens(session_id);
    let started = tokio::time::Instant::now();
    let mut work = std::pin::pin!(future);
    let mut ticker = tokio::time::interval(budget.poll_interval.max(Duration::from_millis(1)));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = ticker.tick() => {
                let elapsed = started.elapsed();
                if !budget.max_runtime.is_zero() && elapsed >= budget.max_runtime {
                    return Err(WorkerBudgetExceeded::runtime(elapsed, budget.max_runtime).into());
                }
                if !budget.max_inactivity.is_zero() {
                    let inactive_for = crate::session_metrics::last_activity_age(session_id)
                        .unwrap_or(elapsed);
                    if inactive_for >= budget.max_inactivity {
                        return Err(
                            WorkerBudgetExceeded::inactivity(inactive_for, budget.max_inactivity)
                                .into(),
                        );
                    }
                }
                let used = cumulative_tokens(session_id).saturating_sub(initial_tokens);
                if budget.max_effective_tokens > 0 && used >= budget.max_effective_tokens {
                    return Err(
                        WorkerBudgetExceeded::tokens(used, budget.max_effective_tokens).into(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serialize env mutation across these tests so a stray value can't leak.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clear_budget_env() {
        unsafe { std::env::remove_var(MAX_TOKENS_ENV) };
        unsafe { std::env::remove_var(MAX_RUNTIME_ENV) };
        unsafe { std::env::remove_var(MAX_INACTIVITY_ENV) };
    }

    #[test]
    fn minimax_prefixed_model_defaults_to_uncapped_token_budget() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        let budget = WorkerBudget::from_env_for_model(Some("minimax:MiniMax-M2.7-highspeed"));
        assert_eq!(budget.max_effective_tokens, 0);
        assert_eq!(budget.max_runtime, Duration::ZERO);
        assert_eq!(
            budget.max_inactivity,
            Duration::from_secs(DEFAULT_MAX_INACTIVITY_SECS)
        );
    }

    #[test]
    fn bare_minimax_model_defaults_to_uncapped_token_budget() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        let budget = WorkerBudget::from_env_for_model(Some("MINIMAX-M3"));
        assert_eq!(budget.max_effective_tokens, 0);
    }

    #[test]
    fn stables_routed_minimax_gateway_ids_are_uncapped() {
        // Regression: Jcode Lite delegates to `minimax/M3` and
        // `minimax/M2.7-highspeed` (Stables-proxied). Those hit the same
        // unlimited MiniMax tier as `minimax:MiniMax-M3`, but only the
        // colon spelling was exempt, so Lite's own default routes were
        // silently capped at 5M.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        assert_eq!(token_cap_for(Some("minimax/M3")), 0);
        assert_eq!(token_cap_for(Some("minimax/M2.7-highspeed")), 0);
    }

    #[test]
    fn provider_routed_minimax_stays_capped() {
        // MiniMax served through a metered third-party quota must stay capped;
        // the exemption is about MiniMax's own unlimited tier, not the name.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        assert_eq!(
            token_cap_for(Some("openrouter/minimax-m2.1")),
            DEFAULT_MAX_EFFECTIVE_TOKENS
        );
    }

    #[test]
    fn non_minimax_model_defaults_to_five_million_token_budget() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        let budget = WorkerBudget::from_env_for_model(Some("gpt-5.6-sol"));
        assert_eq!(budget.max_effective_tokens, DEFAULT_MAX_EFFECTIVE_TOKENS);
    }

    #[test]
    fn unknown_model_defaults_to_five_million_token_budget() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_budget_env();
        let budget = WorkerBudget::from_env_for_model(None);
        assert_eq!(budget.max_effective_tokens, DEFAULT_MAX_EFFECTIVE_TOKENS);
    }

    #[test]
    fn env_token_cap_overrides_minimax_default() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::set_var(MAX_TOKENS_ENV, "12345") };
        let budget = WorkerBudget::from_env_for_model(Some("minimax:MiniMax-M3"));
        assert_eq!(budget.max_effective_tokens, 12345);
        unsafe { std::env::remove_var(MAX_TOKENS_ENV) };
    }

    #[test]
    fn env_token_cap_zero_disables_cap_for_any_model() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        unsafe { std::env::set_var(MAX_TOKENS_ENV, "0") };
        let budget = WorkerBudget::from_env_for_model(Some("gpt-5.6-sol"));
        assert_eq!(budget.max_effective_tokens, 0);
        unsafe { std::env::remove_var(MAX_TOKENS_ENV) };
    }

    #[tokio::test]
    async fn runtime_budget_cancels_a_hung_worker() {
        let result = run_with_budget(
            "budget-runtime-fixture",
            std::future::pending(),
            WorkerBudget {
                max_effective_tokens: 0,
                max_runtime: Duration::from_millis(20),
                max_inactivity: Duration::ZERO,
                poll_interval: Duration::from_millis(2),
            },
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.to_string().contains("runtime budget exceeded"));
        assert!(retryable_budget_error(&error));
    }

    #[tokio::test]
    async fn inactivity_budget_cancels_a_worker_with_no_productive_activity() {
        let session_id = "budget-inactivity-fixture";
        crate::session_metrics::forget(session_id);
        let result = run_with_budget(
            session_id,
            std::future::pending(),
            WorkerBudget {
                max_effective_tokens: 0,
                max_runtime: Duration::ZERO,
                max_inactivity: Duration::from_millis(20),
                poll_interval: Duration::from_millis(2),
            },
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.to_string().contains("inactivity budget exceeded"));
        assert!(retryable_budget_error(&error));
        crate::session_metrics::forget(session_id);
    }

    #[tokio::test]
    async fn productive_worker_can_run_past_the_old_wall_clock_budget() {
        let session_id = "budget-active-fixture";
        crate::session_metrics::forget(session_id);
        let future = async move {
            for _ in 0..8 {
                tokio::time::sleep(Duration::from_millis(8)).await;
                crate::session_metrics::record_activity(session_id);
            }
            Ok(())
        };
        let result = run_with_budget(
            session_id,
            future,
            WorkerBudget {
                max_effective_tokens: 0,
                max_runtime: Duration::ZERO,
                max_inactivity: Duration::from_millis(20),
                poll_interval: Duration::from_millis(2),
            },
        )
        .await;
        assert!(result.is_ok());
        crate::session_metrics::forget(session_id);
    }

    #[tokio::test]
    async fn token_budget_uses_incremental_effective_session_usage() {
        let session_id = "budget-token-fixture";
        crate::session_metrics::record_token_usage(session_id, 90, 10);
        let future = async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            crate::session_metrics::record_token_usage(session_id, 110, 10);
            std::future::pending::<anyhow::Result<()>>().await
        };
        let result = run_with_budget(
            session_id,
            future,
            WorkerBudget {
                max_effective_tokens: 100,
                max_runtime: Duration::from_secs(1),
                max_inactivity: Duration::ZERO,
                poll_interval: Duration::from_millis(2),
            },
        )
        .await;
        let error = result.unwrap_err();
        assert!(error.to_string().contains("token budget exceeded"));
        assert!(!retryable_budget_error(&error));
        crate::session_metrics::forget(session_id);
    }

    #[tokio::test]
    async fn completed_worker_returns_before_budget() {
        let result = run_with_budget(
            "budget-complete-fixture",
            async { Ok(()) },
            WorkerBudget {
                max_effective_tokens: 1,
                max_runtime: Duration::from_secs(1),
                max_inactivity: Duration::ZERO,
                poll_interval: Duration::from_millis(2),
            },
        )
        .await;
        assert!(result.is_ok());
    }
}
