//! Render a project's formation board to a self-contained, clickable HTML file.
//!
//! Step 1 of the Mage/Knowledge OS plan. It deliberately does the smallest
//! useful thing: take a board that already exists (`.opencode/project-os.yaml`),
//! run the established Knowledge OS exporter, and hand back the path.
//!
//! Why shell out instead of rendering here: Knowledge OS is a separate,
//! maintained project whose renderer already produces the interactive
//! node/edge boards this operator works from. Reimplementing that inside jcode
//! would fork a working renderer to gain nothing at this step. jcode's job is
//! to own formation state and make the board reachable from a session.
//!
//! Validation runs before export because the exporter is not the right place to
//! learn your graph is malformed, and a failed export must never leave a
//! half-written board where a good one used to be.

use super::{Tool, ToolContext, ToolOutput};
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "mage_board_overlay.rs"]
mod overlay;

/// Hard ceiling for a single exporter run. The exporter is normally a few
/// seconds; anything beyond this is stuck, and a stuck tool call is a failure
/// mode worth failing loudly rather than waiting out.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(120);

/// Where a project keeps its board, relative to the project root.
const BOARD_YAML: &str = ".opencode/project-os.yaml";
const BOARD_HTML: &str = ".opencode/project-os.html";

pub struct MageBoardTool;

impl MageBoardTool {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Deserialize)]
struct MageBoardInput {
    #[serde(default = "default_action")]
    action: String,
    /// Project root. Defaults to the session working directory.
    #[serde(default)]
    project: Option<String>,
    /// Open the rendered board after a successful export.
    #[serde(default = "default_true")]
    open: bool,
}

fn default_action() -> String {
    "export".to_string()
}

fn default_true() -> bool {
    true
}

/// Locate the Knowledge OS CLI.
///
/// It is a separate repository rather than a bundled asset, so this looks in
/// the configured location first and then the conventional checkout path. When
/// it is missing we say exactly what is missing and where we looked, because
/// "export failed" with no path is the least actionable error possible.
fn find_exporter() -> std::result::Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(configured) = std::env::var("KNOWLEDGE_OS_BIN") {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(home) = dirs::home_dir() {
        candidates.push(
            home.join("Documents")
                .join("LeGrin.tech")
                .join("knowledge-os")
                .join("bin")
                .join("knowledge-os"),
        );
        candidates.push(home.join("knowledge-os").join("bin").join("knowledge-os"));
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(format!(
        "Knowledge OS renderer not found. Looked in:\n{}\n\n\
         Set KNOWLEDGE_OS_BIN to the `bin/knowledge-os` executable, or clone the \
         knowledge-os project to one of those paths.",
        candidates
            .iter()
            .map(|path| format!("  {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn resolve_project(input: Option<&str>, ctx: &ToolContext) -> PathBuf {
    if let Some(project) = input.filter(|value| !value.trim().is_empty()) {
        return PathBuf::from(shellexpand_home(project));
    }
    ctx.working_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
}

fn shellexpand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().into_owned();
    }
    path.to_string()
}

/// Run one Knowledge OS subcommand against `project`.
async fn run_exporter(
    exporter: &Path,
    subcommand: &str,
    project: &Path,
) -> std::result::Result<std::process::Output, String> {
    let mut command = tokio::process::Command::new("node");
    command
        .arg(exporter)
        .arg(subcommand)
        .arg("--project")
        .arg(project)
        .current_dir(project)
        .stdin(std::process::Stdio::null());

    match tokio::time::timeout(EXPORT_TIMEOUT, command.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(format!("failed to run the renderer: {error}")),
        Err(_) => Err(format!(
            "renderer timed out after {}s running `{subcommand}`",
            EXPORT_TIMEOUT.as_secs()
        )),
    }
}

fn describe_output(output: &std::process::Output) -> String {
    let mut parts = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        parts.push(stdout.trim().to_string());
    }
    if !stderr.trim().is_empty() {
        parts.push(stderr.trim().to_string());
    }
    if parts.is_empty() {
        "(no diagnostics)".to_string()
    } else {
        parts.join("\n")
    }
}

#[async_trait]
impl Tool for MageBoardTool {
    fn name(&self) -> &str {
        "mage_board"
    }

    fn description(&self) -> &str {
        "Render a project's formation board (.opencode/project-os.yaml) into a self-contained, \
         clickable HTML graph and open it. Use when the user wants to see the plan, architecture, \
         workflows, or data model as a diagram rather than prose."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "intent": super::intent_schema_property(),
                "action": {
                    "type": "string",
                    "enum": ["export", "validate", "status"],
                    "description": "export renders and opens the board; validate checks the graph without writing; status reports whether a board exists and how fresh it is."
                },
                "project": {
                    "type": "string",
                    "description": "Project root. Defaults to the session working directory."
                },
                "open": {
                    "type": "boolean",
                    "description": "Open the board after a successful export. Default true."
                }
            },
            "required": ["intent"]
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let params: MageBoardInput = serde_json::from_value(input)?;
        let project = resolve_project(params.project.as_deref(), &ctx);
        let yaml = project.join(BOARD_YAML);
        let html = project.join(BOARD_HTML);

        if params.action == "status" {
            let mut lines = vec![format!("Project: {}", project.display())];
            lines.push(if yaml.is_file() {
                format!("Board source: {}", yaml.display())
            } else {
                format!("Board source: missing ({})", yaml.display())
            });
            if html.is_file() {
                let rendered = std::fs::metadata(&html)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339())
                    .unwrap_or_else(|| "unknown".to_string());
                lines.push(format!("Rendered: {} (at {rendered})", html.display()));
                // A board older than its source is the common trap: it looks
                // present, so nobody regenerates it, and it quietly shows an
                // old plan.
                if let (Ok(source), Ok(rendered)) = (
                    std::fs::metadata(&yaml).and_then(|m| m.modified()),
                    std::fs::metadata(&html).and_then(|m| m.modified()),
                ) && source > rendered
                {
                    lines.push(
                        "STALE: the source graph is newer than the rendered board. Re-export."
                            .to_string(),
                    );
                }
            } else {
                lines.push("Rendered: none".to_string());
            }
            return Ok(ToolOutput::new(lines.join("\n")));
        }

        if !yaml.is_file() {
            return Ok(ToolOutput::new(format!(
                "No formation board at {}.\n\n\
                 A board is authored as a graph of nodes and edges, not prose. Create it first, \
                 then render it here.",
                yaml.display()
            )));
        }

        let exporter = match find_exporter() {
            Ok(path) => path,
            Err(message) => return Ok(ToolOutput::new(message)),
        };

        // Validate before writing anything. A malformed graph should report its
        // own diagnostics, not a rendering failure downstream.
        let validation = match run_exporter(&exporter, "validate", &project).await {
            Ok(output) => output,
            Err(message) => return Ok(ToolOutput::new(message)),
        };
        if !validation.status.success() {
            return Ok(ToolOutput::new(format!(
                "Board did not validate; nothing was written.\n\n{}",
                describe_output(&validation)
            )));
        }

        if params.action == "validate" {
            return Ok(ToolOutput::new(format!(
                "Board is valid: {}\n\n{}",
                yaml.display(),
                describe_output(&validation)
            )));
        }

        let export = match run_exporter(&exporter, "export", &project).await {
            Ok(output) => output,
            Err(message) => return Ok(ToolOutput::new(message)),
        };
        if !export.status.success() {
            return Ok(ToolOutput::new(format!(
                "Export failed.\n\n{}",
                describe_output(&export)
            )));
        }

        if !html.is_file() {
            return Ok(ToolOutput::new(format!(
                "Renderer reported success but {} does not exist.\n\n{}",
                html.display(),
                describe_output(&export)
            )));
        }

        let overlay_applied = match overlay::apply(&project, &html).await {
            Ok(applied) => applied,
            Err(message) => {
                return Ok(ToolOutput::new(format!(
                    "Board rendered, but its project review overlay could not be applied.\n\n{message}"
                )));
            }
        };

        let size = std::fs::metadata(&html).map(|meta| meta.len()).unwrap_or(0);
        let mut lines = vec![
            format!("Rendered board: {}", html.display()),
            format!("Size: {:.1} MB (self-contained)", size as f64 / 1_048_576.0),
        ];
        if overlay_applied {
            lines.push("Review overlay: applied to the canonical HTML.".to_string());
        }

        if params.open {
            match open::that_detached(&html) {
                Ok(()) => lines.push("Opened in your browser.".to_string()),
                Err(error) => lines.push(format!("Could not open it automatically: {error}")),
            }
        }

        let diagnostics = describe_output(&export);
        if diagnostics != "(no diagnostics)" {
            lines.push(String::new());
            lines.push(diagnostics);
        }

        Ok(ToolOutput::new(lines.join("\n")))
    }
}

#[cfg(test)]
#[path = "mage_board_tests.rs"]
mod mage_board_tests;
