use super::{Tool, ToolContext, ToolOutput};
use crate::session::{CurrentMissionBrief, Session};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use jcode_tool_core::ACCEPT_LARGE_OUTPUT_KEY;
use serde_json::{Value, json};

/// Root-only surface for the one current brief that guides substantial work.
pub struct MissionBriefTool;

impl MissionBriefTool {
    pub fn new() -> Self {
        Self
    }
}

/// Decode the brief while retaining the closed record used for persistence.
///
/// `intent` and `accept_large_output` are registry-owned fields injected into
/// every public tool definition. They are not part of a mission brief, but a
/// real provider call includes them, so remove only those fields before the
/// closed `CurrentMissionBrief` record is decoded. Other unknown fields remain
/// errors rather than silently becoming persistent context.
fn parse_current_mission_brief(mut input: Value) -> Result<CurrentMissionBrief> {
    let object = input
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mission_brief input must be an object"))?;
    object.remove("intent");
    object.remove(ACCEPT_LARGE_OUTPUT_KEY);

    let brief: CurrentMissionBrief = serde_json::from_value(input)
        .map_err(|error| anyhow::anyhow!("invalid mission_brief input: {error}"))?;
    brief.validate().map_err(|error| anyhow::anyhow!(error))?;
    Ok(brief)
}

#[async_trait]
impl Tool for MissionBriefTool {
    fn name(&self) -> &str {
        "mission_brief"
    }

    fn description(&self) -> &str {
        "Set the root session's current working brief. Use only when a compact outcome, scope boundary, constraints, acceptance observations, autonomous decisions, or meaningful open user decisions will help guide substantial work."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["outcome"],
            "additionalProperties": false,
            "properties": {
                "outcome": {"type": "string"},
                "in_scope": {"type": "array", "items": {"type": "string"}},
                "out_of_scope": {"type": "array", "items": {"type": "string"}},
                "constraints": {"type": "array", "items": {"type": "string"}},
                "acceptance_observations": {"type": "array", "items": {"type": "string"}},
                "autonomous_decisions": {"type": "array", "items": {"type": "string"}},
                "unresolved_user_decisions": {"type": "array", "items": {"type": "string"}}
            }
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let brief = parse_current_mission_brief(input)?;

        let mut session = Session::load(&ctx.session_id).with_context(|| {
            format!(
                "cannot establish mission brief authority for session {}",
                ctx.session_id
            )
        })?;
        if session.parent_id.is_some() {
            bail!("mission_brief requires the root coordinator");
        }
        session.current_mission_brief = Some(brief.clone());
        session.save()?;

        Ok(
            ToolOutput::new("Current mission brief saved for this root session.")
                .with_metadata(json!({"current_mission_brief": brief})),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{MissionBriefTool, Tool};
    use crate::session::Session;
    use crate::tool::{ToolContext, ToolExecutionMode};
    use std::ffi::OsString;
    use std::path::Path;

    struct JcodeHomeGuard {
        previous: Option<OsString>,
    }

    impl JcodeHomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var_os("JCODE_HOME");
            crate::env::set_var("JCODE_HOME", path);
            Self { previous }
        }
    }

    impl Drop for JcodeHomeGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                crate::env::set_var("JCODE_HOME", previous);
            } else {
                crate::env::remove_var("JCODE_HOME");
            }
        }
    }

    fn context(session_id: String) -> ToolContext {
        ToolContext {
            session_id,
            message_id: "mission-brief-test".to_string(),
            tool_call_id: "mission-brief-test".to_string(),
            working_dir: None,
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            structural_review_authority: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    #[test]
    fn schema_exposes_one_compact_current_brief_without_approval_or_revision_fields() {
        let schema = MissionBriefTool::new().parameters_schema();
        assert_eq!(schema["required"], serde_json::json!(["outcome"]));
        assert!(schema["properties"].get("approval_state").is_none());
        assert!(schema["properties"].get("expected_artifact_hash").is_none());
        assert!(schema["properties"].get("created_at").is_none());
    }

    #[test]
    fn public_schema_reserved_fields_are_accepted_without_opening_the_brief_record() {
        let definition = MissionBriefTool::new().to_definition();
        assert!(
            definition.input_schema["properties"]
                .get("intent")
                .is_some()
        );
        assert!(
            definition.input_schema["properties"]
                .get(jcode_tool_core::ACCEPT_LARGE_OUTPUT_KEY)
                .is_some()
        );
        assert_eq!(definition.input_schema["additionalProperties"], false);
    }

    #[tokio::test]
    async fn root_tool_persists_current_brief_and_denies_worker_replacement() {
        let _guard = crate::storage::lock_test_env();
        let home = tempfile::tempdir().expect("temporary JCODE_HOME");
        let _home = JcodeHomeGuard::set(home.path());
        let mut root = Session::create_with_id("root-mission-brief".to_string(), None, None);
        root.save().expect("save root session");
        let mut worker = Session::create_with_id(
            "worker-mission-brief".to_string(),
            Some(root.id.clone()),
            None,
        );
        worker.save().expect("save worker session");

        let input = serde_json::json!({
            "outcome": "Deliver the agreed focused change.",
            "in_scope": ["the current implementation"],
            "out_of_scope": ["unrelated work"],
            "constraints": ["preserve native consent"],
            "acceptance_observations": ["the public tool saves the brief"],
            "autonomous_decisions": ["perform ordinary local implementation"],
            "unresolved_user_decisions": [],
            "intent": "Save the current implementation brief.",
            "accept_large_output": false
        });
        let tool = MissionBriefTool::new();
        let output = tool
            .execute(input.clone(), context(root.id.clone()))
            .await
            .expect("root may save current brief");
        assert_eq!(
            output.metadata.as_ref().expect("brief metadata")["current_mission_brief"]["outcome"],
            "Deliver the agreed focused change."
        );
        assert_eq!(
            Session::load(&root.id)
                .expect("reload root")
                .current_mission_brief
                .expect("persisted brief")
                .outcome,
            "Deliver the agreed focused change."
        );
        assert!(
            tool.execute(input, context(worker.id.clone()))
                .await
                .expect_err("worker cannot replace root brief")
                .to_string()
                .contains("root coordinator")
        );
        assert!(
            Session::load(&worker.id)
                .expect("reload worker")
                .current_mission_brief
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejected_extra_brief_field_names_the_input_problem() {
        let error = MissionBriefTool::new()
            .execute(
                serde_json::json!({
                    "outcome": "Deliver the agreed focused change.",
                    "intent": "Exercise public input parsing.",
                    "accept_large_output": false,
                    "unexpected": true
                }),
                context("missing-session-is-fine-before-authority-check".to_string()),
            )
            .await
            .expect_err("closed mission brief rejects unrelated fields");
        assert!(error.to_string().contains("invalid mission_brief input"));
        assert!(error.to_string().contains("unexpected"));
    }
}
