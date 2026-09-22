//! Publish locally durable GateRequest V2 packets to Hermes exactly once.
//!
//! Hermes delivery is observational. This publisher never answers a gate or
//! wakes a session: a confirmed create response records an acknowledgement and
//! every ambiguous outcome records `DeliveryUnknown`, which prevents retries.

use super::Server;
use crate::auth::external::hermes::load_hermes_api_key;
use crate::gates::{DeliveryAcknowledgement, GateDecision, GateRequest, GateStore, GateSubject};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const TARGET_EXECUTOR: &str = "opencode";
const AWAITING_CONFIRM: &str = "awaiting_confirm";

#[derive(Clone, Debug, PartialEq, Eq)]
struct HermesGateEgressConfig {
    api_base: String,
    api_key: String,
    poll_interval: Duration,
    request_timeout: Duration,
}

impl HermesGateEgressConfig {
    fn from_environment() -> Option<Self> {
        Self::from_values(
            non_empty_env("HERMES_URL"),
            non_empty_env("HERMES_KEY").or_else(load_hermes_api_key),
        )
    }

    fn from_values(api_base: Option<String>, api_key: Option<String>) -> Option<Self> {
        let api_key = api_key?.trim().to_string();
        if api_key.is_empty() {
            return None;
        }
        Some(Self {
            api_base: normalize_api_base(&api_base?),
            api_key,
            poll_interval: configured_poll_interval(),
            request_timeout: REQUEST_TIMEOUT,
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

/// The exact V2 request accepted by Hermes's `/envelopes` endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HermesGateRequestV2 {
    schema: String,
    gate_id: String,
    jcode_session_id: String,
    subject: GateSubject,
    question: String,
    problem: String,
    recommendation: String,
    options: Vec<HermesGateOption>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HermesGateOption {
    decision: GateDecision,
    label: String,
}

/// Strict Hermes create-envelope fields. Hermes-notify dispatches only the
/// explicit `awaiting_confirm` status to the `opencode` target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct HermesGateEnvelope {
    id: String,
    created_by: String,
    title: String,
    task_title: String,
    target_executor: String,
    status: String,
    gate_request: HermesGateRequestV2,
}

#[derive(Debug, Deserialize)]
struct HermesPersistedGateEnvelope {
    id: String,
    created_by: String,
    title: String,
    task_title: String,
    target_executor: String,
    status: String,
    gate_request: HermesGateRequestV2,
}

#[derive(Clone)]
struct HermesGateRequestClient {
    config: HermesGateEgressConfig,
    http: reqwest::Client,
}

impl HermesGateRequestClient {
    fn new(config: HermesGateEgressConfig) -> Self {
        Self {
            config,
            http: crate::provider::shared_http_client(),
        }
    }

    fn envelopes_url(&self) -> String {
        format!("{}envelopes", self.config.api_base)
    }

    fn envelope_for(&self, gate: &GateRequest) -> Result<HermesGateEnvelope> {
        let transport = gate
            .transport_binding
            .as_ref()
            .context("pending transported gate missing transport binding")?;
        Ok(HermesGateEnvelope {
            id: transport.envelope_id.clone(),
            created_by: "jcode".to_string(),
            title: "Jcode gate".to_string(),
            task_title: "User decision".to_string(),
            target_executor: TARGET_EXECUTOR.to_string(),
            status: AWAITING_CONFIRM.to_string(),
            gate_request: HermesGateRequestV2 {
                schema: "hermes.gate_request/v2".to_string(),
                gate_id: gate.gate_id.clone(),
                jcode_session_id: gate.binding.session_id.clone(),
                subject: gate.binding.subject.clone(),
                question: gate.question.question.clone(),
                problem: gate.question.problem.clone(),
                recommendation: gate.question.recommendation.clone(),
                options: gate
                    .allowed_decisions
                    .iter()
                    .copied()
                    .map(|decision| HermesGateOption {
                        decision,
                        label: decision_label(decision).to_string(),
                    })
                    .collect(),
                created_at: gate.created_at,
                expires_at: gate.expires_at,
                // The local deterministic envelope identity is intentionally URL
                // independent and is also Hermes's idempotency correlation key.
                idempotency_key: transport.envelope_id.clone(),
            },
        })
    }

    async fn publish_gate(&self, gate: &GateRequest) -> std::result::Result<(), &'static str> {
        let envelope = self
            .envelope_for(gate)
            .map_err(|_| "invalid local gate transport binding")?;
        let body = serde_json::to_vec(&envelope).map_err(|_| "invalid Hermes gate request")?;
        if body.len() > MAX_BODY_BYTES {
            return Err("Hermes gate request body exceeds limit");
        }

        let response = self
            .http
            .post(self.envelopes_url())
            .header("X-Hermes-Key", &self.config.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(self.config.request_timeout)
            .send()
            .await
            .map_err(|_| "Hermes gate request was not confirmed")?;
        if response.status() != reqwest::StatusCode::CREATED {
            return Err("Hermes gate request returned an unconfirmed status");
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY_BYTES as u64)
        {
            return Err("Hermes gate response body exceeds limit");
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "Hermes gate response was not confirmed")?;
            if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
                return Err("Hermes gate response body exceeds limit");
            }
            body.extend_from_slice(&chunk);
        }
        let persisted: HermesPersistedGateEnvelope =
            serde_json::from_slice(&body).map_err(|_| "Hermes gate response was malformed")?;
        if persisted.id != envelope.id
            || persisted.created_by != envelope.created_by
            || persisted.title != envelope.title
            || persisted.task_title != envelope.task_title
            || persisted.target_executor != envelope.target_executor
            || persisted.status != envelope.status
            || persisted.gate_request != envelope.gate_request
        {
            return Err("Hermes gate response did not persist the exact gate");
        }
        Ok(())
    }

    /// Scan only pending transported gates, and only publish a never-attempted
    /// delivery. Every attempted item becomes terminally observed in the ledger.
    async fn publish_once(&self, store: &GateStore, now: DateTime<Utc>) {
        let Ok(gates) = store.list_pending_transported_gates() else {
            return;
        };
        for gate in gates {
            if !matches!(gate.delivery, DeliveryAcknowledgement::NotAttempted) {
                continue;
            }
            let delivery = match self.publish_gate(&gate).await {
                Ok(()) => DeliveryAcknowledgement::Acknowledged {
                    acknowledged_at: now,
                },
                Err(reason) => DeliveryAcknowledgement::DeliveryUnknown {
                    observed_at: now,
                    reason: reason.to_string(),
                },
            };
            if store
                .record_delivery(&gate.gate_id, &gate.binding.session_id, delivery)
                .is_err()
            {
                crate::logging::warn("Hermes gate delivery observation could not be persisted");
            }
        }
    }
}

fn decision_label(decision: GateDecision) -> &'static str {
    match decision {
        GateDecision::Approve => "Approve",
        GateDecision::RequestChanges => "Request changes",
        GateDecision::Reject => "Reject",
    }
}

pub(super) fn spawn_if_configured(server: &Server) {
    let Some(config) = HermesGateEgressConfig::from_environment() else {
        crate::logging::info("Hermes gate-request egress disabled: HERMES_URL or key unavailable");
        return;
    };
    let store = match GateStore::from_jcode_home() {
        Ok(store) => store,
        Err(_) => {
            crate::logging::warn("Hermes gate-request egress disabled: gate store unavailable");
            return;
        }
    };
    let poll_interval = config.poll_interval;
    let client = HermesGateRequestClient::new(config);
    let cancellation = server.hermes_gate_egress_cancellation.clone();

    crate::logging::info("Hermes gate-request egress started");
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = interval.tick() => client.publish_once(&store, Utc::now()).await,
            }
        }
    });
}

#[cfg(test)]
#[path = "hermes_gate_egress_tests.rs"]
mod tests;
