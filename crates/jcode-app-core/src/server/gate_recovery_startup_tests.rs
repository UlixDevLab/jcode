use super::*;
use crate::gates::types::GATE_SCHEMA_VERSION;
use crate::gates::{
    AnswerRecord, DeliveryAcknowledgement, GateBinding, GateContinuation, GateDecision, GateOption,
    GateQuestion, GateRequest, GateState,
};
use crate::message::{ContentBlock, Message, StreamEvent, ToolDefinition};
use crate::provider::{EventStream, Provider};
use crate::tool::Registry;
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use futures::stream;
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::RwLock;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 5, 10, 0, 0).unwrap()
}

fn gate(id: &str, session_id: &str) -> GateRequest {
    GateRequest {
        schema_version: GATE_SCHEMA_VERSION,
        gate_id: id.to_string(),
        binding: GateBinding::mission_plan(
            session_id,
            "mission-startup-gate-recovery",
            2,
            "b3:startup-gate-recovery",
        ),
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
    calls: Arc<StdMutex<Vec<Vec<Message>>>>,
}

#[async_trait]
impl Provider for CapturingProvider {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[ToolDefinition],
        _system: &str,
        _resume_session_id: Option<&str>,
    ) -> anyhow::Result<EventStream> {
        self.calls
            .lock()
            .expect("capture calls")
            .push(messages.to_vec());
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
) -> Arc<tokio::sync::Mutex<crate::agent::Agent>> {
    let registry = Registry::new(Arc::clone(&provider)).await;
    let mut session = crate::session::Session::create_with_id(session_id.to_string(), None, None);
    session.model = Some("capturing".to_string());
    Arc::new(tokio::sync::Mutex::new(
        crate::agent::Agent::new_with_session(provider, registry, session, None),
    ))
}

fn has_text(messages: &[Message], expected: &str) -> bool {
    messages.iter().any(|message| {
        message.content.iter().any(
            |block| matches!(block, ContentBlock::Text { text, .. } if text.contains(expected)),
        )
    })
}

struct HomeGuard {
    previous: Option<OsString>,
}

impl HomeGuard {
    fn set(home: &std::path::Path) -> Self {
        let previous = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", home);
        Self { previous }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            crate::env::set_var("JCODE_HOME", previous);
        } else {
            crate::env::remove_var("JCODE_HOME");
        }
    }
}

#[tokio::test]
async fn empty_ledger_has_no_startup_gate_recovery_work() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = crate::gates::GateStore::at(temp.path().join("gates"));
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));

    let stats =
        recover_unconsumed_gate_directives_for_existing_sessions(&store, &sessions, &swarm_members)
            .await?;

    assert_eq!(stats.discovered, 0);
    assert_eq!(stats.delivered, 0);
    Ok(())
}

#[tokio::test]
async fn malformed_scan_does_not_block_startup_or_consume_a_valid_directive() -> anyhow::Result<()>
{
    let _test_env = crate::storage::lock_test_env();
    let temp = tempfile::tempdir()?;
    let _home = HomeGuard::set(temp.path());
    let store = crate::gates::GateStore::from_jcode_home()?;
    let request = gate("gate-malformed", "session-malformed");
    store.create(request.clone())?;
    store.record_answer(answer(&request, GateDecision::Approve), now())?;
    std::fs::create_dir_all(temp.path().join("gates"))?;
    std::fs::write(temp.path().join("gates").join("bad-entry"), "malformed")?;

    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));
    let stats = recover_unconsumed_gate_directives_on_startup(&sessions, &swarm_members).await;

    assert!(
        stats.scan_failed,
        "malformed ledger must only degrade startup"
    );
    assert_eq!(stats.delivered, 0);
    assert!(
        store.load_unconsumed_directives().is_err(),
        "malformed ledger must remain fail-closed after startup recovery"
    );
    let stored = store.load(&request.gate_id, &request.binding.session_id)?;
    assert!(matches!(stored.state, GateState::Answered { .. }));
    assert!(
        stored.resume_directive.is_some(),
        "valid directive must stay unconsumed"
    );
    Ok(())
}

#[tokio::test]
async fn restored_exact_session_is_delivered_once_applied_and_never_redelivered()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = crate::gates::GateStore::at(temp.path().join("gates"));
    let request = gate("gate-restored", "session-restored");
    store.create(request.clone())?;
    store.record_answer(answer(&request, GateDecision::Approve), now())?;
    let provider = Arc::new(CapturingProvider::default());
    let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
    let sessions = Arc::new(RwLock::new(HashMap::from([(
        request.binding.session_id.clone(),
        agent,
    )])));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));

    let first =
        recover_unconsumed_gate_directives_for_existing_sessions(&store, &sessions, &swarm_members)
            .await?;
    assert_eq!(first.discovered, 1);
    assert_eq!(first.delivered, 1);
    assert!(matches!(
        store
            .load(&request.gate_id, &request.binding.session_id)?
            .state,
        GateState::Applied { .. }
    ));
    let calls_after_first = provider.calls.lock().expect("capture calls").clone();
    assert!(has_text(&calls_after_first.concat(), "Gate approved"));
    assert!(has_text(
        &calls_after_first.concat(),
        "Continue approved work."
    ));

    let second =
        recover_unconsumed_gate_directives_for_existing_sessions(&store, &sessions, &swarm_members)
            .await?;
    assert_eq!(second.discovered, 0);
    assert_eq!(second.delivered, 0);
    assert_eq!(
        provider.calls.lock().expect("capture calls").len(),
        calls_after_first.len(),
        "second startup sweep must not redeliver an applied directive"
    );
    Ok(())
}

#[tokio::test]
async fn missing_session_stays_unconsumed_for_an_exact_restore_owner() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = crate::gates::GateStore::at(temp.path().join("gates"));
    let request = gate("gate-missing", "session-missing");
    store.create(request.clone())?;
    store.record_answer(answer(&request, GateDecision::Approve), now())?;
    let sessions = Arc::new(RwLock::new(HashMap::new()));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));

    let stats =
        recover_unconsumed_gate_directives_for_existing_sessions(&store, &sessions, &swarm_members)
            .await?;

    assert_eq!(stats.discovered, 1);
    assert_eq!(stats.deferred_for_restore, 1);
    assert_eq!(stats.delivered, 0);
    assert!(matches!(
        store
            .load(&request.gate_id, &request.binding.session_id)?
            .state,
        GateState::Answered { .. }
    ));
    assert_eq!(store.load_unconsumed_directives()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn rejected_directive_sends_only_the_refusal_continuation() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = crate::gates::GateStore::at(temp.path().join("gates"));
    let request = gate("gate-rejected", "session-rejected");
    store.create(request.clone())?;
    store.record_answer(answer(&request, GateDecision::Reject), now())?;
    let provider = Arc::new(CapturingProvider::default());
    let agent = restored_agent(provider.clone(), &request.binding.session_id).await;
    let sessions = Arc::new(RwLock::new(HashMap::from([(
        request.binding.session_id.clone(),
        agent,
    )])));
    let swarm_members = Arc::new(RwLock::new(HashMap::new()));

    let stats =
        recover_unconsumed_gate_directives_for_existing_sessions(&store, &sessions, &swarm_members)
            .await?;

    assert_eq!(stats.delivered, 1);
    let calls = provider.calls.lock().expect("capture calls").clone();
    assert!(has_text(&calls.concat(), "Do not perform the gated work"));
    assert!(has_text(&calls.concat(), "Stop the gated work."));
    assert!(!has_text(&calls.concat(), "Continue approved work."));
    Ok(())
}
