use super::{OpenAction, OpenInput, ResolvedTarget, ToolContext, resolve_target};
use serde_json::Value;

/// True only for fixed, locally resolved page/file/app opens. This runs before
/// the tool executes so the native-consent boundary and the tool agree.
pub(crate) fn is_default_navigation_request(input: &Value, ctx: &ToolContext) -> bool {
    let Ok(input) = serde_json::from_value::<OpenInput>(input.clone()) else {
        return false;
    };
    if !matches!(
        OpenAction::parse(input.action.as_deref()),
        Ok(OpenAction::Open)
    ) {
        return false;
    }
    let Ok(target) = resolve_target(&input.target, ctx) else {
        return false;
    };
    match target {
        ResolvedTarget::Url(url) => is_http_navigation_url(&url),
        ResolvedTarget::Local { path, kind } => {
            matches!(kind, super::LocalTargetKind::File)
                || path.extension().is_some_and(|extension| extension == "app")
        }
    }
}

/// A browser navigation may only use HTTP(S) without URL userinfo.
pub(crate) fn is_http_navigation_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}
