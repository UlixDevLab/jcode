//! Shared concurrent Anthropic OAuth subscription monitor.
//!
//! This monitor deliberately holds its cross-process lock only while reading or
//! writing durable state and appending credential-free telemetry. Network calls
//! and model streaming happen outside the lock, so independent sessions can use
//! supported OAuth models concurrently. A suspicious five-hour movement pauses
//! *future* calls for operator approval without discarding the call that
//! observed it.

use crate::config::ProviderConfig;
use crate::usage::USAGE_URL;
use crate::usage::openai_helpers::usage_percent_to_ratio;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
type PlMutex<T> = Mutex<T>;

/// Schema 1 was the exclusive, fail-closed kill guard. Schema 2 is a
/// concurrent monitor with explicit operator approval.
pub const STATE_SCHEMA_VERSION: u32 = 2;
/// Kept public for source compatibility. An empty configured allowlist now
/// admits the supported `claude-*` OAuth family (except Fable).
pub const SAFE_DEFAULT_ALLOWED_MODELS: &[&str] = &["claude-*"];
const POST_FETCH_TIMEOUT: Duration = Duration::from_secs(8);
const POST_FETCH_INITIAL_BACKOFF: Duration = Duration::from_millis(50);
const POST_FETCH_MAX_BACKOFF: Duration = Duration::from_millis(800);

/// How long one usage-meter reading stays reusable.
///
/// Anthropic's usage endpoint reports five-hour and seven-day rolling windows,
/// so a reading is not meaningfully staler at 30s than at 0s. Before this, every
/// guarded call fetched twice (preflight + post-call) and `fetch_post_meter`
/// retried for up to `POST_FETCH_TIMEOUT` on failure, so a single busy session
/// could fire a dozen or more requests. The endpoint rate-limited us in
/// response, both fetches then returned `None`, and the guard ran blind while
/// warning on every call. Serving reads from a short cache keeps the guard
/// metered instead.
const METER_CACHE_TTL: Duration = Duration::from_secs(30);

/// How long a 429 suppresses further fetches.
///
/// Retrying into an active rate limit is what produced the limit. When the
/// endpoint says "too many requests", the only useful response is to stop
/// asking for a while.
const METER_RATE_LIMIT_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub enum AdmissionError {
    Disabled,
    OperatorDisabled,
    ApprovalRequired {
        reason: String,
    },
    DisallowedModel {
        model: String,
    },
    /// Compatibility spelling for callers built against the old kill guard.
    /// New code receives `ApprovalRequired` for all durable pauses.
    Killed {
        killed_at_unix: u64,
        reason: String,
    },
    PreflightExceeded {
        percent: f32,
        max_percent: f32,
    },
    PreflightUnverified(String),
    StateUnavailable(String),
}

impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "anthropic subscription monitor is disabled by config"),
            Self::OperatorDisabled => write!(
                f,
                "anthropic OAuth monitor is globally off; ask the operator to run `jcode claude on`"
            ),
            Self::ApprovalRequired { reason } => write!(
                f,
                "anthropic OAuth monitor requires operator approval; ask the operator to run `jcode claude approve` ({reason})"
            ),
            Self::DisallowedModel { model } => {
                write!(f, "anthropic OAuth monitor does not allow model {model:?}")
            }
            Self::Killed {
                killed_at_unix,
                reason,
            } => write!(
                f,
                "anthropic OAuth monitor paused at {killed_at_unix}; {reason}"
            ),
            Self::PreflightExceeded {
                percent,
                max_percent,
            } => write!(
                f,
                "anthropic OAuth monitor requires approval: preflight utilization {percent:.2}% >= max {max_percent:.2}%"
            ),
            Self::PreflightUnverified(msg) => write!(
                f,
                "anthropic OAuth monitor could not verify preflight usage: {msg}"
            ),
            Self::StateUnavailable(msg) => {
                write!(f, "anthropic OAuth monitor state unavailable: {msg}")
            }
        }
    }
}
impl std::error::Error for AdmissionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishOutcome {
    WithinBudget,
    /// Compatibility name: the response completed, but future calls are paused
    /// pending explicit operator approval instead of permanently killed.
    Killed,
    UnverifiedMeter,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireOutcome {
    Disabled,
    Admitted,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionTelemetry {
    #[serde(default)]
    pub started_calls: u64,
    #[serde(default)]
    pub finished_calls: u64,
    #[serde(default)]
    pub meter_failures: u64,
    #[serde(default)]
    pub last_interval_delta_pct: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActiveCall {
    pub model: String,
    pub session_id: Option<String>,
    pub started_unix_ms: u64,
    pub pre_pct: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuardState {
    pub version: u32,
    #[serde(default = "default_operator_enabled")]
    pub operator_enabled: bool,
    #[serde(default)]
    pub approval_pending: bool,
    #[serde(default)]
    pub window_reset_id: String,
    #[serde(default)]
    pub window_resets_at: Option<String>,
    #[serde(default)]
    pub window_baseline_pct: Option<f32>,
    #[serde(default)]
    pub latest_five_hour_pct: Option<f32>,
    #[serde(default)]
    pub peak_five_hour_pct: Option<f32>,
    #[serde(default)]
    pub aggregate_observed_movement_pct: f32,
    #[serde(default)]
    pub active_calls: BTreeMap<String, ActiveCall>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionTelemetry>,
    #[serde(default)]
    pub last_warning: Option<String>,
    // Compatibility/readability fields retained from schema 1. They are
    // telemetry only and never enforce a durable kill.
    #[serde(default)]
    pub pre_pct: Option<f32>,
    #[serde(default)]
    pub post_pct: Option<f32>,
    #[serde(default)]
    pub last_single_delta_pct: Option<f32>,
    #[serde(default)]
    pub cumulative_delta_pct: f32,
    #[serde(default)]
    pub peak_cumulative_delta_pct: f32,
    #[serde(default)]
    pub in_flight: Option<InFlight>,
    #[serde(default)]
    pub kill: Option<KillRecord>,
}
fn default_operator_enabled() -> bool {
    true
}
impl Default for GuardState {
    fn default() -> Self {
        Self {
            version: STATE_SCHEMA_VERSION,
            operator_enabled: true,
            approval_pending: false,
            window_reset_id: String::new(),
            window_resets_at: None,
            window_baseline_pct: None,
            latest_five_hour_pct: None,
            peak_five_hour_pct: None,
            aggregate_observed_movement_pct: 0.0,
            active_calls: BTreeMap::new(),
            sessions: BTreeMap::new(),
            last_warning: None,
            pre_pct: None,
            post_pct: None,
            last_single_delta_pct: None,
            cumulative_delta_pct: 0.0,
            peak_cumulative_delta_pct: 0.0,
            in_flight: None,
            kill: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InFlight {
    pub model: String,
    pub call_id: String,
    pub started_unix_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KillRecord {
    pub reason: String,
    pub killed_at_unix_ms: u64,
    pub pre_pct: f32,
    pub post_pct: f32,
    pub single_delta_pct: f32,
    pub cumulative_delta_pct: f32,
    pub window_resets_at: Option<String>,
}

pub struct Lease {
    pub model: String,
    pub call_id: String,
    pub pre_pct: Option<f32>,
    pub window_resets_at: Option<String>,
    pub window_reset_id: String,
    pub config: Arc<GuardConfig>,
    pub state_path: PathBuf,
    pub session_id: Option<String>,
}
impl std::fmt::Debug for Lease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lease")
            .field("model", &self.model)
            .field("call_id", &self.call_id)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct GuardConfig {
    pub enabled: bool,
    pub threshold_pct: f32,
    pub max_preflight_pct: f32,
    pub fail_closed: bool,
    pub allowed_models: Vec<String>,
}
impl GuardConfig {
    pub fn from_provider_config(cfg: &ProviderConfig) -> Arc<Self> {
        Arc::new(Self {
            enabled: cfg.anthropic_subscription_guard_enabled,
            threshold_pct: cfg.anthropic_subscription_guard_threshold_percent,
            max_preflight_pct: cfg
                .anthropic_subscription_guard_max_preflight_percent
                .clamp(0.0, 100.0),
            fail_closed: cfg.anthropic_subscription_guard_fail_closed,
            allowed_models: cfg
                .anthropic_subscription_guard_allowed_models
                .iter()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect(),
        })
    }
    pub fn model_allowed(&self, model: &str) -> bool {
        let model = model.trim();
        if !model.starts_with("claude-") {
            return false;
        }
        // Fable is expensive enough that it must never be admitted by a
        // wildcard. It requires its own exact entry in the allowlist, so
        // neither an empty list nor `claude-*` can select it by accident.
        if model.to_ascii_lowercase().contains("fable") {
            return self.allowed_models.iter().any(|allowed| allowed == model);
        }
        self.allowed_models.is_empty()
            || self
                .allowed_models
                .iter()
                .any(|allowed| allowed == model || allowed == "claude-*")
    }
    pub fn exceeds_threshold(&self, delta_pct: f32) -> bool {
        delta_pct.abs() >= self.threshold_pct
    }
}

pub fn guard_dir() -> Result<PathBuf> {
    let dir = crate::storage::jcode_dir()
        .context("jcode home directory unavailable")?
        .join("anthropic_subscription_guard");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create monitor dir at {}", dir.display()))?;
    Ok(dir)
}
pub(crate) fn state_path() -> Result<PathBuf> {
    Ok(guard_dir()?.join("state.json"))
}
pub(crate) fn lock_path() -> Result<PathBuf> {
    Ok(guard_dir()?.join("state.lock"))
}
fn event_path() -> Result<PathBuf> {
    Ok(guard_dir()?.join("events.jsonl"))
}

pub struct CrossProcessLock {
    _file: std::fs::File,
    _path: PathBuf,
}
impl CrossProcessLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(path)
                .with_context(|| format!("open lock file {}", path.display()))?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            use std::os::unix::io::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(anyhow!("flock failed: {}", std::io::Error::last_os_error()));
            }
            Ok(Self {
                _file: file,
                _path: path.to_path_buf(),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            loop {
                match OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .share_mode(0)
                    .open(path)
                {
                    Ok(file) => {
                        return Ok(Self {
                            _file: file,
                            _path: path.to_path_buf(),
                        });
                    }
                    Err(err)
                        if err.kind() == std::io::ErrorKind::PermissionDenied
                            || matches!(err.raw_os_error(), Some(32 | 33)) =>
                    {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(err) => return Err(anyhow!("exclusive Windows lock open failed: {err}")),
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(anyhow!(
                "Anthropic monitor has no cross-process lock for this platform"
            ))
        }
    }
}
impl Drop for CrossProcessLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe {
                libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

/// Read and safely migrate schema-1 state. Schema-1 kills become an approval
/// pause so no prior usage decision silently turns into a permanent ban.
pub(crate) fn read_state(path: &Path) -> Result<GuardState> {
    if !path.exists() {
        return Ok(GuardState::default());
    }
    let raw =
        std::fs::read(path).with_context(|| format!("read monitor state at {}", path.display()))?;
    if raw.is_empty() {
        return Ok(GuardState::default());
    }
    let value: serde_json::Value = serde_json::from_slice(&raw).context("parse monitor state")?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    if version == STATE_SCHEMA_VERSION as u64 {
        return serde_json::from_value(value).context("decode monitor state");
    }
    if version != 1 {
        return Err(anyhow!("unsupported monitor state version {version}"));
    }
    let mut migrated = GuardState::default();
    migrated.window_reset_id = value
        .get("window_reset_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    migrated.window_resets_at = value
        .get("window_resets_at")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    migrated.window_baseline_pct = value
        .get("window_baseline_pct")
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32);
    migrated.latest_five_hour_pct = value
        .get("post_pct")
        .and_then(serde_json::Value::as_f64)
        .or_else(|| value.get("pre_pct").and_then(serde_json::Value::as_f64))
        .map(|v| v as f32);
    migrated.peak_five_hour_pct = migrated.latest_five_hour_pct;
    migrated.aggregate_observed_movement_pct = value
        .get("cumulative_delta_pct")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0) as f32;
    migrated.cumulative_delta_pct = migrated.aggregate_observed_movement_pct;
    migrated.peak_cumulative_delta_pct = value
        .get("peak_cumulative_delta_pct")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(migrated.aggregate_observed_movement_pct as f64)
        as f32;
    if value.get("kill").is_some_and(|kill| !kill.is_null()) {
        migrated.approval_pending = true;
        migrated.last_warning =
            Some("migrated schema-1 kill requires operator approval".to_string());
    }
    Ok(migrated)
}
pub(crate) fn write_state(path: &Path, state: &GuardState) -> Result<()> {
    crate::storage::write_json_secret(path, state).context("persist Claude monitor state")
}

#[derive(Serialize)]
struct MonitorEvent<'a> {
    kind: &'a str,
    at_unix_ms: u64,
    call_id: Option<&'a str>,
    model: Option<&'a str>,
    session_id: Option<&'a str>,
    percent: Option<f32>,
    movement_pct: f32,
    approval_pending: bool,
    operator_enabled: bool,
}
fn append_event(
    state: &GuardState,
    kind: &str,
    call_id: Option<&str>,
    model: Option<&str>,
    session_id: Option<&str>,
    percent: Option<f32>,
) -> Result<()> {
    let path = event_path()?;
    let event = MonitorEvent {
        kind,
        at_unix_ms: unix_now_millis(),
        call_id,
        model,
        session_id,
        percent,
        movement_pct: state.aggregate_observed_movement_pct,
        approval_pending: state.approval_pending,
        operator_enabled: state.operator_enabled,
    };
    let line = serde_json::to_vec(&event)?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(path)?;
    f.write_all(&line)?;
    f.write_all(b"\n")?;
    Ok(())
}

pub(crate) fn reconcile_window_reset(state: &mut GuardState, new_reset_id: &str) -> bool {
    if !state.window_reset_id.is_empty() && state.window_reset_id != new_reset_id {
        state.approval_pending = false;
        state.window_baseline_pct = None;
        state.latest_five_hour_pct = None;
        state.peak_five_hour_pct = None;
        state.aggregate_observed_movement_pct = 0.0;
        state.cumulative_delta_pct = 0.0;
        state.peak_cumulative_delta_pct = 0.0;
        state.pre_pct = None;
        state.post_pct = None;
        state.last_single_delta_pct = None;
        state.last_warning = None;
        return true;
    }
    false
}
pub(crate) fn unix_now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[derive(Debug, Clone)]
pub struct UsageMeterSnapshot {
    pub five_hour_pct: f32,
    pub five_hour_resets_at: Option<String>,
}

pub(crate) fn apply_sample(
    state: &mut GuardState,
    meter: &UsageMeterSnapshot,
    config: &GuardConfig,
) -> bool {
    let reset_id = meter.five_hour_resets_at.clone().unwrap_or_default();
    let reset = reconcile_window_reset(state, &reset_id);
    state.window_reset_id = reset_id;
    state.window_resets_at = meter.five_hour_resets_at.clone();
    let pct = meter.five_hour_pct.max(0.0);
    let baseline = *state.window_baseline_pct.get_or_insert(pct);
    // Concurrent overlapping calls may observe samples out of order. Only the
    // maximum utilization advances the global sample, so movement counts once.
    let latest = state.latest_five_hour_pct.unwrap_or(pct).max(pct);
    state.latest_five_hour_pct = Some(latest);
    state.peak_five_hour_pct = Some(state.peak_five_hour_pct.unwrap_or(latest).max(latest));
    state.aggregate_observed_movement_pct = (latest - baseline).max(0.0);
    state.cumulative_delta_pct = state.aggregate_observed_movement_pct;
    state.peak_cumulative_delta_pct = state
        .peak_cumulative_delta_pct
        .max(state.aggregate_observed_movement_pct);
    if config.exceeds_threshold(state.aggregate_observed_movement_pct) {
        state.approval_pending = true;
        state.last_warning = Some(format!(
            "five-hour movement {:.2}pp reached {:.2}pp threshold",
            state.aggregate_observed_movement_pct, config.threshold_pct
        ));
    }
    reset
}
fn session_key(session: Option<&str>) -> String {
    session.unwrap_or("unknown-session").to_string()
}

pub async fn acquire_anthropic_subscription_guard(
    token: &str,
    model: &str,
    call_id: &str,
) -> Result<Lease, AdmissionError> {
    let cfg = resolve_runtime_config();
    if !cfg.enabled {
        return Err(AdmissionError::Disabled);
    }
    if !cfg.model_allowed(model) {
        return Err(AdmissionError::DisallowedModel {
            model: model.to_string(),
        });
    }
    let state_file = state_path().map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
    let lock_file = lock_path().map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;

    // `off` is an immediate local control. Check it before contacting the usage
    // endpoint so globally disabled calls produce no Anthropic network traffic.
    // Approval-pending calls still sample below because a new five-hour window
    // can clear the pause automatically.
    let precheck_state = state_file.clone();
    let precheck_lock = lock_file.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AdmissionError> {
        let _lock = CrossProcessLock::acquire(&precheck_lock)
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        let state = read_state(&precheck_state)
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        if state.operator_enabled {
            Ok(())
        } else {
            Err(AdmissionError::OperatorDisabled)
        }
    })
    .await
    .map_err(|e| AdmissionError::StateUnavailable(format!("monitor precheck join error: {e}")))??;

    // Fetch outside the OS lock. A meter failure is telemetry, not a durable kill.
    // Served from a short cache: preflight and post-call previously issued two
    // separate requests per turn, which is what rate-limited the endpoint and
    // left the guard unmetered.
    let meter = fetch_meter_cached(token).await;
    let model = model.to_string();
    let call_id = call_id.to_string();
    let session_id = crate::logging::current_session();
    tokio::task::spawn_blocking(move || -> Result<Lease, AdmissionError> {
        let _lock = CrossProcessLock::acquire(&lock_file)
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        let mut state =
            read_state(&state_file).map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        if let Some(meter) = meter.as_ref() {
            apply_sample(&mut state, meter, &cfg);
            append_event(
                &state,
                "sample",
                Some(&call_id),
                Some(&model),
                session_id.as_deref(),
                Some(meter.five_hour_pct),
            )
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        } else {
            state.last_warning = Some(
                "usage meter unavailable before OAuth call; call allowed and telemetry recorded"
                    .to_string(),
            );
            let telemetry = state
                .sessions
                .entry(session_key(session_id.as_deref()))
                .or_default();
            telemetry.meter_failures += 1;
            append_event(
                &state,
                "warn",
                Some(&call_id),
                Some(&model),
                session_id.as_deref(),
                None,
            )
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        }
        if !state.operator_enabled {
            write_state(&state_file, &state)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            return Err(AdmissionError::OperatorDisabled);
        }
        if state.approval_pending {
            let reason = state
                .last_warning
                .clone()
                .unwrap_or_else(|| "approval pending".to_string());
            write_state(&state_file, &state)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            return Err(AdmissionError::ApprovalRequired { reason });
        }
        if let Some(meter) = meter.as_ref()
            && meter.five_hour_pct >= cfg.max_preflight_pct
        {
            state.approval_pending = true;
            state.last_warning = Some(format!(
                "preflight utilization {:.2}% reached {:.2}% maximum",
                meter.five_hour_pct, cfg.max_preflight_pct
            ));
            append_event(
                &state,
                "warn",
                Some(&call_id),
                Some(&model),
                session_id.as_deref(),
                Some(meter.five_hour_pct),
            )
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            write_state(&state_file, &state)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            return Err(AdmissionError::ApprovalRequired {
                reason: state.last_warning.clone().unwrap_or_default(),
            });
        }
        let pre_pct = meter.as_ref().map(|m| m.five_hour_pct);
        state.pre_pct = pre_pct;
        state.post_pct = None;
        state.last_single_delta_pct = None;
        state.active_calls.insert(
            call_id.clone(),
            ActiveCall {
                model: model.clone(),
                session_id: session_id.clone(),
                started_unix_ms: unix_now_millis(),
                pre_pct,
            },
        );
        state
            .sessions
            .entry(session_key(session_id.as_deref()))
            .or_default()
            .started_calls += 1;
        append_event(
            &state,
            "start",
            Some(&call_id),
            Some(&model),
            session_id.as_deref(),
            pre_pct,
        )
        .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        write_state(&state_file, &state)
            .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        Ok(Lease {
            model,
            call_id,
            pre_pct,
            window_resets_at: state.window_resets_at.clone(),
            window_reset_id: state.window_reset_id.clone(),
            config: cfg,
            state_path: state_file,
            session_id,
        })
    })
    .await
    .map_err(|e| AdmissionError::StateUnavailable(format!("monitor lock join error: {e}")))?
}

pub async fn try_acquire(
    token: &str,
    model: &str,
    call_id: &str,
) -> (AcquireOutcome, Option<Lease>) {
    match acquire_anthropic_subscription_guard(token, model, call_id).await {
        Err(AdmissionError::Disabled) => (AcquireOutcome::Disabled, None),
        Err(_) => (AcquireOutcome::Denied, None),
        Ok(lease) => (AcquireOutcome::Admitted, Some(lease)),
    }
}

pub type TestFetcher = Arc<
    dyn Fn(
            &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<UsageMeterSnapshot>> + Send>,
        > + Send
        + Sync,
>;
static TEST_FETCHER: OnceLock<PlMutex<Option<TestFetcher>>> = OnceLock::new();
fn test_fetcher_cell() -> &'static PlMutex<Option<TestFetcher>> {
    TEST_FETCHER.get_or_init(|| PlMutex::new(None))
}
pub fn set_test_fetcher(fetcher: Option<TestFetcher>) -> Option<TestFetcher> {
    let mut slot = test_fetcher_cell()
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    std::mem::replace(&mut *slot, fetcher)
}
pub async fn fetch_anthropic_usage_uncached(token: &str) -> Result<UsageMeterSnapshot> {
    let fetcher = test_fetcher_cell()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(f) = fetcher {
        return f(token).await;
    }
    let client = crate::provider::shared_http_client();
    let response = crate::provider::anthropic::apply_oauth_attribution_headers(
        client
            .get(USAGE_URL)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header(
                "User-Agent",
                crate::provider::anthropic::CLAUDE_CLI_USER_AGENT,
            )
            .header("Authorization", format!("Bearer {token}"))
            .header("anthropic-beta", "oauth-2025-04-20,claude-code-20250219"),
        &crate::provider::anthropic::new_oauth_request_id(),
    )
    .send()
    .await
    .context("anthropic usage fetch failed")?;
    if !response.status().is_success() {
        return Err(anyhow!("usage api returned {}", response.status()));
    }
    let parsed: serde_json::Value = response.json().await.context("parse usage json")?;
    let five_hour_pct = parsed
        .get("five_hour")
        .and_then(|w| w.get("utilization"))
        .and_then(serde_json::Value::as_f64)
        .map(|v| usage_percent_to_ratio(v as f32) * 100.0)
        .ok_or_else(|| anyhow!("missing five_hour.utilization"))?;
    Ok(UsageMeterSnapshot {
        five_hour_pct,
        five_hour_resets_at: parsed
            .get("five_hour")
            .and_then(|w| w.get("resets_at"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

impl Lease {
    pub async fn finish(self, token: &str) -> Result<FinishOutcome, AdmissionError> {
        // Prefer a fresh post-call reading, but fall back to the cached one
        // rather than reporting the meter unverified. Reporting "unverified"
        // when we simply declined to re-ask a rate-limited endpoint trained the
        // operator to ignore a warning that is supposed to mean something.
        let meter = match fetch_post_meter(token).await {
            Ok(meter) => Some(meter),
            Err(_) => fetch_meter_cached(token).await,
        };
        let state_file = self.state_path.clone();
        let lock_file = lock_path().map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
        tokio::task::spawn_blocking(move || -> Result<FinishOutcome, AdmissionError> {
            let _lock = CrossProcessLock::acquire(&lock_file)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            let mut state = read_state(&state_file)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            let active = state.active_calls.remove(&self.call_id);
            let session = active
                .as_ref()
                .and_then(|a| a.session_id.as_deref())
                .or(self.session_id.as_deref());
            let telemetry = state.sessions.entry(session_key(session)).or_default();
            telemetry.finished_calls += 1;
            let outcome = if let Some(meter) = meter.as_ref() {
                let delta = self.pre_pct.map(|pre| (meter.five_hour_pct - pre).max(0.0));
                state.pre_pct = self.pre_pct;
                state.post_pct = Some(meter.five_hour_pct);
                state.last_single_delta_pct = delta;
                telemetry.last_interval_delta_pct = delta;
                apply_sample(&mut state, meter, &self.config);
                append_event(
                    &state,
                    "sample",
                    Some(&self.call_id),
                    Some(&self.model),
                    session,
                    Some(meter.five_hour_pct),
                )
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
                append_event(
                    &state,
                    "finish",
                    Some(&self.call_id),
                    Some(&self.model),
                    session,
                    Some(meter.five_hour_pct),
                )
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
                if state.approval_pending {
                    FinishOutcome::Killed
                } else {
                    FinishOutcome::WithinBudget
                }
            } else {
                telemetry.meter_failures += 1;
                state.last_warning = Some(
                    "usage meter unavailable after OAuth call; no durable pause created"
                        .to_string(),
                );
                append_event(
                    &state,
                    "warn",
                    Some(&self.call_id),
                    Some(&self.model),
                    session,
                    None,
                )
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
                append_event(
                    &state,
                    "finish",
                    Some(&self.call_id),
                    Some(&self.model),
                    session,
                    None,
                )
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
                FinishOutcome::UnverifiedMeter
            };
            write_state(&state_file, &state)
                .map_err(|e| AdmissionError::StateUnavailable(e.to_string()))?;
            Ok(outcome)
        })
        .await
        .map_err(|e| AdmissionError::StateUnavailable(format!("monitor lock join error: {e}")))?
    }
}

/// Compatibility pure helper. It applies a post-call sample, with per-call
/// deltas marked as non-additive telemetry when calls overlap.
pub fn apply_finish_decision(
    state: &mut GuardState,
    pre_pct: f32,
    post_pct: f32,
    config: &GuardConfig,
) -> FinishOutcome {
    let meter = UsageMeterSnapshot {
        five_hour_pct: post_pct,
        five_hour_resets_at: state.window_resets_at.clone(),
    };
    state.pre_pct = Some(pre_pct);
    state.post_pct = Some(post_pct);
    state.last_single_delta_pct = Some((post_pct - pre_pct).max(0.0));
    apply_sample(state, &meter, config);
    if state.approval_pending {
        FinishOutcome::Killed
    } else {
        FinishOutcome::WithinBudget
    }
}
pub async fn sleep_for_retry(attempt: u32) {
    let delay = POST_FETCH_INITIAL_BACKOFF
        .checked_mul(1u32 << attempt.min(6))
        .unwrap_or(POST_FETCH_MAX_BACKOFF)
        .min(POST_FETCH_MAX_BACKOFF);
    tokio::time::sleep(delay).await;
}
pub async fn fetch_post_meter(token: &str) -> Result<UsageMeterSnapshot> {
    let deadline = Instant::now() + POST_FETCH_TIMEOUT;
    let mut attempt = 0;
    loop {
        match fetch_anthropic_usage_uncached(token).await {
            Ok(m) => {
                let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
                cache.snapshot = Some((Instant::now(), m.clone()));
                cache.blocked_until = None;
                return Ok(m);
            }
            // A rate limit is not a transient hiccup: retrying is what caused
            // it. Stop immediately, record the backoff so other call sites stop
            // too, and let the caller fall back to the cached reading.
            Err(e) if is_usage_rate_limited(&e) => {
                let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
                cache.blocked_until = Some(Instant::now() + METER_RATE_LIMIT_BACKOFF);
                return Err(e);
            }
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {
                sleep_for_retry(attempt).await;
                attempt += 1;
            }
        }
    }
}

/// Last successful reading, and the time after which fetching is allowed again.
///
/// Shared by the preflight and post-call paths so one turn costs at most one
/// request instead of two-plus.
struct MeterCache {
    snapshot: Option<(Instant, UsageMeterSnapshot)>,
    blocked_until: Option<Instant>,
}

static METER_CACHE: OnceLock<PlMutex<MeterCache>> = OnceLock::new();

fn meter_cache() -> &'static PlMutex<MeterCache> {
    METER_CACHE.get_or_init(|| {
        PlMutex::new(MeterCache {
            snapshot: None,
            blocked_until: None,
        })
    })
}

/// Whether `error` is the usage endpoint refusing us for rate reasons.
fn is_usage_rate_limited(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_ascii_lowercase();
    text.contains("429") || text.contains("too many requests") || text.contains("rate limit")
}

/// Read the usage meter, reusing a recent reading and honoring a rate-limit
/// backoff.
///
/// Returns `None` only when there is genuinely nothing usable: no fresh
/// reading, no cached reading, and either a live failure or an active backoff.
/// Callers treat `None` as "unmetered", so keeping a slightly stale reading is
/// strictly better than discarding it.
pub async fn fetch_meter_cached(token: &str) -> Option<UsageMeterSnapshot> {
    let now = Instant::now();
    {
        let cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, snapshot)) = cache.snapshot.as_ref()
            && now.duration_since(*at) < METER_CACHE_TTL
        {
            return Some(snapshot.clone());
        }
        // Inside a rate-limit backoff, serve the last reading rather than
        // spending another request to be told "no" again.
        if cache.blocked_until.is_some_and(|until| now < until) {
            return cache.snapshot.as_ref().map(|(_, s)| s.clone());
        }
    }

    match fetch_anthropic_usage_uncached(token).await {
        Ok(snapshot) => {
            let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
            cache.snapshot = Some((Instant::now(), snapshot.clone()));
            cache.blocked_until = None;
            Some(snapshot)
        }
        Err(error) => {
            let rate_limited = is_usage_rate_limited(&error);
            let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
            if rate_limited {
                cache.blocked_until = Some(Instant::now() + METER_RATE_LIMIT_BACKOFF);
                crate::logging::warn(&format!(
                    "anthropic usage meter rate-limited; pausing meter fetches for {}s: {error:#}",
                    METER_RATE_LIMIT_BACKOFF.as_secs()
                ));
            }
            cache.snapshot.as_ref().map(|(_, s)| s.clone())
        }
    }
}

/// Backdate the cached reading so a test can exercise the post-TTL path
/// without sleeping.
#[cfg(test)]
pub(crate) fn expire_meter_cache_for_tests() {
    let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at, _)) = cache.snapshot.as_mut() {
        *at = Instant::now() - (METER_CACHE_TTL + Duration::from_secs(1));
    }
}

/// The post-call retry budget, exposed so a test can assert we return well
/// inside it rather than looping.
#[cfg(test)]
pub(crate) const fn post_fetch_timeout_for_tests() -> Duration {
    POST_FETCH_TIMEOUT
}

#[cfg(test)]
pub(crate) fn reset_meter_cache_for_tests() {
    let mut cache = meter_cache().lock().unwrap_or_else(|p| p.into_inner());
    cache.snapshot = None;
    cache.blocked_until = None;
}
pub fn resolve_runtime_config() -> Arc<GuardConfig> {
    if let Some(config) = test_config_override_cell()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        return config;
    }
    let cfg = crate::config::Config::load();
    GuardConfig::from_provider_config(&cfg.provider)
}
static TEST_CONFIG_OVERRIDE: OnceLock<PlMutex<Option<Arc<GuardConfig>>>> = OnceLock::new();
fn test_config_override_cell() -> &'static PlMutex<Option<Arc<GuardConfig>>> {
    TEST_CONFIG_OVERRIDE.get_or_init(|| PlMutex::new(None))
}
pub fn set_test_guard_config_override(cfg: Option<Arc<GuardConfig>>) {
    *test_config_override_cell()
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = cfg;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeMonitorCommand {
    On,
    Off,
    Approve,
}
pub fn claude_monitor_status() -> Result<GuardState> {
    let state_file = state_path()?;
    let lock_file = lock_path()?;
    let _lock = CrossProcessLock::acquire(&lock_file)?;
    read_state(&state_file)
}
pub fn control_claude_monitor(command: ClaudeMonitorCommand) -> Result<GuardState> {
    let state_file = state_path()?;
    let lock_file = lock_path()?;
    let _lock = CrossProcessLock::acquire(&lock_file)?;
    let mut state = read_state(&state_file)?;
    match command {
        ClaudeMonitorCommand::On => {
            state.operator_enabled = true;
            append_event(&state, "on", None, None, None, None)?;
        }
        ClaudeMonitorCommand::Off => {
            state.operator_enabled = false;
            append_event(&state, "off", None, None, None, None)?;
        }
        ClaudeMonitorCommand::Approve => {
            state.approval_pending = false;
            state.window_baseline_pct = state.latest_five_hour_pct;
            state.aggregate_observed_movement_pct = 0.0;
            state.cumulative_delta_pct = 0.0;
            state.peak_cumulative_delta_pct = 0.0;
            state.last_warning = None;
            append_event(
                &state,
                "approve",
                None,
                None,
                None,
                state.latest_five_hour_pct,
            )?;
        }
    }
    write_state(&state_file, &state)?;
    Ok(state)
}
