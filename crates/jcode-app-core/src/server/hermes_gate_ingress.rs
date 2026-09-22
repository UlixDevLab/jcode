//! Pull Hermes gate answers only for locally-known pending transport bindings.
//!
//! Hermes is a transport, not an authorization source. This module first obtains
//! the bounded list from [`GateStore::list_pending_transported_gates`], requires
//! every returned receipt to match that local binding exactly, then asks the
//! existing gate-resume consumer to deliver the resulting durable directive.

use super::Server;
use super::gate_resume::consume_and_deliver_to_live_session;
use super::live_turn::LiveTurnSwarmContext;
use crate::auth::external::hermes::load_hermes_api_key;
use crate::gates::{
    AnswerRecord, GateBinding, GateDecision, GateRequest, GateStore, GateSubject,
    RecordAnswerOutcome,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Minimal runtime configuration for the narrow Hermes answer endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
struct HermesGateIngressConfig {
    api_base: String,
    api_key: String,
    poll_interval: Duration,
}

impl HermesGateIngressConfig {
    fn from_environment() -> Option<Self> {
        let api_base = non_empty_env("HERMES_URL")?;
        let api_key = non_empty_env("HERMES_KEY").or_else(load_hermes_api_key)?;
        Some(Self {
            api_base: normalize_api_base(&api_base),
            api_key,
            poll_interval: configured_poll_interval(),
        })
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim();
    if trimmed.ends_with('/') {
        trimmed.to_string()
    } else {
        format!("{trimmed}/")
    }
}

fn configured_poll_interval() -> Duration {
    let seconds = non_empty_env("JCODE_HERMES_GATE_POLL_SECS")
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_POLL_INTERVAL);
    seconds.clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL)
}

#[derive(Clone)]
struct HermesGateAnswerClient {
    config: HermesGateIngressConfig,
    http: reqwest::Client,
}

impl HermesGateAnswerClient {
    fn new(config: HermesGateIngressConfig) -> Self {
        Self {
            config,
            http: crate::provider::shared_http_client(),
        }
    }

    fn answer_url(&self, envelope_id: &str) -> String {
        format!(
            "{}envelopes/{}/gate-answer",
            self.config.api_base,
            urlencoding::encode(envelope_id)
        )
    }

    /// Fetch one exact known envelope. `None` means the answer is absent or
    /// unusable, and therefore can never lead to a continuation.
    async fn fetch_answer(&self, envelope_id: &str) -> Result<Option<HermesGateAnswerReceipt>> {
        let response = self
            .http
            .get(self.answer_url(envelope_id))
            .header("X-Hermes-Key", &self.config.api_key)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .context("Hermes gate-answer request failed")?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            anyhow::bail!(
                "Hermes gate-answer request returned status {}",
                response.status()
            );
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            anyhow::bail!("Hermes gate-answer response body exceeds limit");
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Hermes gate-answer response stream failed")?;
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                anyhow::bail!("Hermes gate-answer response body exceeds limit");
            }
            body.extend_from_slice(&chunk);
        }
        let response: HermesGateAnswerResponse =
            serde_json::from_slice(&body).context("invalid Hermes gate-answer response")?;
        Ok(Some(response.into_receipt()))
    }

    /// Poll only transport-bound gates that are still locally pending. A fetch
    /// error on one gate never changes that gate or another gate's lifecycle.
    async fn poll_once(&self, store: &GateStore, now: DateTime<Utc>) -> Vec<RecordedGateAnswer> {
        let Ok(gates) = store.list_pending_transported_gates() else {
            return Vec::new();
        };
        let mut recorded = Vec::new();
        for gate in gates {
            let Some(transport) = gate.transport_binding.as_ref() else {
                continue;
            };
            let receipt = match self.fetch_answer(&transport.envelope_id).await {
                Ok(Some(receipt)) => receipt,
                Ok(None) | Err(_) => continue,
            };
            let Some(answer) = answer_for_gate(&gate, receipt) else {
                continue;
            };
            match store.record_answer(answer, now) {
                Ok(
                    RecordAnswerOutcome::Recorded { .. } | RecordAnswerOutcome::Idempotent { .. },
                ) => {
                    recorded.push(RecordedGateAnswer {
                        gate_id: gate.gate_id,
                        session_id: gate.binding.session_id,
                    });
                }
                Ok(RecordAnswerOutcome::Expired) | Err(_) => {}
            }
        }
        recorded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedGateAnswer {
    gate_id: String,
    session_id: String,
}

/// Hermes's V2 receipt has additional receipt/idempotency metadata that Jcode
/// does not authorize against. Serde intentionally ignores those extra fields;
/// the envelope, gate, exact binding, decision, comment, and timestamp below
/// are the complete Jcode authorization input.
#[derive(Debug, Clone, Deserialize)]
struct HermesGateAnswerReceipt {
    envelope_id: String,
    gate_id: String,
    jcode_session_id: String,
    subject: GateSubject,
    decision: GateDecision,
    #[serde(default)]
    comment: Option<String>,
    #[serde(alias = "decided_at")]
    answered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum HermesGateAnswerResponse {
    Receipt(HermesGateAnswerReceipt),
    Wrapped { receipt: HermesGateAnswerReceipt },
}

impl HermesGateAnswerResponse {
    fn into_receipt(self) -> HermesGateAnswerReceipt {
        match self {
            Self::Receipt(receipt) | Self::Wrapped { receipt } => receipt,
        }
    }
}

fn answer_for_gate(gate: &GateRequest, receipt: HermesGateAnswerReceipt) -> Option<AnswerRecord> {
    let transport = gate.transport_binding.as_ref()?;
    if receipt.envelope_id != transport.envelope_id
        || receipt.gate_id != gate.gate_id
        || receipt.jcode_session_id != gate.binding.session_id
        || receipt.subject != gate.binding.subject
    {
        return None;
    }

    let answer = AnswerRecord {
        schema_version: 2,
        gate_id: receipt.gate_id,
        binding: GateBinding {
            session_id: receipt.jcode_session_id,
            subject: receipt.subject,
        },
        decision: receipt.decision,
        comment: receipt.comment,
        answered_at: receipt.answered_at,
        received_via: "hermes".to_string(),
    };
    gate.validate_answer(&answer).ok()?;
    Some(answer)
}

pub(super) fn spawn_if_configured(server: &Server) {
    let Some(config) = HermesGateIngressConfig::from_environment() else {
        crate::logging::info("Hermes gate-answer ingress disabled: HERMES_URL or key unavailable");
        return;
    };
    let store = match GateStore::from_jcode_home() {
        Ok(store) => store,
        Err(_) => {
            crate::logging::warn("Hermes gate-answer ingress disabled: gate store unavailable");
            return;
        }
    };
    let poll_interval = config.poll_interval;
    let client = HermesGateAnswerClient::new(config);
    let swarm = LiveTurnSwarmContext::new(
        &server.swarm_state.members,
        &server.swarm_state.swarms_by_id,
        &server.event_history,
        &server.event_counter,
        &server.swarm_event_tx,
    );
    let sessions = Arc::clone(&server.sessions);
    let soft_interrupt_queues = Arc::clone(&server.soft_interrupt_queues);
    let cancellation = server.hermes_gate_ingress_cancellation.clone();

    crate::logging::info("Hermes gate-answer ingress started");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = interval.tick() => {
                    for answer in client.poll_once(&store, Utc::now()).await {
                        if consume_and_deliver_to_live_session(
                            &store,
                            &answer.gate_id,
                            &answer.session_id,
                            Utc::now(),
                            &sessions,
                            &soft_interrupt_queues,
                            swarm.clone(),
                        ).await.is_err() {
                            crate::logging::warn("Hermes gate answer was recorded but live delivery failed");
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
#[path = "hermes_gate_ingress_tests.rs"]
mod tests;
