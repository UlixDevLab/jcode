//! Best-effort metadata accounting at the MultiProvider dispatch boundary.
//! No transcript, credentials or errors are serialized. Telemetry never gates work.
use crate::message::StreamEvent;
use crate::provider::{EventStream, Provider};
use chrono::Utc;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{OnceLock, mpsc};
use std::task::{Context, Poll};
use std::time::Instant;

pub mod transport;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageContext {
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub working_dir: Option<String>,
    pub origin: Option<String>,
}

tokio::task_local! { static CONTEXT: UsageContext; }

pub async fn scope<T>(context: UsageContext, future: impl Future<Output = T>) -> T {
    CONTEXT.scope(context, future).await
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageTokens {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageEvent {
    pub schema: &'static str,
    pub event_id: String,
    pub request_id: String,
    pub segment: u32,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    #[serde(flatten)]
    pub context: UsageContext,
    pub provider: String,
    pub runtime: String,
    pub model_at_dispatch: String,
    pub model_actual: Option<String>,
    pub account_ref: Option<String>,
    pub account_aliases: Vec<String>,
    pub billing_route: Option<String>,
    pub transport_observations: Vec<transport::TransportObservation>,
    pub auth_method_at_dispatch: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub upstream_request_id: Option<String>,
    pub status: String,
    pub usage_present: bool,
    pub tokens: UsageTokens,
    pub attempt_coverage: &'static str,
}

struct Call {
    event: UsageEvent,
    started: Instant,
    destination: Option<PathBuf>,
    finalized: bool,
    observations: transport::Observations,
}

impl Call {
    fn new(provider: &dyn Provider) -> Self {
        let request_id = uuid::Uuid::new_v4().to_string();
        Self {
            event: UsageEvent {
                schema: "jcode-usage-event/v1",
                event_id: format!("{request_id}:0"),
                request_id,
                segment: 0,
                started_at: Utc::now().to_rfc3339(),
                completed_at: String::new(),
                duration_ms: 0,
                context: CONTEXT.try_with(Clone::clone).unwrap_or_default(),
                provider: provider.name().to_string(),
                runtime: provider.display_name(),
                model_at_dispatch: provider.model(),
                model_actual: None,
                account_ref: None,
                account_aliases: Vec::new(),
                billing_route: None,
                transport_observations: Vec::new(),
                auth_method_at_dispatch: provider.active_auth_method_label().map(str::to_owned),
                reasoning_effort: provider.reasoning_effort(),
                service_tier: provider.service_tier(),
                upstream_request_id: None,
                status: "cancelled".into(),
                usage_present: false,
                tokens: UsageTokens::default(),
                attempt_coverage: "logical_dispatch_with_observed_rollback_segments; internal_unannounced_retries_unknown",
            },
            started: Instant::now(),
            destination: crate::storage::jcode_dir()
                .ok()
                .map(|p| p.join("usage-events")),
            finalized: false,
            observations: Default::default(),
        }
    }

    fn observe(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::TokenUsage {
                input_tokens,
                output_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
            } => {
                self.event.usage_present = true;
                // Updates are snapshots. Missing fields do not erase earlier values.
                if input_tokens.is_some() {
                    self.event.tokens.input = *input_tokens;
                }
                if output_tokens.is_some() {
                    self.event.tokens.output = *output_tokens;
                }
                if cache_read_input_tokens.is_some() {
                    self.event.tokens.cache_read = *cache_read_input_tokens;
                }
                if cache_creation_input_tokens.is_some() {
                    self.event.tokens.cache_write = *cache_creation_input_tokens;
                }
            }
            StreamEvent::RetryRollback { .. } => {
                self.emit("interrupted_retry");
                self.event.segment += 1;
                self.event.event_id = format!("{}:{}", self.event.request_id, self.event.segment);
                self.event.usage_present = false;
                self.event.tokens = UsageTokens::default();
                self.event.status = "cancelled".into();
            }
            StreamEvent::MessageEnd { .. } if self.event.status != "error" => {
                self.event.status = "completed".into();
            }
            StreamEvent::Error { .. } => self.event.status = "error".into(),
            _ => {}
        }
    }

    fn emit(&mut self, status: &str) {
        self.event.transport_observations = self
            .observations
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default();
        // Retry/failover observations can race the consumer's rollback event.
        // Never assign their aggregate tokens to the latest mutable account.
        if self.event.segment == 0
            && status != "interrupted_retry"
            && self.event.transport_observations.len() == 1
        {
            let observed = &self.event.transport_observations[0];
            self.event.account_ref = observed.account_ref.clone();
            self.event.account_aliases = observed.account_aliases.clone();
            self.event.billing_route = observed.billing_route.clone();
            self.event.model_actual = observed.model_actual.clone();
            self.event.upstream_request_id = observed.upstream_request_id.clone();
        }
        self.event.completed_at = Utc::now().to_rfc3339();
        self.event.duration_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.event.status = status.to_owned();
        if let Some(destination) = &self.destination {
            enqueue(destination.clone(), self.event.clone());
        }
    }

    fn finish(&mut self, status: &str) {
        if !self.finalized {
            self.finalized = true;
            self.emit(status);
        }
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        let status = self.event.status.clone();
        self.finish(&status);
    }
}

/// Wrap exactly one selected runtime invocation, including dispatch failures.
/// Metadata is captured before await. Account/actual-model remain unknown until
/// an upstream transport supplies them, rather than sampling mutable state later.
pub async fn observe(
    provider: &dyn Provider,
    future: impl Future<Output = anyhow::Result<EventStream>>,
) -> anyhow::Result<EventStream> {
    let mut call = Call::new(provider);
    match transport::OBSERVATIONS
        .scope(call.observations.clone(), future)
        .await
    {
        Ok(inner) => Ok(Box::pin(AccountedStream { inner, call })),
        Err(error) => {
            call.finish("dispatch_error");
            Err(error)
        }
    }
}

struct AccountedStream {
    inner: EventStream,
    call: Call,
}
impl Stream for AccountedStream {
    type Item = anyhow::Result<StreamEvent>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = self.inner.as_mut().poll_next(cx);
        match &result {
            Poll::Ready(Some(Ok(event))) => self.call.observe(event),
            Poll::Ready(Some(Err(_))) => self.call.event.status = "error".into(),
            Poll::Ready(None) => {
                let status = if self.call.event.status == "cancelled" {
                    "ended_without_message_end".into()
                } else {
                    self.call.event.status.clone()
                };
                self.call.finish(&status);
            }
            _ => {}
        }
        result
    }
}

type Pending = (PathBuf, UsageEvent);
static WRITER: OnceLock<Option<mpsc::SyncSender<Pending>>> = OnceLock::new();
fn enqueue(directory: PathBuf, event: UsageEvent) {
    let writer = WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Pending>(256);
        match std::thread::Builder::new()
            .name("usage-journal".into())
            .spawn(move || {
                for (directory, event) in rx {
                    if append_event(&directory, &event).is_err() {
                        crate::logging::warn(
                            "Usage journal write failed; accounting coverage incomplete",
                        );
                    }
                }
            }) {
            Ok(_) => Some(tx),
            Err(_) => None,
        }
    });
    if writer
        .as_ref()
        .is_none_or(|tx| tx.try_send((directory, event)).is_err())
    {
        crate::logging::warn("Usage journal queue unavailable/full; accounting event dropped");
    }
}

fn append_event(directory: &std::path::Path, event: &UsageEvent) -> std::io::Result<()> {
    std::fs::create_dir_all(directory)?;
    // One append writer per process. Unique process epoch prevents PID reuse
    // collisions, and daily files make retention/aggregation cheap.
    static EPOCH: OnceLock<String> = OnceLock::new();
    let epoch = EPOCH.get_or_init(|| uuid::Uuid::new_v4().to_string());
    let day = &event.completed_at[..10];
    let path = directory.join(format!("{day}-{epoch}.jsonl"));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // A broken producer cannot exhaust the disk. Normal metadata volume is
    // hundreds of bytes per response, far below this per-process/day ceiling.
    if file.metadata()?.len() > 32 * 1024 * 1024 {
        return Err(std::io::Error::other("usage journal daily shard full"));
    }
    let mut bytes = serde_json::to_vec(event)?;
    bytes.push(b'\n');
    file.write_all(&bytes)
}

#[cfg(test)]
#[path = "usage_journal_tests.rs"]
mod tests;
