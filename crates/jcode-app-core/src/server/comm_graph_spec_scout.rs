//! Mandatory, toolless Luna pass over a materialized mission specification.
//!
//! The output is deliberately parsed before the approval gate is built. A model
//! failure, prose response, or structurally invalid object leaves no gate to
//! approve, so a scout can never silently become advisory-only.

use crate::agent::Agent;
use crate::decomposition_materializer::SpecScoutResult;
use crate::server::SessionAgents;
use anyhow::{Context, Result, bail};
use jcode_base::gates::GateOption;

const SPEC_SCOUT_LUNA_MODEL: &str = "gpt-5.6-luna";
const SPEC_SCOUT_EFFORT: &str = "low";

pub(super) async fn run(
    sessions: &SessionAgents,
    coordinator_session_id: &str,
    specification: &str,
) -> Result<SpecScoutResult> {
    if specification.trim().is_empty() {
        bail!("mandatory spec scout requires a non-empty specification");
    }
    let coordinator = sessions
        .read()
        .await
        .get(coordinator_session_id)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!("mandatory spec scout coordinator session is unavailable")
        })?;
    let (provider, registry, working_dir, provider_key, route_api_method) = {
        let coordinator = coordinator.lock().await;
        (
            coordinator.provider_fork(),
            coordinator.registry(),
            coordinator.working_dir().map(str::to_string),
            coordinator.session_provider_key(),
            coordinator.session_route_api_method(),
        )
    };

    let mut scout = Agent::new_context_scout_luna_worker_with_initial_working_dir(
        provider,
        registry,
        working_dir.as_deref(),
    );
    scout.apply_spec_scout_luna_profile();
    if provider_key.is_some() {
        scout.set_session_provider_key(provider_key.clone());
    }
    let model_request = crate::provider::MultiProvider::model_switch_request_for_session_route(
        SPEC_SCOUT_LUNA_MODEL,
        provider_key.as_deref(),
        route_api_method.as_deref(),
    );
    scout
        .set_model(&model_request)
        .with_context(|| "mandatory spec scout could not select gpt-5.6-luna")?;
    if scout.provider_model() != SPEC_SCOUT_LUNA_MODEL {
        bail!(
            "mandatory spec scout selected '{}' instead of '{}', refusing approval",
            scout.provider_model(),
            SPEC_SCOUT_LUNA_MODEL
        );
    }
    scout
        .set_reasoning_effort(SPEC_SCOUT_EFFORT)
        .with_context(|| "mandatory spec scout could not set low effort")?;

    let response = scout.run_once_capture(&prompt(specification)).await?;
    parse(&response)
}

fn prompt(specification: &str) -> String {
    format!(
        "You are the mandatory pre-approval specification scout. Read only the specification below. \
You have no tools and must not infer repository facts. Return exactly one JSON object, with no Markdown \
or prose, matching this schema:\n{{\"contradictions\":[{{\"a\":\"statement A\",\"b\":\"statement B\",\"fact\":\"the fact assigned two values\"}}],\"unstated_defaults\":[{{\"behavior\":\"behavior relied upon but not stated\",\"question\":\"one-line question\"}}]}}\n\nSpecification:\n{specification}"
    )
}

pub(super) fn parse(response: &str) -> Result<SpecScoutResult> {
    let result: SpecScoutResult = serde_json::from_str(response)
        .context("mandatory spec scout must return the required JSON object")?;
    result.validate().map_err(|error| {
        anyhow::anyhow!("mandatory spec scout result failed validation: {error}")
    })?;
    Ok(result)
}

pub(crate) fn gate_options(result: &SpecScoutResult) -> Vec<GateOption> {
    let mut options = vec![
        GateOption {
            label: "Approve".to_string(),
            description: "Authorize only this already-materialized DAG after reviewing the mandatory spec-scout findings.".to_string(),
        },
        GateOption {
            label: "Request changes".to_string(),
            description: "Keep the DAG materialized and request a changed packet or specification.".to_string(),
        },
        GateOption {
            label: "Reject".to_string(),
            description: "Do not authorize this materialized DAG.".to_string(),
        },
    ];
    options.extend(result.contradictions.iter().map(|finding| GateOption {
        label: format!("Resolve contradiction: {}", finding.fact),
        description: format!("\"{}\" conflicts with \"{}\".", finding.a, finding.b),
    }));
    options.extend(result.unstated_defaults.iter().map(|finding| GateOption {
        label: format!("Specify default: {}", finding.behavior),
        description: finding.question.clone(),
    }));
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planted_contradiction_fixture_requires_the_exact_structured_output() {
        let result = parse(
            r#"{"contradictions":[{"a":"conflict key is name","b":"conflict key is qid","fact":"partner conflict key"}],"unstated_defaults":[{"behavior":"deleted rows","question":"What happens to rows deleted upstream?"}]}"#,
        )
        .expect("fixture conforms to the mandatory scout schema");
        assert_eq!(result.contradictions[0].fact, "partner conflict key");
        assert_eq!(result.unstated_defaults[0].behavior, "deleted rows");
        let options = gate_options(&result);
        assert!(
            options
                .iter()
                .any(|option| option.label == "Resolve contradiction: partner conflict key")
        );
        assert!(
            options
                .iter()
                .any(|option| option.description == "What happens to rows deleted upstream?")
        );
    }

    #[test]
    fn missing_or_malformed_scout_output_is_not_accepted() {
        for response in [
            "not json",
            r#"{"contradictions":[]}"#,
            r#"{"contradictions":[{"a":"","b":"B","fact":"F"}],"unstated_defaults":[]}"#,
        ] {
            assert!(parse(response).is_err(), "must fail closed: {response}");
        }
    }
}
