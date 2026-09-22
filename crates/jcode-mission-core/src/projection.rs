use crate::{MissionArtifact, MissionCoverageVerdict, MissionStatus, WaveStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionProjection {
    pub markdown: Vec<u8>,
    pub html: Vec<u8>,
}

pub fn render_projection(artifact: &MissionArtifact) -> MissionProjection {
    let mut markdown = String::new();
    markdown.push_str("# Mission\n\n");
    markdown.push_str(&format!(
        "- Mission ID: `{}`\n",
        artifact.mission_id().as_str()
    ));
    markdown.push_str(&format!(
        "- Revision: `{}`\n",
        artifact.active_revision().revision
    ));
    markdown.push_str(&format!(
        "- State hash: `{}`\n",
        artifact.state_hash().as_str()
    ));
    markdown.push_str(&format!(
        "- Status: `{}`\n\n",
        status_name(artifact.status())
    ));
    markdown.push_str("## Waves\n\n");
    for wave in artifact.waves() {
        markdown.push_str(&format!("### {} ({})\n", wave.id, wave_name(wave.status)));
        for entry in &wave.scope {
            markdown.push_str(&format!(
                "- `{}` · `{}` · {}\n",
                entry.relative_path,
                entry.entity_id,
                if entry.write_set { "write" } else { "read" }
            ));
        }
        markdown.push('\n');
    }
    markdown.push_str("## Evidence\n\n");
    if artifact.evidence().is_empty() {
        markdown.push_str("_No evidence recorded._\n");
    }
    for evidence in artifact.evidence() {
        markdown.push_str(&format!(
            "- {}: {}\n",
            evidence.acceptance_id, evidence.reference
        ));
    }
    let coverage = artifact.verify_requirement_coverage();
    markdown.push_str("## Requirement coverage\n\n");
    markdown.push_str(&format!(
        "- Verdict: `{}`\n",
        match coverage.verdict {
            MissionCoverageVerdict::Accepted => "accepted",
            MissionCoverageVerdict::Blocked => "blocked",
        }
    ));
    for entry in coverage.entries {
        markdown.push_str(&format!(
            "- {} -> {} -> {}\n",
            entry.requirement_id,
            entry.wave_id.as_deref().unwrap_or("unmapped"),
            entry.evidence_record.as_deref().unwrap_or("unmapped"),
        ));
    }
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Mission</title></head><body><pre>",
    );
    html.push_str(&escape_html(&markdown));
    html.push_str("</pre></body></html>\n");
    MissionProjection {
        markdown: markdown.into_bytes(),
        html: html.into_bytes(),
    }
}

fn status_name(status: MissionStatus) -> &'static str {
    match status {
        MissionStatus::Declared => "declared",
        MissionStatus::Approved => "approved",
        MissionStatus::InProgress => "in_progress",
        MissionStatus::Completed => "completed",
    }
}

fn wave_name(status: WaveStatus) -> &'static str {
    match status {
        WaveStatus::Pending => "pending",
        WaveStatus::Active => "active",
        WaveStatus::Completed => "completed",
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
