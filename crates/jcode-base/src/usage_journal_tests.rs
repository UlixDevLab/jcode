use super::*;
use crate::message::{Message, ToolDefinition};
use futures::StreamExt;
use std::sync::Mutex;

struct Fake(Mutex<String>);
#[async_trait::async_trait]
impl Provider for Fake {
    fn fork(&self) -> std::sync::Arc<dyn Provider> {
        std::sync::Arc::new(Fake(Mutex::new(self.model())))
    }
    fn name(&self) -> &str {
        "test"
    }
    fn model(&self) -> String {
        self.0.lock().unwrap().clone()
    }
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        Ok(Box::pin(futures::stream::empty()))
    }
}
fn call(provider: &Fake) -> Call {
    let mut call = Call::new(provider);
    call.destination = None; // Test never writes to personal state.
    call
}
fn usage(input: Option<u64>, output: Option<u64>) -> StreamEvent {
    StreamEvent::TokenUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_input_tokens: None,
        cache_creation_input_tokens: None,
    }
}

#[tokio::test]
async fn request_context_and_model_are_captured_not_reconstructed_from_session() {
    let provider = Fake(Mutex::new("model-a".into()));
    let a = scope(
        UsageContext {
            session_id: Some("same-session".into()),
            working_dir: Some("/project-a".into()),
            ..Default::default()
        },
        async { call(&provider) },
    )
    .await;
    *provider.0.lock().unwrap() = "model-b".into();
    let b = scope(
        UsageContext {
            session_id: Some("same-session".into()),
            parent_session_id: Some("root".into()),
            working_dir: Some("/project-b".into()),
            ..Default::default()
        },
        async { call(&provider) },
    )
    .await;
    assert_eq!(a.event.model_at_dispatch, "model-a");
    assert_eq!(b.event.model_at_dispatch, "model-b");
    assert_ne!(a.event.context.working_dir, b.event.context.working_dir);
    assert_ne!(a.event.event_id, b.event.event_id);
    assert!(a.event.account_ref.is_none());
    assert!(a.event.model_actual.is_none());
    assert!(CONTEXT.try_with(Clone::clone).is_err());
}

#[tokio::test]
async fn stream_snapshots_coalesce_and_usage_after_message_end_is_retained() {
    let provider = Fake(Mutex::new("model".into()));
    let inner = futures::stream::iter(vec![
        Ok(usage(Some(10), Some(2))),
        Ok(usage(Some(10), Some(3))),
        Ok(StreamEvent::MessageEnd { stop_reason: None }),
        Ok(usage(None, Some(5))),
    ]);
    let mut stream = AccountedStream {
        inner: Box::pin(inner),
        call: call(&provider),
    };
    let mut count = 0;
    while stream.next().await.is_some() {
        count += 1;
    }
    assert_eq!(count, 4);
    assert_eq!(stream.call.event.tokens.input, Some(10));
    assert_eq!(stream.call.event.tokens.output, Some(5));
    assert_eq!(stream.call.event.status, "completed");
    assert!(stream.call.finalized);
    let completed = stream.call.event.completed_at.clone();
    stream.call.finish("error");
    assert_eq!(stream.call.event.completed_at, completed);
    assert_eq!(stream.call.event.status, "completed");
}

#[test]
fn message_end_does_not_erase_an_observed_error() {
    let provider = Fake(Mutex::new("model".into()));
    let mut call = call(&provider);
    call.observe(&StreamEvent::Error {
        message: "not serialized".into(),
        retry_after_secs: None,
    });
    call.observe(&StreamEvent::MessageEnd { stop_reason: None });
    assert_eq!(call.event.status, "error");
}

#[test]
fn rollback_segments_are_distinct_and_missing_usage_is_not_zero() {
    let provider = Fake(Mutex::new("model".into()));
    let mut call = call(&provider);
    assert!(!call.event.usage_present);
    assert!(call.event.tokens.input.is_none());
    call.observe(&usage(Some(50), Some(3)));
    let first = call.event.event_id.clone();
    call.observe(&StreamEvent::RetryRollback { attempt: 1, max: 3 });
    assert_ne!(first, call.event.event_id);
    assert!(!call.event.usage_present);
    assert!(call.event.tokens.input.is_none());
    call.observe(&usage(Some(51), Some(4)));
    call.finish("cancelled");
    assert_eq!(call.event.tokens.output, Some(4));
    assert_eq!(call.event.status, "cancelled");
}

#[test]
fn journal_serializes_only_metadata_and_unwritable_storage_is_an_io_error() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = Fake(Mutex::new("model".into()));
    let mut call = call(&provider);
    call.observe(&StreamEvent::TextDelta(
        "private prompt must not persist".into(),
    ));
    call.observe(&StreamEvent::Error {
        message: "secret credential must not persist".into(),
        retry_after_secs: None,
    });
    call.finish("error");
    append_event(tmp.path(), &call.event).unwrap();
    let path = std::fs::read_dir(tmp.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("private prompt"));
    assert!(!text.contains("secret credential"));
    let value: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(value["status"], "error");
    assert!(append_event(&path, &call.event).is_err());
    // The production queue sends failures to diagnostics, never to the caller.
    assert!(text.len() < 2048);
}

#[tokio::test]
async fn transport_identity_survives_spawn_and_uses_only_opaque_metadata() {
    let provider = Fake(Mutex::new("requested".into()));
    let mut call = call(&provider);
    transport::OBSERVATIONS
        .scope(call.observations.clone(), async {
            tokio::spawn(transport::inherit(async {
                let identity = transport::subscription_identity(
                    Some("abc"),
                    Some("person@example.test"),
                    "https",
                );
                let index = transport::begin(identity);
                transport::response(index, Some("server-request-123"), Some("actual-model"));
            }))
            .await
            .unwrap();
        })
        .await;
    call.finish("completed");
    assert_eq!(
        call.event.account_ref.as_deref(),
        Some("account-ba7816bf8f01")
    );
    assert_eq!(call.event.model_actual.as_deref(), Some("actual-model"));
    assert_eq!(
        call.event.upstream_request_id.as_deref(),
        Some("server-request-123")
    );
    assert_eq!(
        call.event.billing_route.as_deref(),
        Some("codex_subscription")
    );
    assert_eq!(call.event.account_aliases.len(), 1);
    let serialized = serde_json::to_string(&call.event).unwrap();
    assert!(!serialized.contains("person@example.test"));
    assert!(!serialized.contains("\"abc\""));
    assert!(transport::begin(Default::default()).is_none());
}

#[tokio::test]
async fn multiple_transports_or_retry_segments_never_guess_the_last_account() {
    let provider = Fake(Mutex::new("model".into()));
    let mut call = call(&provider);
    transport::OBSERVATIONS
        .scope(call.observations.clone(), async {
            transport::begin(transport::subscription_identity(
                Some("account-a"),
                None,
                "https",
            ));
            transport::begin(transport::subscription_identity(
                Some("account-b"),
                None,
                "https",
            ));
        })
        .await;
    call.observe(&usage(Some(100), Some(5)));
    call.finish("completed");
    assert!(call.event.account_ref.is_none());
    assert_eq!(call.event.transport_observations.len(), 2);

    let mut retried = super::tests::call(&provider);
    transport::OBSERVATIONS
        .scope(retried.observations.clone(), async {
            transport::begin(transport::subscription_identity(
                Some("account-a"),
                None,
                "https",
            ));
        })
        .await;
    retried.observe(&StreamEvent::RetryRollback { attempt: 1, max: 2 });
    retried.finish("completed");
    assert!(retried.event.account_ref.is_none());
}

#[tokio::test]
async fn transport_metadata_is_bounded_and_account_identity_is_not_current_session_state() {
    let provider = Fake(Mutex::new("model".into()));
    let mut first = call(&provider);
    let second = call(&provider);
    transport::OBSERVATIONS
        .scope(first.observations.clone(), async {
            for _ in 0..40 {
                let index =
                    transport::begin(transport::subscription_identity(Some("abc"), None, "https"));
                transport::response(index, Some("bad\nheader"), Some(&"x".repeat(161)));
            }
        })
        .await;
    first.finish("completed");
    assert_eq!(first.event.transport_observations.len(), 32);
    assert!(
        first
            .event
            .transport_observations
            .iter()
            .all(|r| r.upstream_request_id.is_none() && r.model_actual.is_none())
    );
    assert!(second.observations.lock().unwrap().is_empty());
}
