use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const REVIEW_OVERLAY: &str = "scripts/project-os-review-overlay.mjs";
const OVERLAY_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) fn find(project: &Path) -> Option<PathBuf> {
    let candidate = project.join(REVIEW_OVERLAY);
    candidate.is_file().then_some(candidate)
}

pub(super) async fn apply(project: &Path, html: &Path) -> Result<bool, String> {
    let Some(script) = find(project) else {
        return Ok(false);
    };

    let mut command = tokio::process::Command::new("node");
    command
        .arg(&script)
        .arg(html)
        .current_dir(project)
        .stdin(Stdio::null());

    let output = match tokio::time::timeout(OVERLAY_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return Err(format!("failed to run review overlay: {error}")),
        Err(_) => {
            return Err(format!(
                "review overlay timed out after {}s",
                OVERLAY_TIMEOUT.as_secs()
            ));
        }
    };

    if output.status.success() {
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let diagnostics = if !stderr.trim().is_empty() {
        stderr.trim()
    } else if !stdout.trim().is_empty() {
        stdout.trim()
    } else {
        "no diagnostics"
    };
    Err(format!("review overlay failed: {diagnostics}"))
}
