use super::super::consent::request_batch_grant_approval;
use super::super::{Tool, ToolContext, ToolOutput};
use super::{BatchGrantManifest, BatchGrantStore, registered_scope};
use anyhow::{Result, bail};
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) struct ConsentGrantTool;

impl ConsentGrantTool {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct ConsentInput {
    action: String,
    registration: Option<String>,
    grant_id: Option<String>,
}

#[async_trait]
impl Tool for ConsentGrantTool {
    fn name(&self) -> &str {
        "consent"
    }

    fn description(&self) -> &str {
        "List or revoke durable user-approved batch grants. Grant proposals require a fixed code registration and a native user approval. No service mutation is registered in this release."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["propose", "list", "revoke"],
                    "description": "propose only selects a fixed code registration. It never accepts model-provided provider, principal, action, or resource scope."
                },
                "registration": {
                    "type": "string",
                    "description": "Fixed service registration identifier. No registrations are available in M2."
                },
                "grant_id": {"type": "string", "description": "Grant ID returned by list, required to revoke."}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: ConsentInput = serde_json::from_value(input)?;
        match input.action.as_str() {
            "propose" => {
                let registration = input.registration.as_deref().unwrap_or_default();
                let scope = registered_scope(registration).ok_or_else(|| {
                    anyhow::anyhow!(
                        "no durable batch-grant scope is registered for this operation; M2 intentionally registers no Gmail, MCP, browser, shell, filesystem, desktop, or unknown mutation"
                    )
                })?;
                let manifest = BatchGrantManifest::from_registered_scope(&scope)?;
                let approval = request_batch_grant_approval(&manifest, &ctx).await?;
                let grant = BatchGrantStore::local()?.create_after_native_approval(
                    approval,
                    &ctx.session_id,
                    manifest,
                )?;
                Ok(ToolOutput::new(serde_json::to_string_pretty(&grant)?))
            }
            "list" => {
                let grants = BatchGrantStore::local()?.list(&ctx.session_id)?;
                Ok(ToolOutput::new(serde_json::to_string_pretty(&grants)?))
            }
            "revoke" => {
                let grant_id = input
                    .grant_id
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("grant_id is required to revoke a batch grant")
                    })?;
                BatchGrantStore::local()?.revoke(&ctx.session_id, grant_id, Utc::now())?;
                Ok(ToolOutput::new(format!("Revoked batch grant {grant_id}.")))
            }
            _ => bail!("invalid consent action; expected propose, list, or revoke"),
        }
    }
}
