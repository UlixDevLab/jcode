use super::*;
use crate::gates::types::GATE_SCHEMA_VERSION;
use crate::gates::{
    AnswerRecord, DeliveryAcknowledgement, GateBinding, GateContinuation, GateOption, GateQuestion,
};
use crate::message::{ContentBlock, Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::tool::Registry;
use async_trait::async_trait;
use chrono::{Duration, TimeZone};
use futures::stream;
use std::sync::{Arc, Mutex as StdMutex};
use tempfile::TempDir;
use tokio::sync::{Mutex, mpsc};

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 10, 0, 0).unwrap()
}

fn gate(id: &str, session_id: &str) -> GateRequest {
    GateRequest {
        schema_version: GATE_SCHEMA_VERSION,
        gate_id: id.to_string(),
        binding: GateBinding::mission_plan(session_id, "mission-gate-resume", 2, "b3:gate-resume"),
        transport_binding: None,
        question: GateQuestion {
            question: "Proceed?".to_string(),
            problem: "A user decision is required.".to_string(),
            impact: "The plan will be resumed or stopped.".to_string(),
            options: vec![GateOption {
                label: "Choose".to_string(),
                description: "Choose a continuation.".to_string(),
            }],
            recommendation: "Approve only when ready.".to_string(),
        },
        allowed_decisions: vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject,
        ],
        continuation: GateContinuation {
            on_approve: "Continue approved work.".to_string(),
            on_request_changes: "Revise the approved scope.".to_string(),
            on_reject: "Stop the gated work.".to_string(),
            on_expiry: "Stop after expiry.".to_string(),
        },
        created_at: now(),
        expires_at: now() + Duration::minutes(30),
        delivery: DeliveryAcknowledgement::NotAttempted,
        state: GateState::Pending,
        resume_directive: None,
    }
}

fn answer(gate: &GateRequest, decision: GateDecision) -> AnswerRecord {
    AnswerRecord {
        schema_version: GATE_SCHEMA_VERSION,
        gate_id: gate.gate_id.clone(),
        binding: gate.binding.clone(),
        decision,
        comment: None,
        answered_at: now(),
        received_via: "test".to_string(),
    }
}

#[derive(Clone, Default)]
struct CapturingProvider {
    calls: Arc<StdMutex<Vec<(Vec<Message>, String)>>>,
}

#[async_trait]
impl Provider for CapturingProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDefinition],
        system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        self.calls
            .lock()
            .expect("capture calls")
            .push((messages.to_vec(), system.to_string()));
        Ok(Box::pin(stream::iter(vec![Ok(StreamEvent::MessageEnd {
            stop_reason: Some("end_turn".to_string()),
        })])))
    }

    fn name(&self) -> &str {
        "capturing"
    }

    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

async fn restored_agent(
    provider: Arc<dyn Provider>,
    session_id: &str,
) -> Arc<Mutex<crate::agent::Agent>> {
    let registry = Registry::new(Arc::clone(&provider)).await;
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("capturing".to_string());
    Arc::new(Mutex::new(crate::agent::Agent::new_with_session(
        provider, registry, session, None,
    )))
}

fn has_text(message: &Message, expected: &str) -> bool {
    message
        .content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { text, .. } if text.contains(expected)))
}

#[tokio::test]
async fn restored_exact_session_delivers_one_system_continuation_then_never_replays()
-> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = GateStore::at(temp.path().join("gates"));
    let request = gate("gate-restored-once", "session-restored-once");
    ledger.create(request.clone())?;
    ledger.record_answer(answer(&request, GateDecision::Approve), now())?;
    let provider = Arc::new(CapturingProvider::default());
    let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
    let (event_tx, _event_rx) = mpsc::unbounded_channel();

    assert_eq!(
        consume_and_deliver_to_restored_session(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now(),
            agent,
            event_tx,
        )
        .await?,
        GateResumeRestoredOutcome::DeliveredRestoredSession
    );
    assert!(matches!(
        ledger
            .load(&request.gate_id, &request.binding.session_id)?
            .state,
        GateState::Applied { .. }
    ));
    let calls = provider.calls.lock().expect("capture calls");
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .flat_map(|(messages, _)| messages)
            .any(|message| {
                has_text(message, "Gate approved") && has_text(message, "Continue approved work.")
            })
    );
    let calls_before_retry = calls.len();
    drop(calls);

    let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    assert_eq!(
        consume_and_deliver_to_restored_session(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now(),
            agent,
            event_tx,
        )
        .await?,
        GateResumeRestoredOutcome::NoDirective
    );
    assert_eq!(
        provider.calls.lock().expect("capture calls").len(),
        calls_before_retry
    );
    Ok(())
}

#[tokio::test]
async fn restored_rejection_delivers_only_refusal_or_termination_system_continuation()
-> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = GateStore::at(temp.path().join("gates"));
    let request = gate("gate-restored-reject", "session-restored-reject");
    ledger.create(request.clone())?;
    ledger.record_answer(answer(&request, GateDecision::Reject), now())?;
    let provider = Arc::new(CapturingProvider::default());
    let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
    let (event_tx, _event_rx) = mpsc::unbounded_channel();

    assert_eq!(
        consume_and_deliver_to_restored_session(
            &ledger,
            &request.gate_id,
            &request.binding.session_id,
            now(),
            agent,
            event_tx,
        )
        .await?,
        GateResumeRestoredOutcome::DeliveredRestoredSession
    );
    let calls = provider.calls.lock().expect("capture calls");
    assert!(
        calls
            .iter()
            .flat_map(|(messages, _)| messages)
            .any(|message| {
                has_text(message, "Do not perform the gated work")
                    && has_text(message, "Stop the gated work.")
            })
    );
    assert!(
        !calls
            .iter()
            .flat_map(|(messages, _)| messages)
            .any(|message| has_text(message, "Gate approved"))
    );
    Ok(())
}

#[tokio::test]
async fn restored_terminal_or_unanswered_gates_do_not_start_a_continuation() -> anyhow::Result<()> {
    let temp = TempDir::new()?;
    let ledger = GateStore::at(temp.path().join("gates"));
    let provider = Arc::new(CapturingProvider::default());

    for request in [
        gate("gate-restored-pending", "session-restored-pending"),
        gate("gate-restored-expired", "session-restored-expired"),
        gate("gate-restored-cancelled", "session-restored-cancelled"),
    ] {
        ledger.create(request.clone())?;
        if request.gate_id.contains("expired") {
            ledger.expire(
                &request.gate_id,
                &request.binding.session_id,
                now() + Duration::hours(1),
            )?;
        } else if request.gate_id.contains("cancelled") {
            ledger.cancel(
                &request.gate_id,
                &request.binding.session_id,
                "user withdrew the request",
                now(),
            )?;
        }
        let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        assert_eq!(
            consume_and_deliver_to_restored_session(
                &ledger,
                &request.gate_id,
                &request.binding.session_id,
                now(),
                agent,
                event_tx,
            )
            .await?,
            GateResumeRestoredOutcome::NoDirective
        );
    }

    assert!(provider.calls.lock().expect("capture calls").is_empty());
    Ok(())
}
