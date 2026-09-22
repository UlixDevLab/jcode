use super::{
    HermesControlClient, HermesControlConfig, HermesControlRecord, HermesResult, JcodeControlServer,
};
use crate::agent::Agent;
use crate::jcode_control::{
    ClaimOutcome, ControlAction, ControlExecutionResult, ControlReceipt, ControlReceiptState,
    ControlReceiptStore, ControlRequest, ReceiptKey,
};
use crate::provider::{MultiProvider, set_model_with_auth_refresh};
use crate::session::Session;
use crate::tool::Registry;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::server::{
    client_actions::{NotifySessionContext, handle_notify_session},
    register_session_interrupt_queue,
};

pub(super) async fn process_record(
    server: &JcodeControlServer,
    client: &HermesControlClient,
    receipts: &ControlReceiptStore,
    record: HermesControlRecord,
) -> Result<()> {
    let claimed = client.claim(&record.id).await?;
    if claimed.id != record.id {
        bail!("Hermes control claim id mismatch")
    }
    let (principal, request) = match claimed.map_request(&client.config) {
        Ok(value) => value,
        Err(_) => {
            client
                .resolve(
                    &record.id,
                    HermesResult {
                        target_node: &client.config.node,
                        status: "failed",
                        session_id: None,
                        error_code: Some("rejected"),
                    },
                )
                .await?;
            return Ok(());
        }
    };
    let pinned = match &request.action {
        ControlAction::CreateSession { .. } => Some(client.config.pinned_resolution()?),
        _ => None,
    };
    let receipt = match receipts.claim(&principal, &request, pinned)? {
        ClaimOutcome::Claimed(receipt) | ClaimOutcome::Replay(receipt) => receipt,
    };
    let key = ReceiptKey::new(principal, request.idempotency_key.clone())?;
    let receipt = match receipt.state {
        ControlReceiptState::Completed { result, .. } => {
            post_result(client, &record.id, &result).await?;
            return Ok(());
        }
        ControlReceiptState::Failed { result, .. } => {
            post_result(client, &record.id, &result).await?;
            return Ok(());
        }
        ControlReceiptState::Claimed { .. } => {
            match execute(server, &client.config, &key, &request).await {
                Ok(result) => receipts.complete(&key, result)?,
                Err(_) => receipts.fail(&key, ControlExecutionResult::new("rejected", None)?)?,
            }
        }
        ControlReceiptState::Received => bail!("receipt remained unclaimed"),
    };
    post_receipt(client, &record.id, &receipt).await
}

async fn execute(
    server: &JcodeControlServer,
    config: &HermesControlConfig,
    key: &ReceiptKey,
    request: &ControlRequest,
) -> Result<ControlExecutionResult> {
    match &request.action {
        ControlAction::MessageExisting {
            jcode_session_id,
            message,
            ..
        } => {
            if !session_is_bound(server, jcode_session_id, &config.root).await {
                bail!("target session binding rejected")
            }
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            handle_notify_session(
                1,
                jcode_session_id.clone(),
                message.clone(),
                NotifySessionContext {
                    sessions: &server.sessions,
                    soft_interrupt_queues: &server.soft_interrupt_queues,
                    client_connections: &server.client_connections,
                    swarm_members: &server.swarm_members,
                    swarms_by_id: &server.swarms_by_id,
                    event_history: &server.event_history,
                    event_counter: &server.event_counter,
                    swarm_event_tx: &server.swarm_event_tx,
                    client_event_tx: &tx,
                },
            )
            .await;
            match rx.recv().await {
                Some(crate::protocol::ServerEvent::Done { .. }) => {
                    ControlExecutionResult::new("accepted", None)
                }
                _ => bail!("message was not accepted"),
            }
        }
        ControlAction::CreateSession { initial_prompt, .. } => {
            let id = deterministic_session_id(key);
            ensure_control_session(server, config, &id, initial_prompt.as_deref()).await?;
            ControlExecutionResult::new("created", Some(id))
        }
    }
}

async fn session_is_bound(server: &JcodeControlServer, id: &str, root: &Path) -> bool {
    let Some(agent) = server.sessions.read().await.get(id).cloned() else {
        return false;
    };
    let agent = agent.lock().await;
    agent
        .working_dir()
        .and_then(|path| Path::new(path).canonicalize().ok())
        .as_deref()
        == Some(root)
}

pub(super) fn deterministic_session_id(key: &ReceiptKey) -> String {
    let mut hash = Sha256::new();
    hash.update(b"jcode-hermes-control-session-v1\0");
    hash.update(key.principal().as_str().as_bytes());
    hash.update(b"\0");
    hash.update(key.idempotency_key().as_bytes());
    format!("hermes-control-{:x}", hash.finalize())
}

async fn ensure_control_session(
    server: &JcodeControlServer,
    config: &HermesControlConfig,
    id: &str,
    prompt: Option<&str>,
) -> Result<()> {
    if session_is_bound(server, id, &config.root).await {
        return Ok(());
    }
    let provider = server.provider.fork();
    let request = MultiProvider::model_switch_request_for_session_route(
        &config.route_model,
        None,
        config.route_api_method.as_deref(),
    );
    set_model_with_auth_refresh(provider.as_ref(), &request)
        .context("configured control route unavailable")?;
    if provider.model() != config.route_model {
        bail!("configured control route resolved to a different model")
    }
    let mut session =
        Session::load(id).unwrap_or_else(|_| Session::create_with_id(id.to_string(), None, None));
    session.working_dir = Some(config.root.to_string_lossy().into_owned());
    session.model = Some(config.route_model.clone());
    session.route_api_method = config.route_api_method.clone();
    session.reasoning_effort = config.route_effort.clone();
    session.save()?;
    let registry = Registry::new(provider.clone()).await;
    registry.rebind_working_dir(Some(&config.root)).await;
    let mut agent = Agent::new_with_session(provider, registry, session, None);
    if let Some(effort) = config.route_effort.as_deref() {
        agent.set_reasoning_effort(effort)?;
    }
    if let Some(prompt) = prompt {
        agent.append_user_context_message(prompt, Vec::new())?;
    }
    let agent = Arc::new(Mutex::new(agent));
    register_session_interrupt_queue(
        &server.soft_interrupt_queues,
        id,
        agent.lock().await.soft_interrupt_queue(),
    )
    .await;
    server.sessions.write().await.insert(id.to_string(), agent);
    Ok(())
}

async fn post_receipt(
    client: &HermesControlClient,
    id: &str,
    receipt: &ControlReceipt,
) -> Result<()> {
    match &receipt.state {
        ControlReceiptState::Completed { result, .. }
        | ControlReceiptState::Failed { result, .. } => post_result(client, id, result).await,
        _ => bail!("non-terminal receipt cannot be posted"),
    }
}

async fn post_result(
    client: &HermesControlClient,
    id: &str,
    result: &ControlExecutionResult,
) -> Result<()> {
    let success = result.code == "accepted" || result.code == "created";
    let session_id = (result.code == "created")
        .then_some(result.detail.as_deref())
        .flatten();
    client
        .resolve(
            id,
            HermesResult {
                target_node: &client.config.node,
                status: if success { "completed" } else { "failed" },
                session_id,
                error_code: (!success).then_some(result.code.as_str()),
            },
        )
        .await
}
