use super::{ProtectedOperation, ToolContext};
use serde_json::Value;

const NAVIGATION_FAMILY: &str = "navigation";
const BROWSER_FAMILY: &str = "browser";
const OPEN_EXTERNAL_FAMILY: &str = "open-external";
const DESKTOP_INPUT_FAMILY: &str = "desktop-input";
const APPLESCRIPT_FAMILY: &str = "applescript";
const DESKTOP_MUTATION_FAMILY: &str = "desktop-mutation";
const AUTH_FAMILY: &str = "auth";
const UNKNOWN_FAMILY: &str = "unknown";

/// Fixed native-consent categories. Unknown operations fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationClass {
    Navigation,
    Browser,
    OpenExternal,
    DesktopInput,
    AppleScript,
    DesktopMutation,
    Auth,
    Unknown,
}

impl OperationClass {
    fn family(self) -> &'static str {
        match self {
            Self::Navigation => NAVIGATION_FAMILY,
            Self::Browser => BROWSER_FAMILY,
            Self::OpenExternal => OPEN_EXTERNAL_FAMILY,
            Self::DesktopInput => DESKTOP_INPUT_FAMILY,
            Self::AppleScript => APPLESCRIPT_FAMILY,
            Self::DesktopMutation => DESKTOP_MUTATION_FAMILY,
            Self::Auth => AUTH_FAMILY,
            Self::Unknown => UNKNOWN_FAMILY,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassifiedOperation {
    pub(super) requirement: ProtectedOperation,
    pub(super) family: String,
    class: OperationClass,
}

impl ClassifiedOperation {
    fn new(requirement: ProtectedOperation, class: OperationClass) -> Self {
        Self {
            requirement,
            family: class.family().to_string(),
            class,
        }
    }

    pub(crate) fn from_native(requirement: ProtectedOperation) -> Self {
        let class = match (requirement.tool.as_str(), requirement.action.as_str()) {
            ("gmail", "connect") => OperationClass::Auth,
            _ => OperationClass::Unknown,
        };
        Self::new(requirement, class)
    }

    #[cfg(test)]
    pub(crate) fn class(&self) -> OperationClass {
        self.class
    }

    pub(crate) fn family(&self) -> &str {
        &self.family
    }

    pub(crate) fn requires_native_consent(&self, policy: &crate::config::ConsentConfig) -> bool {
        self.class != OperationClass::Navigation || !policy.navigation.allows_navigation()
    }

    pub(crate) fn allows_session_boundary_reuse(&self) -> bool {
        matches!(
            self.class,
            OperationClass::Browser
                | OperationClass::OpenExternal
                | OperationClass::DesktopInput
                | OperationClass::AppleScript
        )
    }
}

/// Classify raw inputs at the registry boundary. Only the listed structural
/// shapes enter the default-allow navigation class.
pub(crate) fn protected_operation(name: &str, input: &Value) -> Option<ClassifiedOperation> {
    protected_operation_inner(name, input, None)
}

pub(crate) fn protected_operation_with_context(
    name: &str,
    input: &Value,
    ctx: &ToolContext,
) -> Option<ClassifiedOperation> {
    protected_operation_inner(name, input, Some(ctx))
}

fn protected_operation_inner(
    name: &str,
    input: &Value,
    ctx: Option<&ToolContext>,
) -> Option<ClassifiedOperation> {
    let action = input.get("action").and_then(Value::as_str).unwrap_or("");
    match name {
        "macos_computer_use"
            if !action.is_empty() && !matches!(action, "discover" | "check_permissions") =>
        {
            Some(ClassifiedOperation::new(
                ProtectedOperation::new(
                    "macos_computer_use",
                    action,
                    super::computer_target_summary(input),
                ),
                computer_class(action),
            ))
        }
        "browser" if action != "status" => Some(ClassifiedOperation::new(
            ProtectedOperation::new("browser", action, super::browser_target_summary(input)),
            if is_pure_browser_navigation(input) {
                OperationClass::Navigation
            } else {
                OperationClass::Browser
            },
        )),
        "open" => {
            let action = if action.is_empty() { "open" } else { action };
            Some(ClassifiedOperation::new(
                ProtectedOperation::new(
                    "open",
                    action,
                    super::safe_url_summary(
                        input.get("target").and_then(Value::as_str).unwrap_or(""),
                    ),
                ),
                if ctx.is_some_and(|ctx| {
                    crate::tool::open::navigation::is_default_navigation_request(input, ctx)
                }) {
                    OperationClass::Navigation
                } else {
                    OperationClass::OpenExternal
                },
            ))
        }
        _ => None,
    }
}

/// Only a structurally typed HTTP(S) page load can use the navigation policy.
/// Script execution, form input, selectors, and any unrecognised field retain
/// the browser capability boundary because their effect is not enforceable here.
fn is_pure_browser_navigation(input: &Value) -> bool {
    const FIELDS: &[&str] = &[
        "action",
        "intent",
        "url",
        "browser",
        "tab_id",
        "window_id",
        "wait",
        "new_tab",
        "timeout_ms",
    ];
    let Some(fields) = input.as_object() else {
        return false;
    };
    fields.keys().all(|field| FIELDS.contains(&field.as_str()))
        && fields.get("action").and_then(Value::as_str) == Some("open")
        && fields
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(crate::tool::open::navigation::is_http_navigation_url)
}

fn computer_class(action: &str) -> OperationClass {
    match action {
        "move" | "click" | "double_click" | "right_click" | "drag" | "scroll" | "type" | "key"
        | "key_down" | "key_up" => OperationClass::DesktopInput,
        "run_applescript" | "run_jxa" => OperationClass::AppleScript,
        "press" | "set_value" | "perform_action" | "select_menu" | "activate_app" | "hide_app"
        | "quit_app" | "focus_window" | "move_window" | "resize_window" | "minimize_window"
        | "close_window" | "set_clipboard" | "notify" | "set_brightness" => {
            OperationClass::DesktopMutation
        }
        _ => OperationClass::Unknown,
    }
}
