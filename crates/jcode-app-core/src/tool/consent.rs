use super::consent_grants::BatchGrantManifest;
use super::{StdinInputRequest, ToolContext};
use anyhow::{Result, bail};
pub(crate) use jcode_tool_core::NativeConsentRequirement as ProtectedOperation;
use serde_json::Value;
use std::time::Duration;

#[path = "consent_policy.rs"]
mod policy;
pub(crate) use policy::{
    ClassifiedOperation, OperationClass, protected_operation, protected_operation_with_context,
};

pub(crate) const CONSENT_PROMPT_PREFIX: &str = "JCODE_CONSENT_V1";
const CONSENT_APPROVE_RESPONSE: &str = "approve";
const CONSENT_TIMEOUT: Duration = Duration::from_secs(60);

/// The native consent transport result. Only an explicit non-approval is a
/// denial; an unavailable route or timeout must not create a denial cooldown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConsentOutcome {
    Approved,
    Denied,
    Unavailable,
}

/// An unforgeable, process-local proof that the current TUI user approved one
/// already validated batch manifest. Only this module can construct it.
pub(crate) struct NativeBatchGrantApproval(());

pub(crate) async fn request_batch_grant_approval(
    manifest: &BatchGrantManifest,
    ctx: &ToolContext,
) -> Result<NativeBatchGrantApproval> {
    manifest.validate()?;
    let tx = ctx.stdin_request_tx.as_ref().ok_or_else(|| {
        anyhow::anyhow!("batch grant blocked: no current TUI client is available to approve it")
    })?;
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    tx.send(StdinInputRequest {
        request_id: crate::id::new_id("consent-grant"),
        prompt: manifest.prompt(),
        is_password: false,
        response_tx,
    })
    .map_err(|_| anyhow::anyhow!("batch grant blocked: approval client disconnected"))?;
    match tokio::time::timeout(CONSENT_TIMEOUT, response_rx).await {
        Ok(Ok(response))
            if response
                .trim()
                .eq_ignore_ascii_case(CONSENT_APPROVE_RESPONSE) =>
        {
            Ok(NativeBatchGrantApproval(()))
        }
        Ok(Ok(_)) => {
            bail!("batch grant blocked: current user denied or sent an invalid approval response")
        }
        Ok(Err(_)) => bail!("batch grant blocked: approval client disconnected"),
        Err(_) => bail!("batch grant blocked: current user approval timed out"),
    }
}

/// Clear same-turn locks for a newly accepted top-level message without
/// treating that message as approval.
pub(crate) fn begin_user_turn(session_id: &str) {
    super::consent_state::begin_turn(session_id);
}

pub(crate) fn clear_session(session_id: &str) {
    super::consent_state::clear_session(session_id);
}

pub(super) fn browser_target_summary(input: &serde_json::Value) -> String {
    input
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| is_browser_url(url))
        .map(safe_url_summary)
        .unwrap_or_else(|| "live browser".to_string())
}

pub(super) fn computer_target_summary(input: &serde_json::Value) -> String {
    if let Some(app) = input.get("app").and_then(Value::as_str) {
        return format!("desktop app: {}", truncate_safe(app, 80));
    }
    if let Some(window_id) = input.get("window_id").and_then(Value::as_i64) {
        return format!("desktop window: {window_id}");
    }
    "live desktop".to_string()
}

pub(crate) fn is_browser_url(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("http://") || value.starts_with("https://") || value.starts_with("mailto:")
}

pub(crate) fn safe_url_summary(value: &str) -> String {
    let without_query = value.trim().split(['?', '#']).next().unwrap_or("");
    if without_query
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("mailto:"))
    {
        return "mailto:<redacted>".to_string();
    }
    let without_userinfo = if let Some((scheme, remainder)) = without_query.split_once("://") {
        let authority_end = remainder.find('/').unwrap_or(remainder.len());
        let authority = &remainder[..authority_end];
        let host = authority.rsplit('@').next().unwrap_or(authority);
        format!("{scheme}://{host}{}", &remainder[authority_end..])
    } else {
        without_query.to_string()
    };
    truncate_safe(&without_userinfo, 120)
}

fn truncate_safe(value: &str, max_chars: usize) -> String {
    let sanitized: String = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    let mut result: String = sanitized.chars().take(max_chars).collect();
    if sanitized.chars().count() > max_chars {
        result.push('…');
    }
    result
}

#[path = "consent_boundary.rs"]
mod boundary;

pub(crate) async fn request_classified_operation(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
) -> Result<()> {
    boundary::request_classified_operation_with_timeout(operation, ctx, CONSENT_TIMEOUT).await
}

#[cfg(test)]
use boundary::{
    request_classified_operation_with_timeout,
    request_classified_operation_with_timeout_and_boundary_ttl,
};

async fn request_current_user_consent_with_timeout(
    operation: &ProtectedOperation,
    ctx: &ToolContext,
    timeout: Duration,
) -> Result<()> {
    request_current_user_consent_with_timeout_and_boundary(operation, ctx, timeout, None).await
}

pub(super) async fn request_current_user_consent_with_timeout_and_boundary(
    operation: &ProtectedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    boundary_approval: Option<&str>,
) -> Result<()> {
    request_current_user_consent_with_timeout_and_boundary_outcome(
        operation,
        ctx,
        timeout,
        boundary_approval,
    )
    .await
    .1
}

pub(super) async fn request_current_user_consent_with_timeout_and_boundary_outcome(
    operation: &ProtectedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    boundary_approval: Option<&str>,
) -> (ConsentOutcome, Result<()>) {
    let tool = sanitize_identifier(&operation.tool);
    let action = if operation.action.trim().is_empty()
        || operation.action.chars().any(char::is_control)
        || operation.action.chars().count() > 80
    {
        "<redacted>".to_string()
    } else {
        operation.action.trim().to_string()
    };
    let target_summary = sanitize_target_summary(&operation.target_summary);
    let Some(tx) = ctx.stdin_request_tx.as_ref() else {
        return (
            ConsentOutcome::Unavailable,
            Err(anyhow::anyhow!(
                "{} {} blocked: no current TUI client is available to approve this protected operation",
                tool,
                action
            )),
        );
    };
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let request = StdinInputRequest {
        request_id: crate::id::new_id("consent"),
        prompt: match boundary_approval {
            Some(boundary_approval) => format!(
                "{CONSENT_PROMPT_PREFIX}\n{boundary_approval}\ntool={}\naction={}\ntarget={}",
                tool, action, target_summary
            ),
            None => format!(
                "{CONSENT_PROMPT_PREFIX}\ntool={}\naction={}\ntarget={}\nApprove this one operation?",
                tool, action, target_summary
            ),
        },
        is_password: false,
        response_tx,
    };
    if tx.send(request).is_err() {
        return (
            ConsentOutcome::Unavailable,
            Err(anyhow::anyhow!(
                "{} {} blocked: no current TUI client is available to approve this protected operation",
                tool,
                action
            )),
        );
    }
    // The prompt is now on screen and this task is parked until the human
    // answers. Everything below is the wait, so this is the one moment jcode
    // can honestly report "needs attention" — `pre_tool` cannot, because it
    // also runs for calls that are auto-approved and never block.
    // Already-sanitized values only: the payload reaches an external process.
    let permission_hook = begin_permission_hook(ctx, &tool, &action, &target_summary);
    let waiting_since = std::time::Instant::now();
    let (resolution, outcome, result) = match tokio::time::timeout(timeout, response_rx).await {
        Ok(Ok(response))
            if response
                .trim()
                .eq_ignore_ascii_case(CONSENT_APPROVE_RESPONSE) =>
        {
            (ConsentOutcome::Approved, "approved", Ok(()))
        }
        Ok(Ok(_)) => (
            ConsentOutcome::Denied,
            "denied",
            Err(anyhow::anyhow!(
                "{} {} blocked: current user denied or sent an invalid approval response",
                tool,
                action
            )),
        ),
        Ok(Err(_)) => (
            ConsentOutcome::Unavailable,
            "disconnected",
            Err(anyhow::anyhow!(
                "{} {} blocked: approval client disconnected",
                tool,
                action
            )),
        ),
        Err(_) => (
            ConsentOutcome::Unavailable,
            "timeout",
            Err(anyhow::anyhow!(
                "{} {} blocked: current user approval timed out",
                tool,
                action
            )),
        ),
    };
    // Paired with the "waiting" post above on EVERY exit path, including the
    // denial and timeout ones: a supervisor that armed a waiting indicator has
    // to be told the wait ended, or the panel stays stuck on "needs input"
    // forever after a refused prompt.
    if let Some(permission_hook) = permission_hook {
        permission_hook.dispatch(permission_hook_event(
            ctx,
            "resolved",
            &tool,
            &action,
            &target_summary,
            Some(outcome),
            waiting_since.elapsed(),
        ));
    }
    (resolution, result)
}

/// Start a matched `permission` lifecycle pair. The ordered background
/// dispatcher makes `waiting` observable before its terminal `resolved` event
/// without making the user's response wait for a hook process.
fn begin_permission_hook(
    ctx: &ToolContext,
    tool: &str,
    action: &str,
    target: &str,
) -> Option<crate::hooks::OrderedObserver> {
    crate::hooks::begin_ordered_observer(permission_hook_event(
        ctx,
        "waiting",
        tool,
        action,
        target,
        None,
        Duration::ZERO,
    ))
}

/// Build one already-sanitized permission lifecycle event for an external
/// observer. Hook failures remain observer-only and never affect consent.
fn permission_hook_event(
    ctx: &ToolContext,
    state: &'static str,
    tool: &str,
    action: &str,
    target: &str,
    outcome: Option<&'static str>,
    waited: Duration,
) -> crate::hooks::HookEvent {
    let mut event = crate::hooks::HookEvent::new("permission")
        .session_id(ctx.session_id.clone())
        .field("STATE", state)
        .field("TOOL_NAME", tool.to_string())
        .field("ACTION", action.to_string())
        .field("TARGET", target.to_string());
    if let Some(dir) = ctx.working_dir.as_ref() {
        event = event.cwd(dir.display().to_string());
    }
    if let Some(outcome) = outcome {
        event = event
            .field("OUTCOME", outcome)
            .field("WAITED_MS", waited.as_millis().to_string());
    }
    event
}

fn sanitize_identifier(value: &str) -> String {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
    {
        return "<redacted>".to_string();
    }
    value.to_string()
}

fn sanitize_target_summary(value: &str) -> String {
    let value = truncate_safe(value, 120);
    if is_browser_url(&value) {
        safe_url_summary(&value)
    } else {
        value
    }
}

#[cfg(test)]
#[path = "consent_tests.rs"]
mod tests;
