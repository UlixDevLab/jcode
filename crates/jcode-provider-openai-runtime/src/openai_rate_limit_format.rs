//! Human-readable formatting for OpenAI rate-limit / usage-limit errors.
use super::super::*;

/// Marker appended to a 429 message when the provider's own reset window is
/// longer than any retry loop should wait. The retry classifier keys off this
/// so the turn fails over to another account/provider immediately instead of
/// burning its retry budget against a limit that cannot clear in time.
pub(crate) const LONG_RESET_MARKER: &str = "usage limit will not reset in time to retry here";

/// The provider-reported time until the usage limit resets, from either
/// `resets_in_seconds` or an absolute `resets_at` timestamp.
///
/// OpenAI reports subscription usage-limit resets in the 429 body rather than
/// in a `Retry-After` header, so this is the only signal available for
/// scheduling. Returned verbatim; callers decide whether to wait it out or to
/// treat it as too long and fail over.
pub(crate) fn parse_rate_limit_reset(body: &str) -> Option<Duration> {
    let error = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|payload| payload.get("error").cloned())?;

    if let Some(seconds) = error
        .get("resets_in_seconds")
        .and_then(json_nonnegative_seconds)
    {
        return Some(Duration::from_secs(seconds));
    }

    let resets_at = error.get("resets_at").and_then(json_timestamp)?;
    let now = chrono::Utc::now().timestamp();
    Some(Duration::from_secs(
        u64::try_from(resets_at.saturating_sub(now)).unwrap_or(0),
    ))
}

pub(crate) fn format_rate_limit_error(body: &str, retry_after: Option<Duration>) -> String {
    let Some(error) = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|payload| payload.get("error").cloned())
    else {
        return format_unstructured_rate_limit_error(body, retry_after);
    };

    let Some(message) = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::trim)
        .filter(|message| !message.is_empty())
    else {
        return format_unstructured_rate_limit_error(body, retry_after);
    };

    let mut output = format!("Rate limited: {message}");
    ensure_sentence_ending(&mut output);

    if let Some(plan) = error
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
    {
        output.push_str(&format!(" Plan: {plan}."));
    }

    let resets_in = error
        .get("resets_in_seconds")
        .and_then(json_nonnegative_seconds);
    let resets_at = error
        .get("resets_at")
        .and_then(json_timestamp)
        .and_then(|timestamp| chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.format("%Y-%m-%d %H:%M UTC").to_string());

    match (resets_in, resets_at) {
        (Some(seconds), Some(timestamp)) => output.push_str(&format!(
            " Resets in {} ({timestamp}).",
            format_compact_duration(Duration::from_secs(seconds))
        )),
        (Some(seconds), None) => output.push_str(&format!(
            " Resets in {}.",
            format_compact_duration(Duration::from_secs(seconds))
        )),
        (None, Some(timestamp)) => output.push_str(&format!(" Resets at {timestamp}.")),
        (None, None) => {}
    }

    if let Some(delay) = retry_after {
        output.push_str(&format!(" Retry after {}.", format_compact_duration(delay)));
    }

    output
}

fn format_unstructured_rate_limit_error(body: &str, retry_after: Option<Duration>) -> String {
    let wait_info = retry_after
        .map(|delay| format!(" (retry after {})", format_compact_duration(delay)))
        .unwrap_or_default();
    format!("Rate limited{wait_info}: {body}")
}

fn ensure_sentence_ending(text: &mut String) {
    if !text.ends_with(['.', '!', '?']) {
        text.push('.');
    }
}

fn json_nonnegative_seconds(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
}

fn json_timestamp(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse::<i64>().ok())
}

fn format_compact_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let days = total_seconds / 86_400;
    let hours = total_seconds % 86_400 / 3_600;
    let minutes = total_seconds % 3_600 / 60;
    let seconds = total_seconds % 60;

    if days > 0 {
        let mut parts = vec![format!("{days}d")];
        if hours > 0 {
            parts.push(format!("{hours}h"));
        }
        if minutes > 0 {
            parts.push(format!("{minutes}m"));
        }
        parts.join(" ")
    } else if hours > 0 {
        let mut parts = vec![format!("{hours}h")];
        if minutes > 0 {
            parts.push(format!("{minutes}m"));
        }
        parts.join(" ")
    } else if minutes > 0 {
        let mut parts = vec![format!("{minutes}m")];
        if seconds > 0 {
            parts.push(format!("{seconds}s"));
        }
        parts.join(" ")
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_usage_limit_is_human_readable() {
        let body = r#"{"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"team","resets_at":1787286694,"eligible_promo":null,"resets_in_seconds":2608165}}"#;

        let message = format_rate_limit_error(body, None);

        assert_eq!(
            message,
            "Rate limited: The usage limit has been reached. Plan: team. Resets in 30d 4h 29m (2026-08-21 04:31 UTC)."
        );
        assert!(!message.contains('{'));
        assert!(!message.contains("usage_limit_reached"));
        assert!(!message.contains("eligible_promo"));
        assert!(!message.contains("2608165"));
    }

    #[test]
    fn structured_usage_limit_handles_missing_optional_fields() {
        let body = r#"{"error":{"message":"Too many requests"}}"#;

        assert_eq!(
            format_rate_limit_error(body, Some(Duration::from_secs(65))),
            "Rate limited: Too many requests. Retry after 1m 5s."
        );
    }

    #[test]
    fn structured_usage_limit_accepts_string_reset_values() {
        let body = r#"{"error":{"message":"Limit reached.","resets_at":"1787286694","resets_in_seconds":"2608165"}}"#;

        assert_eq!(
            format_rate_limit_error(body, None),
            "Rate limited: Limit reached. Resets in 30d 4h 29m (2026-08-21 04:31 UTC)."
        );
    }

    #[test]
    fn reset_window_is_parsed_from_seconds_field() {
        let body = r#"{"error":{"message":"Limit reached.","resets_in_seconds":2608165}}"#;
        assert_eq!(
            parse_rate_limit_reset(body),
            Some(Duration::from_secs(2_608_165))
        );
    }

    #[test]
    fn reset_window_falls_back_to_absolute_timestamp() {
        let resets_at = chrono::Utc::now().timestamp() + 300;
        let body = format!(r#"{{"error":{{"message":"Limit.","resets_at":{resets_at}}}}}"#);
        let parsed = parse_rate_limit_reset(&body).expect("reset");
        assert!(parsed <= Duration::from_secs(300));
        assert!(parsed > Duration::from_secs(280));
    }

    #[test]
    fn past_reset_timestamp_reports_no_remaining_wait() {
        let resets_at = chrono::Utc::now().timestamp() - 5_000;
        let body = format!(r#"{{"error":{{"message":"Limit.","resets_at":{resets_at}}}}}"#);
        assert_eq!(parse_rate_limit_reset(&body), Some(Duration::ZERO));
    }

    #[test]
    fn body_without_reset_fields_has_no_window() {
        assert_eq!(
            parse_rate_limit_reset(r#"{"error":{"message":"Too many requests"}}"#),
            None
        );
        assert_eq!(parse_rate_limit_reset("not json"), None);
    }

    #[test]
    fn unknown_or_malformed_rate_limit_payload_keeps_original_body() {
        let unknown = r#"{"unexpected":"still useful"}"#;
        assert_eq!(
            format_rate_limit_error(unknown, None),
            format!("Rate limited: {unknown}")
        );

        let malformed = "upstream temporarily unavailable <request 42>";
        assert_eq!(
            format_rate_limit_error(malformed, Some(Duration::from_secs(9))),
            "Rate limited (retry after 9s): upstream temporarily unavailable <request 42>"
        );
    }
}
