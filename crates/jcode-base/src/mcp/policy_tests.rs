// Native-consent policy unit tests. Extracted from policy.rs so the
// production classifier stays focused and under the file-size gate.
//
// Test name -> classification intent:
// - `firecrawl_safe_tools_are_recognised`: canonical allowlist membership,
//   including `firecrawl_agent_status`.
// - `playwright_safe_tools_are_recognised`: read-only/navigation membership.
// - `broadening_flags_disqualify_headless_isolated_config`: any broadening
//   flag (--user-data-dir, --connect-over-cdp, ...) overrides the
//   --headless + --isolated carve-out, even with both flags present.
// - `classify_firecrawl_uses_safe_list_only`: auto-run carve-out is
//   limited to retrieval; mutating and unknown tools stay protected.
// - `classify_playwright_requires_clean_headless_isolated`: protected when
//   either flag is missing; auto-run only when both are present and no
//   credential markers exist.
// - `classify_playwright_refuses_when_credentials_present`: carve-out
//   collapses when env or headers carry credential markers.
// - `classify_unknown_server_falls_through`: unrecognised servers stay on
//   the existing credentialed-server guard.

use super::*;
use std::collections::HashMap;

fn server(args: Vec<String>, env: HashMap<String, String>) -> McpServerConfig {
    McpServerConfig {
        timeout_secs: None,
        command: "npx".to_string(),
        args,
        env,
        shared: false,
        transport: None,
        url: None,
        headers: HashMap::new(),
        enabled: None,
        disabled: None,
    }
}

#[test]
fn firecrawl_safe_tools_are_recognised() {
    for name in FIRECRAWL_SAFE_TOOLS {
        assert!(is_firecrawl_safe_tool(name));
        assert!(is_recognized_server("firecrawl"));
        assert!(is_recognized_server("Firecrawl"));
        assert!(is_recognized_server("  firecrawl  "));
    }
    assert!(!is_firecrawl_safe_tool("firecrawl_interact"));
    assert!(!is_firecrawl_safe_tool("firecrawl_monitor_create"));
    assert!(!is_recognized_server("notion"));
}

#[test]
fn playwright_safe_tools_are_recognised() {
    for name in PLAYWRIGHT_SAFE_TOOLS {
        assert!(is_playwright_safe_tool(name));
    }
    assert!(!is_playwright_safe_tool("browser_click"));
    assert!(!is_playwright_safe_tool("browser_evaluate"));
}

#[test]
fn broadening_flags_disqualify_headless_isolated_config() {
    // Broadening flag → not headless-isolated even with both flags present.
    let cfg = server(
        vec![
            "@playwright/mcp@latest".into(),
            "--headless".into(),
            "--isolated".into(),
            "--user-data-dir".into(),
            "/tmp/profile".into(),
        ],
        HashMap::new(),
    );
    assert!(!is_headless_isolated_config(&cfg));
}

#[test]
fn classify_firecrawl_protects_browser_interaction_only() {
    let cfg = server(vec![], HashMap::new());
    for name in FIRECRAWL_SAFE_TOOLS {
        assert_eq!(
            classify("firecrawl", name, Some(&cfg)),
            Classification::AutoSafe
        );
    }
    // Browser interaction stays protected; non-browser service and unknown
    // calls use the normal server path without browser consent.
    assert_eq!(
        classify("firecrawl", "firecrawl_interact", Some(&cfg)),
        Classification::Protected
    );
    assert_eq!(
        classify("firecrawl", "firecrawl_future_unknown", Some(&cfg)),
        Classification::ServerDefault
    );
    // case-insensitive server name
    assert_eq!(
        classify("Firecrawl", "firecrawl_scrape", Some(&cfg)),
        Classification::AutoSafe
    );
}

#[test]
fn classify_playwright_requires_clean_headless_isolated() {
    let headless_only = server(
        vec!["@playwright/mcp@latest".into(), "--headless".into()],
        HashMap::new(),
    );
    assert_eq!(
        classify("playwright", "browser_navigate", Some(&headless_only)),
        Classification::Protected
    );
    let isolated_only = server(
        vec!["@playwright/mcp@latest".into(), "--isolated".into()],
        HashMap::new(),
    );
    assert_eq!(
        classify("playwright", "browser_navigate", Some(&isolated_only)),
        Classification::Protected
    );
    let headless_isolated = server(
        vec![
            "@playwright/mcp@latest".into(),
            "--headless".into(),
            "--isolated".into(),
        ],
        HashMap::new(),
    );
    for name in PLAYWRIGHT_SAFE_TOOLS {
        assert_eq!(
            classify("playwright", name, Some(&headless_isolated)),
            Classification::AutoSafe
        );
    }
    for name in [
        "browser_click",
        "browser_type",
        "browser_fill",
        "browser_evaluate",
        "browser_upload",
        "browser_press",
        "browser_select_option",
        "browser_provider_command",
        "browser_unknown_future_tool",
    ] {
        assert_eq!(
            classify("playwright", name, Some(&headless_isolated)),
            Classification::Protected
        );
    }
}

#[test]
fn classify_playwright_refuses_when_credentials_present() {
    let mut env = HashMap::new();
    env.insert("PLAYWRIGHT_API_TOKEN".to_string(), "secret".to_string());
    let cfg = server(
        vec![
            "@playwright/mcp@latest".into(),
            "--headless".into(),
            "--isolated".into(),
        ],
        env,
    );
    assert_eq!(
        classify("playwright", "browser_navigate", Some(&cfg)),
        Classification::Protected
    );
}

#[test]
fn classify_unknown_server_falls_through() {
    let cfg = server(vec![], HashMap::new());
    assert_eq!(
        classify("notion", "search", Some(&cfg)),
        Classification::ServerDefault
    );
}

#[test]
fn firecrawl_agent_status_is_a_safe_retrieval_tool() {
    // Regression guard: the architecture gate added `firecrawl_agent_status`
    // to the canonical retrieval set. If a future edit removes it, this
    // test must fail loudly rather than letting the carve-out silently
    // narrow again.
    assert!(is_firecrawl_safe_tool("firecrawl_agent_status"));
    assert!(FIRECRAWL_SAFE_TOOLS.contains(&"firecrawl_agent_status"));
}
