use super::{ClassifiedOperation, Result, ToolContext};
use chrono::{DateTime, Utc};
use std::time::Duration;

const SESSION_BOUNDARY_TTL: Duration = Duration::from_secs(15 * 60);

pub(super) async fn request_classified_operation_with_timeout(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
    timeout: Duration,
) -> Result<()> {
    request_classified_operation_with_timeout_and_boundary_ttl(
        operation,
        ctx,
        timeout,
        SESSION_BOUNDARY_TTL,
    )
    .await
}

#[cfg(test)]
pub(super) async fn request_classified_operation_with_timeout_and_boundary_ttl(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    boundary_ttl: Duration,
) -> Result<()> {
    request_classified_operation_with_boundary_ttl(operation, ctx, timeout, boundary_ttl).await
}

#[cfg(not(test))]
async fn request_classified_operation_with_timeout_and_boundary_ttl(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    boundary_ttl: Duration,
) -> Result<()> {
    request_classified_operation_with_boundary_ttl(operation, ctx, timeout, boundary_ttl).await
}

async fn request_classified_operation_with_boundary_ttl(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    boundary_ttl: Duration,
) -> Result<()> {
    let now = Utc::now();
    let expires_at = operation
        .allows_session_boundary_reuse()
        .then(|| boundary_expiry(now, boundary_ttl));
    let reservation = super::super::consent_state::reserve(
        &ctx.session_id,
        &operation.family,
        expires_at.is_some(),
        now,
    )?;
    if let super::super::consent_state::Reservation::Reused { expires_at } = reservation {
        crate::logging::event_info(
            "CONSENT_GUARDRAIL",
            vec![
                ("decision".to_string(), "reuse".to_string()),
                ("session_id".to_string(), ctx.session_id.clone()),
                ("class".to_string(), operation.family.clone()),
                ("expires_at".to_string(), expires_at.to_rfc3339()),
            ],
        );
        return Ok(());
    }
    let super::super::consent_state::Reservation::Prompt { generation } = reservation else {
        unreachable!("all consent reservations are prompt or reuse")
    };
    crate::logging::event_info(
        "CONSENT_GUARDRAIL",
        vec![
            ("decision".to_string(), "ask".to_string()),
            ("session_id".to_string(), ctx.session_id.clone()),
            ("class".to_string(), operation.family.clone()),
            ("tool".to_string(), operation.requirement.tool.clone()),
            ("action".to_string(), operation.requirement.action.clone()),
        ],
    );
    let (outcome, result) =
        request_current_user_classified_consent_with_timeout(operation, ctx, timeout, expires_at)
            .await;
    super::super::consent_state::finish(
        &ctx.session_id,
        &operation.family,
        generation,
        outcome,
        expires_at,
    );
    result
}

fn boundary_expiry(now: DateTime<Utc>, ttl: Duration) -> DateTime<Utc> {
    now + chrono::Duration::from_std(ttl).expect("fixed session boundary TTL is representable")
}

async fn request_current_user_classified_consent_with_timeout(
    operation: &ClassifiedOperation,
    ctx: &ToolContext,
    timeout: Duration,
    expires_at: Option<DateTime<Utc>>,
) -> (super::ConsentOutcome, Result<()>) {
    let approval = expires_at.map(|expires_at| {
        format!(
            "class={}\nscope=session\nexpires_at={}\nApprove this capability boundary?",
            operation.family,
            expires_at.to_rfc3339(),
        )
    });
    super::request_current_user_consent_with_timeout_and_boundary_outcome(
        &operation.requirement,
        ctx,
        timeout,
        approval.as_deref(),
    )
    .await
}
