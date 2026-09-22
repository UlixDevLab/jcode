//! Persistence and acknowledgement helpers for durable task-DAG materialization.

use super::*;
use crate::decomposition_materializer::{DecompositionPacket, PacketStore, SpecScoutResult};
use crate::server::SessionAgents;
use tokio::sync::broadcast;

#[path = "decomposition_approval_gate.rs"]
mod approval_gate;
#[path = "comm_graph_spec_scout.rs"]
mod spec_scout;

pub(crate) struct GraphSeedRuntime<'a> {
    pub(crate) swarm_event_tx: &'a broadcast::Sender<SwarmEvent>,
    sessions: Option<&'a SessionAgents>,
}

impl<'a> GraphSeedRuntime<'a> {
    pub(crate) fn with_sessions(
        swarm_event_tx: &'a broadcast::Sender<SwarmEvent>,
        sessions: &'a SessionAgents,
    ) -> Self {
        Self {
            swarm_event_tx,
            sessions: Some(sessions),
        }
    }

    pub(crate) fn without_scout(swarm_event_tx: &'a broadcast::Sender<SwarmEvent>) -> Self {
        Self {
            swarm_event_tx,
            sessions: None,
        }
    }
}

pub(super) fn record_applied_and_create_gate(
    packet: &DecompositionPacket,
    plan: &VersionedPlan,
    spec_scout: &SpecScoutResult,
) -> Result<(), String> {
    let store = PacketStore::durable();
    store
        .record_applied(packet, plan)
        .map_err(|error| error.to_string())?;
    approval_gate::create_packet_approval_gate(&store, packet, spec_scout)
        .map(|_| ())
        .map_err(|error| format!("approval gate was not created: {error}"))
}

pub(super) async fn require_spec_scout_and_create_gate(
    packet: &DecompositionPacket,
    plan: &VersionedPlan,
    coordinator_session_id: &str,
    runtime: &GraphSeedRuntime<'_>,
) -> Result<(), String> {
    let store = PacketStore::durable();
    let scout = match store
        .load_spec_scout(packet)
        .map_err(|error| error.to_string())?
    {
        Some(result) => result,
        None => {
            let sessions = runtime
                .sessions
                .ok_or_else(|| "mandatory spec scout runtime is unavailable".to_string())?;
            let result = spec_scout::run(sessions, coordinator_session_id, &packet.root_prompt)
                .await
                .map_err(|error| {
                    format!("mandatory spec scout did not produce a valid result: {error:#}")
                })?;
            store.record_spec_scout(packet, &result).map_err(|error| {
                format!("mandatory spec scout result was not recorded: {error}")
            })?;
            result
        }
    };
    record_applied_and_create_gate(packet, plan, &scout)
}

pub(super) fn graph_size_error(graph: &TaskGraph) -> Option<String> {
    (graph.len() > MAX_PLAN_ITEMS).then(|| {
        format!(
            "plan would contain {} items, exceeding the per-swarm limit of {}; finish or clear stale plan nodes before adding more",
            graph.len(),
            MAX_PLAN_ITEMS
        )
    })
}

pub(super) fn err(client_event_tx: &mpsc::UnboundedSender<ServerEvent>, id: u64, message: String) {
    let _ = client_event_tx.send(ServerEvent::Error {
        id,
        message,
        retry_after_secs: None,
    });
}

/// Shared finalize: persist, broadcast, record a plan-update event, and ack.
#[expect(
    clippy::too_many_arguments,
    reason = "finalize threads through swarm persistence, broadcast, and event-history handles"
)]
pub(super) async fn finalize(
    _id: u64,
    swarm_id: &str,
    req_session_id: &str,
    reason: &str,
    item_count: usize,
    _client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let from_name = swarm_members
        .read()
        .await
        .get(req_session_id)
        .and_then(|member| member.friendly_name.clone());

    let swarm_state = SwarmState {
        members: Arc::clone(swarm_members),
        swarms_by_id: Arc::clone(swarms_by_id),
        plans: Arc::clone(swarm_plans),
        coordinators: Arc::clone(swarm_coordinators),
    };
    persist_swarm_state_for(swarm_id, &swarm_state).await;
    broadcast_swarm_plan(
        swarm_id,
        Some(reason.to_string()),
        swarm_plans,
        swarm_members,
        swarms_by_id,
    )
    .await;
    record_swarm_event(
        event_history,
        event_counter,
        swarm_event_tx,
        req_session_id.to_string(),
        from_name,
        Some(swarm_id.to_string()),
        SwarmEventType::PlanUpdate {
            swarm_id: swarm_id.to_string(),
            item_count,
        },
    )
    .await;
}
