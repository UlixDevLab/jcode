// McpTool unit tests. Extracted from tool.rs so the production file stays under the 350-line file-size gate.
//
// Test name -> intent:
// - credentialed_mcp_tool_requires_consent_but_local_tool_does_not: the original
//   credentialed-server guard still flags NOTION_API_KEY while leaving a bare server
//   alone. The summary must not echo the secret value.
// - playwright_requires_consent_even_when_model_claims_ui_testing: regression guard
//   that locks in `--isolated` as a precondition for any Playwright carve-out.
// - firecrawl_retrieval_tools_auto_run_even_with_credentials: every name in
//   FIRECRAWL_SAFE_TOOLS auto-runs with and without credential markers.
// - firecrawl_mutating_and_unknown_tools_require_consent: mutating, operational,
//   and unknown Firecrawl tools stay protected regardless of credentials.
// - playwright_read_only_tools_auto_run_in_headless_isolated_mode: every name in
//   PLAYWRIGHT_SAFE_TOOLS auto-runs when the launch is provably
//   --headless + --isolated.
// - playwright_mutating_tools_require_consent_even_in_headless_isolated_mode:
//   click/type/fill/eval/upload/press/select/provider_command and unknown tools
//   stay protected even with the carve-out flags.
// - model_intent_cannot_change_playwright_or_firecrawl_classification: the model
//   display-only `intent` field is never consulted by the policy classifier.
// - firecrawl_safe_tool_set_is_stable_and_documented /
//   playwright_safe_tool_set_is_stable_and_documented: canonical allowlist drift
//   regression guards.
// - headless_isolated_detection_uses_only_official_flags: structural flag
//   vocabulary is pinned to --headless + --isolated only.
// - unknown_server_with_no_credentials_falls_through: unrecognised, non-credentialed
//   servers stay on the existing credentialed-server guard.

use super::*;
use crate::mcp::{McpConfig, McpServerConfig};
use std::collections::HashMap;

fn config(env: HashMap<String, String>) -> Arc<RwLock<McpManager>> {
    let mut config = McpConfig::default();
    config.servers.insert(
        "notion".to_string(),
        McpServerConfig {
            timeout_secs: None,
            command: "notion-mcp".to_string(),
            args: vec![],
            env,
            shared: true,
            transport: None,
            url: None,
            headers: HashMap::new(),
            enabled: None,
            disabled: None,
        },
    );
    Arc::new(RwLock::new(McpManager::with_config(config)))
}

fn tool(env: HashMap<String, String>) -> McpTool {
    McpTool::new(
        "notion".to_string(),
        McpToolDef {
            name: "search".to_string(),
            description: None,
            input_schema: serde_json::json!({"type": "object"}),
        },
        config(env),
    )
}

fn playwright_tool() -> McpTool {
    let mut config = McpConfig::default();
    config.servers.insert(
        "playwright".to_string(),
        McpServerConfig {
            timeout_secs: None,
            command: "npx".to_string(),
            args: vec![
                "@playwright/mcp@latest".to_string(),
                "--headless".to_string(),
            ],
            env: HashMap::new(),
            shared: false,
            transport: None,
            url: None,
            headers: HashMap::new(),
            enabled: None,
            disabled: None,
        },
    );
    McpTool::new(
        "playwright".to_string(),
        McpToolDef {
            name: "browser_navigate".to_string(),
            description: None,
            input_schema: serde_json::json!({"type": "object"}),
        },
        Arc::new(RwLock::new(McpManager::with_config(config))),
    )
}

#[tokio::test]
async fn non_browser_mcp_tools_do_not_require_native_consent() {
    let credentialed = tool(HashMap::from([(
        "NOTION_API_KEY".to_string(),
        "secret".to_string(),
    )]));
    assert!(
        credentialed
            .native_consent_requirement(&serde_json::json!({"query": "tasks"}))
            .await
            .is_none()
    );

    let local = tool(HashMap::new());
    assert!(
        local
            .native_consent_requirement(&serde_json::json!({}))
            .await
            .is_none()
    );
}

#[tokio::test]
async fn playwright_requires_consent_even_when_model_claims_ui_testing() {
    // Regression guard: even after narrowing the consent policy, the
    // default Playwright config used here is `npx @playwright/mcp@latest
    // --headless` (no `--isolated`). That is not a headless-isolated
    // configuration, so any non-bare exception we add later cannot make
    // these calls auto-run. We keep this regression to lock in the
    // "isolated flag is required" precondition.
    let tool = playwright_tool();
    let requirement = tool
        .native_consent_requirement(&serde_json::json!({
            "url": "https://service.example.test",
            "intent": "Research the service dashboard"
        }))
        .await
        .expect("Playwright without --isolated still requires consent");
    assert_eq!(requirement.tool, "playwright");
    assert_eq!(requirement.action, "browser_navigate");
    assert_eq!(requirement.target_summary, "external browser automation");

    let claimed_ui_test = tool
        .native_consent_requirement(&serde_json::json!({
            "url": "http://127.0.0.1:3000",
            "intent": "UI test: verify the modal"
        }))
        .await
        .expect("model-controlled UI-test intent must not bypass consent");
    assert_eq!(claimed_ui_test.tool, "playwright");
    assert_eq!(claimed_ui_test.action, "browser_navigate");

    let mut read_tool = playwright_tool();
    read_tool.tool_def.name = "browser_take_screenshot".to_string();
    assert!(
        read_tool
            .native_consent_requirement(&serde_json::json!({
                "intent": "visual parity screenshot"
            }))
            .await
            .is_some(),
        "Playwright without --isolated still requires consent for read tools"
    );
}

fn firecrawl_tool(name: &str) -> McpTool {
    let mut config = McpConfig::default();
    config.servers.insert(
        "firecrawl".to_string(),
        McpServerConfig {
            timeout_secs: None,
            command: "firecrawl-mcp".to_string(),
            args: vec![],
            env: HashMap::from([("FIRECRAWL_API_KEY".to_string(), "fc-test-key".to_string())]),
            shared: true,
            transport: None,
            url: None,
            headers: HashMap::new(),
            enabled: None,
            disabled: None,
        },
    );
    McpTool::new(
        "firecrawl".to_string(),
        McpToolDef {
            name: name.to_string(),
            description: None,
            input_schema: serde_json::json!({"type": "object"}),
        },
        Arc::new(RwLock::new(McpManager::with_config(config))),
    )
}

fn firecrawl_no_credential_tool(name: &str) -> McpTool {
    let mut config = McpConfig::default();
    config.servers.insert(
        "firecrawl".to_string(),
        McpServerConfig {
            timeout_secs: None,
            command: "firecrawl-mcp".to_string(),
            args: vec![],
            env: HashMap::new(),
            shared: true,
            transport: None,
            url: None,
            headers: HashMap::new(),
            enabled: None,
            disabled: None,
        },
    );
    McpTool::new(
        "firecrawl".to_string(),
        McpToolDef {
            name: name.to_string(),
            description: None,
            input_schema: serde_json::json!({"type": "object"}),
        },
        Arc::new(RwLock::new(McpManager::with_config(config))),
    )
}

#[tokio::test]
async fn firecrawl_retrieval_tools_auto_run_even_with_credentials() {
    for safe_name in [
        "firecrawl_search",
        "firecrawl_scrape",
        "firecrawl_map",
        "firecrawl_parse",
        "firecrawl_crawl",
        "firecrawl_check_crawl_status",
        "firecrawl_agent",
        "firecrawl_agent_status",
        "firecrawl_status",
        "firecrawl_research",
        "firecrawl_developer_search",
    ] {
        let credentialed = firecrawl_tool(safe_name);
        assert!(
            credentialed
                .native_consent_requirement(&serde_json::json!({
                    "url": "https://example.com",
                    "intent": "research"
                }))
                .await
                .is_none(),
            "credentialed Firecrawl retrieval tool `{safe_name}` should auto-run"
        );

        let bare = firecrawl_no_credential_tool(safe_name);
        assert!(
            bare.native_consent_requirement(&serde_json::json!({}))
                .await
                .is_none(),
            "bare Firecrawl retrieval tool `{safe_name}` should auto-run"
        );
    }
}

#[tokio::test]
async fn firecrawl_browser_interaction_requires_consent_but_service_tools_do_not() {
    let interactive = firecrawl_tool("firecrawl_interact");
    let req = interactive
        .native_consent_requirement(&serde_json::json!({"intent": "Interact with a page"}))
        .await
        .expect("Firecrawl browser interaction must require consent");
    assert_eq!(req.tool, "firecrawl");
    assert_eq!(req.action, "firecrawl_interact");
    assert_eq!(req.target_summary, "external browser automation");

    for auto in [
        ("firecrawl_monitor_create", "Monitor a URL"),
        ("firecrawl_monitor_update", "Update monitor"),
        ("firecrawl_monitor_delete", "Delete monitor"),
        ("firecrawl_monitor_run", "Run monitor now"),
        ("firecrawl_feedback", "Submit feedback"),
        ("firecrawl_admin_reindex", "Unknown / future tool"),
        ("firecrawl_billing_portal", "Another unknown tool"),
    ] {
        let (name, intent) = auto;
        let tool = firecrawl_tool(name);
        assert!(
            tool.native_consent_requirement(&serde_json::json!({"intent": intent}))
                .await
                .is_none(),
            "non-browser Firecrawl service tool `{name}` must not need browser consent"
        );
    }
}
