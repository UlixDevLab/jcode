use super::{Tool, ToolContext, ToolOutput};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use jcode_tool_core::{
    StructuralReviewDisposition, StructuralReviewOpen, StructuralReviewSubmission,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

/// Native boundary for an optional server-installed structural review capability.
///
/// Actor identity and reviewer designation never arrive in this tool's input.
pub struct StructuralReviewTool;

impl StructuralReviewTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct StructuralReviewInput {
    action: String,
    owner_session_id: Option<String>,
    reviewer_session_id: Option<String>,
    request_id: Option<String>,
    disposition: Option<String>,
    #[serde(default)]
    refactor_obligations: Vec<String>,
}

fn disposition(value: Option<&str>) -> Result<StructuralReviewDisposition> {
    match value {
        Some("COHESIVE") => Ok(StructuralReviewDisposition::Cohesive),
        Some("REFACTOR_REQUIRED") => Ok(StructuralReviewDisposition::RefactorRequired),
        _ => bail!("structural review disposition must be COHESIVE or REFACTOR_REQUIRED"),
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(root).output()?;
    if !output.status.success() {
        bail!(
            "structural review requires a Git working tree: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn optional_review_candidate(
    working_dir: Option<&Path>,
) -> Result<jcode_tool_core::StructuralReviewCandidate> {
    let root = working_dir.context("structural review requires a project working directory")?;
    let root = std::fs::canonicalize(root)?;
    let baseline = git_output(&root, &["rev-parse", "HEAD"])?;
    let mut paths: Vec<String> = git_output(&root, &["diff", "--name-only", "-z", "HEAD"])?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    paths.extend(
        git_output(&root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned),
    );
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        bail!("structural review needs at least one changed tracked or untracked file");
    }
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(baseline.as_bytes());
    for path in &paths {
        hasher.update(b"\0");
        hasher.update(path.as_bytes());
    }
    Ok(jcode_tool_core::StructuralReviewCandidate {
        project_root: root.to_string_lossy().to_string(),
        cycle_id: 0,
        baseline,
        candidate_paths: paths,
        candidate_digest: format!("{:x}", hasher.finalize()),
        policy_version: "optional-review/v1".to_string(),
        source_changed: true,
        risk_signals: Vec::new(),
    })
}

#[async_trait]
impl Tool for StructuralReviewTool {
    fn name(&self) -> &str {
        "structural_review"
    }

    fn description(&self) -> &str {
        "Open or submit an optional server-authorized independent review for a changed workspace."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "additionalProperties": false,
            "properties": {
                "action": {"type": "string", "enum": ["open", "submit"]},
                "owner_session_id": {"type": "string", "description": "Implementation owner selected by the current coordinator for open only."},
                "reviewer_session_id": {"type": "string", "description": "Distinct live reviewer selected by the current coordinator for open only."},
                "request_id": {"type": "string", "description": "Server-authored request id for submit only."},
                "disposition": {"type": "string", "enum": ["COHESIVE", "REFACTOR_REQUIRED"]},
                "refactor_obligations": {"type": "array", "items": {"type": "string"}}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: StructuralReviewInput = serde_json::from_value(input)?;
        let authority = ctx.structural_review_authority.ok_or_else(|| {
            anyhow::anyhow!("structural review authority is unavailable in this runtime")
        })?;
        match params.action.as_str() {
            "open" => {
                let owner_session_id = params
                    .owner_session_id
                    .ok_or_else(|| anyhow::anyhow!("open requires owner_session_id"))?;
                let reviewer_session_id = params
                    .reviewer_session_id
                    .ok_or_else(|| anyhow::anyhow!("open requires reviewer_session_id"))?;
                let candidate = optional_review_candidate(ctx.working_dir.as_deref())?;
                let receipt = authority
                    .open_structural_review(StructuralReviewOpen {
                        owner_session_id,
                        reviewer_session_id,
                        candidate,
                    })
                    .await?;
                Ok(ToolOutput::new(format!(
                    "Structural review request opened: {}. Only the designated independent reviewer may submit a verdict.",
                    receipt.request_id,
                )).with_metadata(serde_json::to_value(receipt)?))
            }
            "submit" => {
                let request_id = params
                    .request_id
                    .ok_or_else(|| anyhow::anyhow!("submit requires request_id"))?;
                let receipt = authority
                    .submit_structural_review(StructuralReviewSubmission {
                        request_id,
                        disposition: disposition(params.disposition.as_deref())?,
                        refactor_obligations: params.refactor_obligations,
                    })
                    .await?;
                Ok(ToolOutput::new(format!(
                    "Structural review verdict recorded: {}.",
                    match receipt.disposition {
                        StructuralReviewDisposition::Cohesive => "COHESIVE",
                        StructuralReviewDisposition::RefactorRequired => "REFACTOR_REQUIRED",
                    }
                ))
                .with_metadata(serde_json::to_value(receipt)?))
            }
            _ => bail!("structural review action must be open or submit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StructuralReviewTool, Tool};

    #[test]
    fn schema_has_no_model_actor_or_role_field() {
        let schema = StructuralReviewTool::new().parameters_schema();
        assert!(schema["properties"].get("actor_session_id").is_none());
        assert!(schema["properties"].get("role").is_none());
        assert_eq!(
            schema["properties"]["disposition"]["enum"],
            serde_json::json!(["COHESIVE", "REFACTOR_REQUIRED"])
        );
    }
}
