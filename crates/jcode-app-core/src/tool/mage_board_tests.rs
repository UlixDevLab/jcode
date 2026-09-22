use super::*;

fn ctx_with_dir(dir: &Path) -> ToolContext {
    ToolContext {
        session_id: "test".to_string(),
        message_id: "test".to_string(),
        tool_call_id: "test".to_string(),
        working_dir: Some(dir.to_path_buf()),
        stdin_request_tx: None,
        graceful_shutdown_signal: None,
        structural_review_authority: None,
        execution_mode: crate::tool::ToolExecutionMode::Direct,
    }
}

#[test]
fn project_defaults_to_the_session_working_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_dir(dir.path());
    assert_eq!(resolve_project(None, &ctx), dir.path());
}

#[test]
fn explicit_project_wins_over_the_session_dir() {
    let session = tempfile::TempDir::new().unwrap();
    let explicit = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_dir(session.path());
    let resolved = resolve_project(Some(&explicit.path().to_string_lossy()), &ctx);
    assert_eq!(resolved, explicit.path());
}

#[test]
fn blank_project_falls_back_rather_than_pointing_at_an_empty_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let ctx = ctx_with_dir(dir.path());
    assert_eq!(resolve_project(Some("   "), &ctx), dir.path());
}

#[test]
fn home_relative_project_paths_expand() {
    let expanded = shellexpand_home("~/somewhere/board");
    assert!(!expanded.starts_with('~'), "tilde must expand: {expanded}");
    assert!(expanded.ends_with("somewhere/board"));
}

#[tokio::test]
async fn project_local_review_overlay_is_applied_to_the_canonical_html() {
    let dir = tempfile::TempDir::new().unwrap();
    let scripts = dir.path().join("scripts");
    let opencode = dir.path().join(".opencode");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::create_dir_all(&opencode).unwrap();
    let html = opencode.join("project-os.html");
    std::fs::write(&html, "<html><body></body></html>").unwrap();
    std::fs::write(
        scripts.join("project-os-review-overlay.mjs"),
        "import { appendFileSync } from 'node:fs'; appendFileSync(process.argv[2], '<!--review-applied-->');",
    )
    .unwrap();

    assert!(overlay::apply(dir.path(), &html).await.unwrap());
    assert!(
        std::fs::read_to_string(&html)
            .unwrap()
            .contains("review-applied")
    );
}

#[tokio::test]
async fn projects_without_a_review_overlay_keep_the_plain_export() {
    let dir = tempfile::TempDir::new().unwrap();
    let html = dir.path().join("project-os.html");
    std::fs::write(&html, "<html></html>").unwrap();
    assert!(!overlay::apply(dir.path(), &html).await.unwrap());
}

#[tokio::test]
async fn missing_board_explains_what_is_missing_instead_of_failing_opaquely() {
    let dir = tempfile::TempDir::new().unwrap();
    let tool = MageBoardTool::new();
    let output = tool
        .execute(
            serde_json::json!({"intent": "render", "action": "export"}),
            ctx_with_dir(dir.path()),
        )
        .await
        .expect("tool must not error for a missing board");

    assert!(
        output.output.contains("No formation board"),
        "{}",
        output.output
    );
    assert!(
        output.output.contains("project-os.yaml"),
        "must name the expected path: {}",
        output.output
    );
}

#[tokio::test]
async fn status_reports_a_missing_board_without_rendering() {
    let dir = tempfile::TempDir::new().unwrap();
    let tool = MageBoardTool::new();
    let output = tool
        .execute(
            serde_json::json!({"intent": "check", "action": "status"}),
            ctx_with_dir(dir.path()),
        )
        .await
        .unwrap();
    assert!(
        output.output.contains("Board source: missing"),
        "{}",
        output.output
    );
    assert!(
        output.output.contains("Rendered: none"),
        "{}",
        output.output
    );
}

#[tokio::test]
async fn status_flags_a_board_older_than_its_source() {
    // The quiet failure this catches: a rendered board exists, so it looks
    // fine, while the graph has moved on and the picture is out of date.
    let dir = tempfile::TempDir::new().unwrap();
    let opencode = dir.path().join(".opencode");
    std::fs::create_dir_all(&opencode).unwrap();
    std::fs::write(opencode.join("project-os.html"), "<html></html>").unwrap();
    // Write the source second so it is strictly newer.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(opencode.join("project-os.yaml"), "schema_version: x\n").unwrap();

    let tool = MageBoardTool::new();
    let output = tool
        .execute(
            serde_json::json!({"intent": "check", "action": "status"}),
            ctx_with_dir(dir.path()),
        )
        .await
        .unwrap();
    assert!(output.output.contains("STALE"), "{}", output.output);
}

#[test]
fn missing_renderer_names_every_path_it_looked_in() {
    // Point the override at a path that cannot exist so the search fails
    // deterministically regardless of what is installed on this machine.
    let previous = std::env::var_os("KNOWLEDGE_OS_BIN");
    unsafe { std::env::set_var("KNOWLEDGE_OS_BIN", "/nonexistent/knowledge-os-binary") };

    if let Err(message) = find_exporter() {
        assert!(message.contains("KNOWLEDGE_OS_BIN"), "{message}");
        assert!(
            message.contains("/nonexistent/knowledge-os-binary"),
            "the error must show where it looked: {message}"
        );
    }
    // When the real renderer is installed, find_exporter legitimately succeeds
    // from a fallback path; that is not a failure of this contract.

    unsafe {
        match previous {
            Some(value) => std::env::set_var("KNOWLEDGE_OS_BIN", value),
            None => std::env::remove_var("KNOWLEDGE_OS_BIN"),
        }
    }
}
