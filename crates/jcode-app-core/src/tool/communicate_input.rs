//! Typed input and pure request construction for the swarm tool.

use crate::plan::PlanItem;
use crate::protocol::{CommDeliveryMode, Request, UntrustedDispatchIntent};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Deserialize)]
pub(super) struct CommunicateInput {
    pub(super) action: String,
    #[serde(default)]
    pub(super) key: Option<String>,
    #[serde(default)]
    pub(super) value: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
    #[serde(default)]
    pub(super) to_session: Option<String>,
    #[serde(default)]
    pub(super) channel: Option<String>,
    #[serde(default)]
    pub(super) proposer_session: Option<String>,
    #[serde(default)]
    pub(super) reason: Option<String>,
    #[serde(default)]
    pub(super) target_session: Option<String>,
    #[serde(default)]
    pub(super) role: Option<String>,
    #[serde(default)]
    pub(super) working_dir: Option<String>,
    #[serde(default)]
    pub(super) initial_message: Option<String>,
    #[serde(default)]
    pub(super) prompt: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) task_id: Option<String>,
    #[serde(default)]
    pub(super) spawn_if_needed: Option<bool>,
    #[serde(default)]
    pub(super) prefer_spawn: Option<bool>,
    #[serde(default)]
    pub(super) plan_items: Option<Vec<PlanItem>>,
    #[serde(default)]
    pub(super) node_id: Option<String>,
    #[serde(default)]
    pub(super) gate_id: Option<String>,
    #[serde(default)]
    pub(super) nodes: Option<Vec<crate::protocol::TaskGraphNodeSpec>>,
    /// Handoff artifact (object) for complete_node.
    #[serde(default)]
    pub(super) artifact: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) target_status: Option<Vec<String>>,
    #[serde(default)]
    pub(super) session_ids: Option<Vec<String>>,
    #[serde(default)]
    pub(super) mode: Option<String>,
    #[serde(default)]
    pub(super) timeout_minutes: Option<u64>,
    #[serde(default)]
    pub(super) wake: Option<bool>,
    #[serde(default)]
    pub(super) background: Option<bool>,
    #[serde(default)]
    pub(super) notify: Option<bool>,
    #[serde(default)]
    pub(super) delivery: Option<CommDeliveryMode>,
    #[serde(default)]
    pub(super) concurrency_limit: Option<usize>,
    #[serde(default)]
    pub(super) force: Option<bool>,
    #[serde(default)]
    pub(super) retain_agents: Option<bool>,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) validation: Option<String>,
    #[serde(default)]
    pub(super) follow_up: Option<String>,
    #[serde(default)]
    pub(super) spawn_mode: Option<String>,
    /// One-line summary shown collapsed in the recipient's UI for long
    /// message/report bodies. Required when the body exceeds the collapse
    /// threshold.
    #[serde(default)]
    pub(super) tldr: Option<String>,
    /// Per-spawn model override for spawn/assign_task/assign_next/run_plan
    /// spawns. Takes precedence over agents.swarm_model config.
    #[serde(default)]
    pub(super) model: Option<String>,
    /// Reasoning effort for spawned agents (none|minimal|low|medium|high|xhigh|max).
    #[serde(default)]
    pub(super) effort: Option<String>,
    /// Short human-readable label for a spawned agent shown in swarm UI.
    /// Required and nonblank for the explicit `spawn` action.
    #[serde(default)]
    pub(super) label: Option<String>,
    /// Model-declared target/profile/scope request with no authorization
    /// material. It is optional so existing swarm calls stay wire-compatible.
    #[serde(default)]
    pub(super) dispatch_intent: Option<UntrustedDispatchIntent>,
}

impl CommunicateInput {
    pub(super) fn spawn_initial_message(&self) -> Option<String> {
        self.initial_message
            .as_ref()
            .filter(|message| !message.trim().is_empty())
            .cloned()
            .or_else(|| {
                self.prompt
                    .as_ref()
                    .filter(|prompt| !prompt.trim().is_empty())
                    .cloned()
            })
    }

    pub(super) fn required_spawn_label(&self) -> anyhow::Result<String> {
        let label = self
            .label
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("'label' is required for spawn action"))?
            .trim();
        if label.is_empty() {
            return Err(anyhow::anyhow!(
                "'label' must not be blank for spawn action"
            ));
        }
        Ok(label.to_string())
    }

    /// Produce the exact inert spawn request used by direct spawn and all
    /// assignment fallbacks. The mission binding remains model-inaccessible.
    pub(super) fn build_spawn_request(
        &self,
        session_id: &str,
        initial_message: Option<String>,
        request_nonce: Option<String>,
        label: Option<String>,
    ) -> anyhow::Result<Request> {
        let dispatch_intent = self.dispatch_intent.clone();
        if let Some(intent) = &dispatch_intent {
            intent
                .validate()
                .map_err(|reason| anyhow::anyhow!("invalid dispatch_intent: {reason}"))?;
        }
        Ok(Request::CommSpawn {
            id: super::REQUEST_ID,
            session_id: session_id.to_string(),
            working_dir: self.working_dir.clone(),
            initial_message,
            request_nonce,
            spawn_mode: self.spawn_mode.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            label,
            mission_binding: None,
            dispatch_intent,
        })
    }
}

/// JSON schema advertised to the model for strictly untrusted spawn intent.
pub(super) fn dispatch_intent_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "Optional untrusted dispatch preference only. Never provides mission authority, identity, revision, approval, or digests.",
        "properties": {
            "wave_id": { "type": "string" },
            "task_id": { "type": "string" },
            "role": { "type": "string" },
            "profile": { "type": "string" },
            "requested_read_scopes": dispatch_scope_schema(),
            "requested_write_scopes": dispatch_scope_schema()
        }
    })
}

fn dispatch_scope_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["path", "entity_id"],
            "properties": {
                "path": { "type": "string", "minLength": 1 },
                "entity_id": { "type": "string", "minLength": 1 }
            }
        }
    })
}
