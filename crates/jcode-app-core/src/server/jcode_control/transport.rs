use crate::auth::external::hermes::load_hermes_api_key;
use crate::jcode_control::{
    AuthenticatedPrincipal, ControlAction, ControlRequest, PinnedCreateSessionResolution,
};
use anyhow::{Context, Result, bail};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);
const MAX_POLL_INTERVAL: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const POLL_LIMIT: usize = 16;

/// All fields are local operator configuration. Omitting any required field keeps
/// the task disabled, rather than guessing a root, principal, or model route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HermesControlConfig {
    pub(super) api_base: String,
    pub(super) api_key: String,
    pub(super) node: String,
    pub(super) principal: String,
    pub(super) root_id: String,
    pub(super) root: PathBuf,
    pub(super) route_alias: String,
    pub(super) route_model: String,
    pub(super) route_api_method: Option<String>,
    pub(super) route_effort: Option<String>,
    pub(super) poll_interval: Duration,
}

impl HermesControlConfig {
    pub(super) fn from_environment() -> Option<Self> {
        let api_base = non_empty_env("HERMES_URL")?;
        let api_key = non_empty_env("HERMES_KEY").or_else(load_hermes_api_key)?;
        let node = non_empty_env("JCODE_HERMES_CONTROL_NODE")?;
        let principal = non_empty_env("JCODE_HERMES_CONTROL_PRINCIPAL")?;
        let root_id = non_empty_env("JCODE_HERMES_CONTROL_ROOT_ID")?;
        let root = PathBuf::from(non_empty_env("JCODE_HERMES_CONTROL_ROOT_PATH")?);
        let root = root.canonicalize().ok()?;
        let route_alias = non_empty_env("JCODE_HERMES_CONTROL_ROUTE_ALIAS")?;
        let route_model = non_empty_env("JCODE_HERMES_CONTROL_ROUTE_MODEL")?;
        Some(Self {
            api_base: normalize_api_base(&api_base),
            api_key,
            node,
            principal,
            root_id,
            root,
            route_alias,
            route_model,
            route_api_method: non_empty_env("JCODE_HERMES_CONTROL_ROUTE_API_METHOD"),
            route_effort: non_empty_env("JCODE_HERMES_CONTROL_ROUTE_EFFORT"),
            poll_interval: configured_poll_interval(),
        })
    }

    pub(super) fn pinned_resolution(&self) -> Result<PinnedCreateSessionResolution> {
        PinnedCreateSessionResolution::new(self.root_id.clone(), self.route_alias.clone())
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
    non_empty_env("JCODE_HERMES_CONTROL_POLL_SECS")
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_POLL_INTERVAL)
        .clamp(MIN_POLL_INTERVAL, MAX_POLL_INTERVAL)
}

#[derive(Clone)]
pub(super) struct HermesControlClient {
    pub(super) config: HermesControlConfig,
    http: reqwest::Client,
}

impl HermesControlClient {
    pub(super) fn new(config: HermesControlConfig) -> Self {
        Self {
            config,
            http: crate::provider::shared_http_client(),
        }
    }

    fn requests_url(&self) -> String {
        format!("{}jcode-control/requests", self.config.api_base)
    }

    fn request_url(&self, id: &str, suffix: &str) -> String {
        format!(
            "{}/{}/{}",
            self.requests_url(),
            urlencoding::encode(id),
            suffix
        )
    }

    pub(super) async fn pending(&self) -> Result<Vec<HermesControlRecord>> {
        let response = self
            .http
            .get(self.requests_url())
            .query(&[
                ("target_node", self.config.node.as_str()),
                ("limit", &POLL_LIMIT.to_string()),
            ])
            .header("X-Hermes-Key", &self.config.api_key)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .context("Hermes control pending request failed")?;
        if !response.status().is_success() {
            bail!("Hermes control pending returned unconfirmed status")
        }
        read_json(response)
            .await
            .context("invalid Hermes control pending response")
    }

    pub(super) async fn claim(&self, id: &str) -> Result<HermesControlRecord> {
        let response = self
            .http
            .post(self.request_url(id, "claim"))
            .header("X-Hermes-Key", &self.config.api_key)
            .json(&HermesTarget {
                target_node: &self.config.node,
            })
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .context("Hermes control claim request failed")?;
        if !response.status().is_success() {
            bail!("Hermes control claim was not confirmed")
        }
        read_json(response)
            .await
            .context("invalid Hermes control claim response")
    }

    pub(super) async fn resolve(&self, id: &str, result: HermesResult<'_>) -> Result<()> {
        let response = self
            .http
            .post(self.request_url(id, "result"))
            .header("X-Hermes-Key", &self.config.api_key)
            .json(&result)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .context("Hermes control result request failed")?;
        if !response.status().is_success() {
            bail!("Hermes control result was not confirmed")
        }
        let _: HermesControlRecord = read_json(response)
            .await
            .context("invalid Hermes control result response")?;
        Ok(())
    }
}

async fn read_json<T: for<'de> Deserialize<'de>>(response: reqwest::Response) -> Result<T> {
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BODY_BYTES as u64)
    {
        bail!("Hermes control response exceeds limit")
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Hermes control response stream failed")?;
        if bytes.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
            bail!("Hermes control response exceeds limit")
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Debug, Serialize)]
struct HermesTarget<'a> {
    target_node: &'a str,
}

#[derive(Debug, Serialize)]
pub(super) struct HermesResult<'a> {
    pub(super) target_node: &'a str,
    pub(super) status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error_code: Option<&'a str>,
}

/// Strict DTO for the only Hermes records this task understands. Payload fields
/// are intentionally absent from Debug output so a rejected payload cannot leak.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HermesControlRecord {
    pub(super) id: String,
    authenticated_principal: String,
    idempotency_key: String,
    submission: HermesSubmission,
    state: String,
    #[serde(rename = "claimed_by", default)]
    _claimed_by: String,
    #[serde(rename = "created_at")]
    _created_at: String,
    #[serde(rename = "claimed_at", default)]
    _claimed_at: String,
    #[serde(rename = "resolved_at", default)]
    _resolved_at: String,
    #[serde(rename = "result", default)]
    _result: Option<serde_json::Value>,
}

impl std::fmt::Debug for HermesControlRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HermesControlRecord")
            .field("id", &self.id)
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HermesSubmission {
    schema: String,
    idempotency_key: String,
    action: String,
    #[serde(rename = "envelope_id")]
    _envelope_id: String,
    target_node: String,
    #[serde(default)]
    jcode_session_id: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    wake: bool,
    #[serde(default)]
    root_id: String,
    #[serde(default)]
    route_alias: String,
    #[serde(default)]
    initial_prompt: String,
}

impl std::fmt::Debug for HermesSubmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HermesSubmission")
            .field("schema", &self.schema)
            .field("action", &self.action)
            .finish()
    }
}

impl HermesControlRecord {
    pub(super) fn map_request(
        &self,
        config: &HermesControlConfig,
    ) -> Result<(AuthenticatedPrincipal, ControlRequest)> {
        if self.state != "pending" && self.state != "claimed" {
            bail!("Hermes control record is not executable")
        }
        if self.submission.schema != "hermes.jcode_control/v1"
            || self.submission.target_node != config.node
        {
            bail!("Hermes control target/schema rejected")
        }
        if self.submission.idempotency_key != self.idempotency_key {
            bail!("Hermes control idempotency mismatch")
        }
        let principal = AuthenticatedPrincipal::new(self.authenticated_principal.clone())?;
        if principal.as_str() != config.principal {
            bail!("Hermes control principal rejected")
        }
        let action = match self.submission.action.as_str() {
            "message_existing" => {
                if self.submission.root_id.is_empty()
                    && self.submission.route_alias.is_empty()
                    && self.submission.initial_prompt.is_empty()
                {
                    ControlAction::MessageExisting {
                        jcode_session_id: self.submission.jcode_session_id.clone(),
                        message: self.submission.message.clone(),
                        wake: self.submission.wake,
                    }
                } else {
                    bail!("Hermes message action has create fields")
                }
            }
            "create_session" => {
                if self.submission.jcode_session_id.is_empty()
                    && self.submission.message.is_empty()
                    && !self.submission.wake
                {
                    ControlAction::CreateSession {
                        root_id: self.submission.root_id.clone(),
                        route_alias: self.submission.route_alias.clone(),
                        initial_prompt: (!self.submission.initial_prompt.is_empty())
                            .then(|| self.submission.initial_prompt.clone()),
                    }
                } else {
                    bail!("Hermes create action has message fields")
                }
            }
            _ => bail!("Hermes control action rejected"),
        };
        let request = ControlRequest {
            schema: "jcode_control.v1".to_string(),
            idempotency_key: self.idempotency_key.clone(),
            action,
        };
        request.validate()?;
        if let ControlAction::CreateSession {
            root_id,
            route_alias,
            ..
        } = &request.action
            && (root_id != &config.root_id || route_alias != &config.route_alias)
        {
            bail!("Hermes control root/route rejected")
        }
        Ok((principal, request))
    }
}
