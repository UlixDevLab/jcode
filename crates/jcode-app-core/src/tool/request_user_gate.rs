use super::{Tool, ToolContext, ToolOutput};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use jcode_base::gates::{
    DeliveryAcknowledgement, GateBinding, GateContinuation, GateDecision, GateOption, GateQuestion,
    GateRequest, GateState, GateStore, GateTransportBinding,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(super) const PACKET_REVISION: u64 = 1;
const MAX_EXPIRY: Duration = Duration::days(30);
const GATE_ID_DOMAIN: &[u8] = b"jcode/request-user-gate/id/v1\0";
const PACKET_HASH_DOMAIN: &[u8] = b"jcode/request-user-gate/packet/v1\0";

/// Persist a high-level user choice without delivering, waking, or waiting for it.
pub(super) struct RequestUserGateTool;

impl RequestUserGateTool {
    pub(super) fn new() -> Self {
        Self
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestUserGateInput {
    question: String,
    context: RequestUserGateContext,
    options: Vec<GateOption>,
    recommendation: String,
    narrows: String,
    affects_constraints: Vec<String>,
    allowed_decisions: Vec<GateDecision>,
    expires_at: DateTime<Utc>,
    continuation: GateContinuation,
    // The central registry owns the schema requirement. Accept it only so its
    // model-visible field does not become an unknown field at execution time.
    #[serde(default, rename = "intent")]
    _intent: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestUserGateContext {
    problem: String,
    impact: String,
}

#[async_trait]
impl Tool for RequestUserGateTool {
    fn name(&self) -> &str {
        "request_user_gate"
    }

    fn description(&self) -> &str {
        "Persist a high-level user decision request and return immediately while it waits."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": [
                "question",
                "context",
                "options",
                "recommendation",
                "narrows",
                "affects_constraints",
                "allowed_decisions",
                "expires_at",
                "continuation"
            ],
            "properties": {
                "question": {"type": "string"},
                "context": {
                    "type": "object",
                    "required": ["problem", "impact"],
                    "properties": {
                        "problem": {"type": "string"},
                        "impact": {"type": "string"}
                    },
                    "additionalProperties": false
                },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["label", "description"],
                        "properties": {
                            "label": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "additionalProperties": false
                    }
                },
                "recommendation": {"type": "string"},
                "narrows": {"type": "string"},
                "affects_constraints": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "allowed_decisions": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["approve", "request_changes", "reject"]
                    }
                },
                "expires_at": {"type": "string", "format": "date-time"},
                "continuation": {
                    "type": "object",
                    "required": ["on_approve", "on_request_changes", "on_reject", "on_expiry"],
                    "properties": {
                        "on_approve": {"type": "string"},
                        "on_request_changes": {"type": "string"},
                        "on_reject": {"type": "string"},
                        "on_expiry": {"type": "string"}
                    },
                    "additionalProperties": false
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let request: RequestUserGateInput =
            serde_json::from_value(input.clone()).context("invalid request_user_gate input")?;
        let gate_id = deterministic_gate_id(&ctx.session_id, &ctx.tool_call_id);
        let store = GateStore::from_jcode_home()?;

        // This check intentionally precedes Utc::now(). Exact retries must reuse
        // the original wall-clock fields and their packet hash, never regenerate
        // a superficially similar durable request.
        let path = store.path_for(&gate_id, &ctx.session_id)?;
        if path.exists() {
            let existing = store.load(&gate_id, &ctx.session_id)?;
            if replay_matches(&existing, &request, &gate_id, &ctx.session_id)? {
                return waiting_output(&existing);
            }
            bail!("conflicting request_user_gate replay");
        }

        let created_at = Utc::now();
        validate_new_request(&request, created_at)?;
        let packet_hash = packet_hash_for_request(&gate_id, &ctx.session_id, &request, created_at);
        let binding = GateBinding::session_decision(&ctx.session_id, PACKET_REVISION, packet_hash);
        let gate = GateRequest {
            schema_version: 2,
            gate_id,
            binding: binding.clone(),
            question: GateQuestion {
                question: request.question,
                problem: request.context.problem,
                impact: request.context.impact,
                options: request.options,
                recommendation: request.recommendation,
            },
            allowed_decisions: request.allowed_decisions,
            continuation: request.continuation,
            created_at,
            expires_at: request.expires_at,
            delivery: DeliveryAcknowledgement::NotAttempted,
            state: GateState::Pending,
            transport_binding: None,
            resume_directive: None,
        };
        let transport = GateTransportBinding::for_gate(&gate.gate_id, binding)?;
        store.create_pending_transported(gate.clone(), transport)?;
        waiting_output(&gate)
    }
}

fn validate_new_request(request: &RequestUserGateInput, created_at: DateTime<Utc>) -> Result<()> {
    if request.narrows.trim().is_empty() {
        bail!("request_user_gate narrows must be a verbatim intent quote, constraint id, or none");
    }
    if request.narrows == "none" && request.affects_constraints.is_empty() {
        bail!("this is a consent, not a decision");
    }
    if request
        .affects_constraints
        .iter()
        .any(|constraint| constraint.trim().is_empty())
    {
        bail!("request_user_gate affects_constraints cannot contain empty ids");
    }
    if request.options.is_empty() {
        bail!("request_user_gate requires at least one option");
    }
    let mut labels = HashSet::new();
    for option in &request.options {
        if !labels.insert(option.label.trim()) {
            bail!("request_user_gate contains duplicate option labels");
        }
    }
    if request.allowed_decisions.is_empty() {
        bail!("request_user_gate requires at least one allowed decision");
    }
    if request.expires_at <= created_at {
        bail!("request_user_gate expiry must be in the future");
    }
    if request.expires_at > created_at + MAX_EXPIRY {
        bail!("request_user_gate expiry cannot exceed 30 days");
    }
    Ok(())
}

fn replay_matches(
    existing: &GateRequest,
    request: &RequestUserGateInput,
    gate_id: &str,
    session_id: &str,
) -> Result<bool> {
    if existing.question.question != request.question
        || existing.question.problem != request.context.problem
        || existing.question.impact != request.context.impact
        || existing.question.options != request.options
        || existing.question.recommendation != request.recommendation
        || existing.allowed_decisions != request.allowed_decisions
        || existing.continuation != request.continuation
        || existing.expires_at != request.expires_at
    {
        return Ok(false);
    }
    let expected_hash = packet_hash_for_request(gate_id, session_id, request, existing.created_at);
    Ok(existing.binding
        == GateBinding::session_decision(session_id, PACKET_REVISION, expected_hash))
}

fn waiting_output(gate: &GateRequest) -> Result<ToolOutput> {
    Ok(ToolOutput::new(format!(
        "User decision gate `{}` is waiting for a response.",
        gate.gate_id
    ))
    .with_metadata(json!({
        "status": "waiting",
        "gate_id": gate.gate_id,
        "request_id": gate.gate_id,
        "expires_at": timestamp_bytes(gate.expires_at),
        "continuation_pending": true
    })))
}

pub(super) fn deterministic_gate_id(session_id: &str, tool_call_id: &str) -> String {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(GATE_ID_DOMAIN);
    append_field(&mut canonical, session_id.as_bytes());
    append_field(&mut canonical, tool_call_id.as_bytes());
    let digest = Sha256::digest(canonical);
    format!("request-user-gate-v1-{digest:x}")
}

#[cfg(test)]
pub(super) fn packet_hash(
    gate_id: &str,
    session_id: &str,
    input: &Value,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<String> {
    let mut request: RequestUserGateInput = serde_json::from_value(input.clone())?;
    request.expires_at = expires_at;
    Ok(packet_hash_for_request(
        gate_id, session_id, &request, created_at,
    ))
}

fn packet_hash_for_request(
    gate_id: &str,
    session_id: &str,
    request: &RequestUserGateInput,
    created_at: DateTime<Utc>,
) -> String {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(PACKET_HASH_DOMAIN);
    append_field(&mut canonical, gate_id.as_bytes());
    append_field(&mut canonical, session_id.as_bytes());
    append_field(&mut canonical, &PACKET_REVISION.to_be_bytes());
    append_field(&mut canonical, request.question.as_bytes());
    append_field(&mut canonical, request.context.problem.as_bytes());
    append_field(&mut canonical, request.context.impact.as_bytes());
    append_count(&mut canonical, request.options.len());
    for option in &request.options {
        append_field(&mut canonical, option.label.as_bytes());
        append_field(&mut canonical, option.description.as_bytes());
    }
    append_field(&mut canonical, request.recommendation.as_bytes());
    append_field(&mut canonical, request.narrows.as_bytes());
    append_count(&mut canonical, request.affects_constraints.len());
    for constraint in &request.affects_constraints {
        append_field(&mut canonical, constraint.as_bytes());
    }
    append_count(&mut canonical, request.allowed_decisions.len());
    for decision in &request.allowed_decisions {
        append_field(&mut canonical, decision_name(*decision).as_bytes());
    }
    append_field(&mut canonical, request.continuation.on_approve.as_bytes());
    append_field(
        &mut canonical,
        request.continuation.on_request_changes.as_bytes(),
    );
    append_field(&mut canonical, request.continuation.on_reject.as_bytes());
    append_field(&mut canonical, request.continuation.on_expiry.as_bytes());
    append_field(&mut canonical, timestamp_bytes(created_at).as_bytes());
    append_field(
        &mut canonical,
        timestamp_bytes(request.expires_at).as_bytes(),
    );
    format!("sha256:v1:{:x}", Sha256::digest(canonical))
}

fn append_count(canonical: &mut Vec<u8>, count: usize) {
    append_field(canonical, &(count as u64).to_be_bytes());
}

fn append_field(canonical: &mut Vec<u8>, field: &[u8]) {
    canonical.extend_from_slice(&(field.len() as u64).to_be_bytes());
    canonical.extend_from_slice(field);
}

fn decision_name(decision: GateDecision) -> &'static str {
    match decision {
        GateDecision::Approve => "approve",
        GateDecision::RequestChanges => "request_changes",
        GateDecision::Reject => "reject",
    }
}

fn timestamp_bytes(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true)
}
