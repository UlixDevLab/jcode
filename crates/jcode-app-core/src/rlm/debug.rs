use anyhow::{Result, bail};

use super::{CounterfactualSelection, manifest::load_session_manifests};

/// Run the intentionally small, read-only `jcode-rlm rlm-manifest` command.
/// It accepts `list SESSION_ID` and `render SESSION_ID CALL_ID` only.
pub fn run_manifest_command(args: impl IntoIterator<Item = String>) -> Option<Result<()>> {
    let args: Vec<_> = args.into_iter().collect();
    if args.first().map(String::as_str) != Some("rlm-manifest") {
        return None;
    }
    Some(run(args.get(1..).unwrap_or_default()))
}

fn run(args: &[String]) -> Result<()> {
    if !super::launcher_identity().is_rlm() {
        bail!("rlm-manifest is available only through the jcode-rlm launcher")
    }
    match args {
        [action, session_id] if action == "list" => list(session_id),
        [action, session_id, call_id] if action == "render" => render(session_id, call_id),
        _ => bail!("usage: jcode-rlm rlm-manifest <list SESSION_ID | render SESSION_ID CALL_ID>"),
    }
}

fn list(session_id: &str) -> Result<()> {
    let manifests = load_session_manifests(session_id)?;
    if manifests.is_empty() {
        bail!("no redacted RLM manifests found for session {session_id}")
    }
    for manifest in manifests {
        println!(
            "{} flow={} provider_tokens={} selected={} excluded={}",
            manifest.call_id,
            manifest.flow,
            manifest.provider_token_estimate,
            manifest.counterfactual.included.len(),
            manifest.counterfactual.excluded.len(),
        );
    }
    Ok(())
}

fn render(session_id: &str, call_id: &str) -> Result<()> {
    let manifest = load_session_manifests(session_id)?
        .into_iter()
        .find(|manifest| manifest.call_id == call_id)
        .ok_or_else(|| {
            anyhow::anyhow!("no redacted RLM manifest {call_id} for session {session_id}")
        })?;
    print!("{}", render_selection(&manifest.counterfactual));
    Ok(())
}

fn render_selection(selection: &CounterfactualSelection) -> String {
    let mut output = format!(
        "counterfactual tokens={}/{} eligible={} selected={} budget_excluded={} labels={}\n",
        selection.token_estimate,
        selection.token_budget,
        selection.recall.eligible,
        selection.recall.selected,
        selection.recall.excluded_for_budget,
        selection.minimal_evidence_labels.join(","),
    );
    for (heading, entries) in [
        ("included", &selection.included),
        ("excluded", &selection.excluded),
    ] {
        output.push_str(heading);
        output.push_str(":\n");
        for entry in entries {
            output.push_str(&format!(
                "  {} reason={} tokens={} hash={}\n",
                entry.id, entry.reason, entry.token_estimate, entry.content_hash
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use crate::message::Message;

    use super::*;

    #[test]
    fn rendering_never_exposes_message_payloads() {
        let secret = "fixture-secret-never-render";
        let selection = super::super::select_counterfactual(&[Message::user(secret)], 1_000);
        let rendered = render_selection(&selection);
        assert!(!rendered.contains(secret));
        assert!(rendered.contains("hash="));
    }

    #[test]
    fn ordinary_binary_manifest_command_fails_closed() {
        assert!(
            run_manifest_command(["rlm-manifest".into(), "list".into(), "session".into()])
                .expect("recognized")
                .is_err()
        );
    }
}
