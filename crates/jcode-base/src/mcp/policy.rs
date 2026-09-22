//! Native-consent policy for MCP tool invocations.
//!
//! This module narrows the set of MCP tool calls that can run without
//! one-shot native user consent. The previous blanket policy required
//! consent for every Playwright call and every credentialed MCP server,
//! which made safe local/dev and public read-only automation noisy without
//! buying real protection.
//!
//! The narrow policy keeps central enforcement in the Registry and treats
//! the model's display-only `intent` field as untrusted. It explicitly
//! recognises two first-party server surfaces:
//!
//! * **Firecrawl retrieval/read-only tools** auto-run even when the server
//!   has credential markers (`FIRECRAWL_API_KEY`). This is an explicit
//!   first-party exception for public read-only scraping tools; mutating
//!   browser interaction (`firecrawl_interact`) remains protected. Monitor,
//!   feedback, billing, and unknown non-browser service tools do not require a
//!   browser-consent popup.
//!
//! * **Playwright read-only observation/navigation tools** auto-run when
//!   the launch config is provably `--headless` + `--isolated` and carries
//!   no credential markers. Mutating surfaces (`browser_click`,
//!   `browser_type`, `browser_fill`, `browser_evaluate`, `browser_upload`,
//!   `browser_press`, `browser_select_option`,
//!   `browser_provider_command`) and any unknown tool remain protected.
//!
//! `--isolated` only proves an ephemeral browser profile; it does **not**
//! prove loopback-only URL restriction. jcode therefore refuses to invent
//! a "trusted local server that auto-runs all UI-test calls": full local
//! interactive Playwright use goes through the project's own Playwright
//! CLI/tests, where the user (or CI) is the actual principal. See the
//! `loopback_restricted_at_launch` documentation for the conditions under
//! which that could change.

use crate::mcp::protocol::McpServerConfig;

/// The Firecrawl tool names that auto-run even on a credentialed server.
///
/// This is the canonical allow-list for the public read-only Firecrawl
/// retrieval API. Mutating and operational tools are deliberately not
/// listed; see [`is_firecrawl_safe_tool`].
pub const FIRECRAWL_SAFE_TOOLS: &[&str] = &[
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
];

/// The Playwright tool names that auto-run when the launch config is
/// headless + isolated (see [`is_headless_isolated_config`]) and has no
/// credential markers.
pub const PLAYWRIGHT_SAFE_TOOLS: &[&str] = &[
    "browser_navigate",
    "browser_navigate_back",
    "browser_navigate_forward",
    "browser_take_screenshot",
    "browser_snapshot",
    "browser_console_messages",
    "browser_wait_for",
    "browser_tabs_new",
    "browser_tabs_select",
    "browser_tabs_close",
    "browser_close",
    "browser_resize",
];

/// Whether `tool_name` is on the Firecrawl safe (read-only retrieval) list.
pub fn is_firecrawl_safe_tool(tool_name: &str) -> bool {
    FIRECRAWL_SAFE_TOOLS.contains(&tool_name)
}

/// Firecrawl surface that drives a real external browser session.
pub fn is_firecrawl_browser_tool(tool_name: &str) -> bool {
    tool_name == "firecrawl_interact"
}

/// Whether `tool_name` is on the Playwright read-only/navigation list.
pub fn is_playwright_safe_tool(tool_name: &str) -> bool {
    PLAYWRIGHT_SAFE_TOOLS.contains(&tool_name)
}

/// Whether the configured Playwright launch is structurally
/// `--headless` + `--isolated` (and nothing else that would broaden the
/// surface, such as `--executable-path`, `--user-data-dir`,
/// `--connect-over-cdp`, or any remote URL flag).
///
/// Both flags are required. The combination is necessary but not
/// sufficient: the caller must additionally check
/// [`McpServerConfig::has_credential_markers`] returns `false`. Together
/// they prove "ephemeral, headless browser instance"; they do **not**
/// prove loopback-only URL restriction.
pub fn is_headless_isolated_config(config: &McpServerConfig) -> bool {
    let mut has_headless = false;
    let mut has_isolated = false;
    for arg in &config.args {
        if arg == "--headless" {
            has_headless = true;
        } else if arg == "--isolated" {
            has_isolated = true;
        } else if is_broadening_playwright_flag(arg) {
            // Any flag that points the server at a persistent profile, a
            // remote browser, or a specific binary disqualifies the
            // launch from the auto-run carve-out.
            return false;
        }
    }
    has_headless && has_isolated
}

/// Flags that broaden the Playwright surface beyond the
/// `--headless` + `--isolated` minimum. Each one is paired with the
/// reason it disqualifies the launch from the auto-run carve-out.
fn is_broadening_playwright_flag(arg: &str) -> bool {
    matches!(
        arg,
        // Custom browser binary: not necessarily the same as the
        // chromium bundled with the MCP server.
        "--executable-path"
        // Persistent profile directory: not ephemeral.
        | "--user-data-dir"
        // Connect to an existing Chrome instance (potentially with the
        // user's logged-in state).
        | "--connect-over-cdp"
        | "--cdp-endpoint"
        // Remote browser service endpoint.
        | "--browser-service"
        // Allow origins other than the loopback default.
        | "--allowed-origins"
        | "--blocked-origins"
        | "--ignore-https-errors"
        // Save/load storage state across runs.
        | "--storage-state"
    )
}

/// Whether `server_name` matches a server surface this policy module
/// understands. The match is case-insensitive and ignores surrounding
/// whitespace, mirroring the upstream `eq_ignore_ascii_case` checks.
pub fn is_recognized_server(server_name: &str) -> bool {
    let trimmed = server_name.trim();
    trimmed.eq_ignore_ascii_case("firecrawl") || trimmed.eq_ignore_ascii_case("playwright")
}

/// Classify an MCP tool call for the narrow consent policy.
///
/// * `AutoSafe` — auto-runs without one-shot native consent.
/// * `ServerDefault` — fall back to the existing server-level rules
///   (credential markers, etc.).
/// * `Protected` — always requires one-shot native consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// The call is provably read-only/observational and the launch config
    /// proves the runtime surface needed for the carve-out. Auto-runs.
    AutoSafe,
    /// The call is not covered by this policy. The caller should fall
    /// back to its existing server-level rules (e.g. the
    /// credentialed-server guard).
    ServerDefault,
    /// The call is on a known mutating/operational/unknown surface and
    /// must request one-shot native consent regardless of credentials.
    Protected,
}

/// Classify an MCP tool call using only the recognized server allowlists
/// and structural launch flags. Does not examine the tool input, so the
/// model's display-only `intent` field cannot influence the decision.
pub fn classify(
    server_name: &str,
    tool_name: &str,
    server_config: Option<&McpServerConfig>,
) -> Classification {
    let trimmed_server = server_name.trim();
    if trimmed_server.eq_ignore_ascii_case("firecrawl") {
        // Firecrawl retrieval and service APIs do not depend on a native
        // browser popup. Only the Interact surface controls a real external
        // browser session and remains protected.
        return if is_firecrawl_browser_tool(tool_name) {
            Classification::Protected
        } else if is_firecrawl_safe_tool(tool_name) {
            Classification::AutoSafe
        } else {
            Classification::ServerDefault
        };
    }

    if trimmed_server.eq_ignore_ascii_case("playwright") {
        // Playwright read-only tools auto-run only when the launch is
        // provably `--headless` + `--isolated` AND carries no credential
        // markers. Everything else stays protected.
        let Some(config) = server_config else {
            return Classification::Protected;
        };
        if !is_headless_isolated_config(config) {
            return Classification::Protected;
        }
        if config.has_credential_markers() {
            return Classification::Protected;
        }
        return if is_playwright_safe_tool(tool_name) {
            Classification::AutoSafe
        } else {
            Classification::Protected
        };
    }

    Classification::ServerDefault
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod policy_tests;
