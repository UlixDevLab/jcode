//! Server handlers for the task-DAG mutation ops (seed/expand/complete/inject).
//!
//! These are the live counterparts of the validated engine ops in
//! `jcode_plan::dag`. Each handler lifts the swarm's current `VersionedPlan` into
//! a `TaskGraph` (via `jcode_plan::bridge`), applies the engine op (which enforces
//! acyclicity, ownership, gate insertion, and artifact validation), lowers the
//! result back into the plan, then persists and broadcasts using the existing
//! swarm machinery. This keeps a single source of truth and reuses the scheduler,
//! persistence, and TUI broadcast paths.

use super::{
    SwarmEvent, SwarmEventType, SwarmMember, SwarmState, VersionedPlan, broadcast_swarm_plan,
    persist_swarm_state_for, record_swarm_event,
};
use crate::protocol::ServerEvent;
use crate::protocol::TaskGraphNodeSpec;
use jcode_plan::MAX_PLAN_ITEMS;
use jcode_plan::bridge::{apply_task_graph, parse_kind, to_task_graph};
use jcode_plan::dag::{self, HandoffArtifact, NodeSpec, NodeStatus, TaskGraph};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::{RwLock, broadcast};

#[path = "comm_graph_materialization.rs"]
mod materialization;
pub(crate) use materialization::GraphSeedRuntime;
use materialization::{err, finalize, graph_size_error};

fn spec_from_wire(spec: TaskGraphNodeSpec) -> NodeSpec {
    NodeSpec {
        id: Some(spec.id),
        content: spec.content,
        kind: parse_kind(spec.kind.as_deref()),
        depends_on: spec.depends_on,
        priority: spec.priority,
    }
}

async fn swarm_id_for(
    session_id: &str,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Option<String> {
    swarm_members
        .read()
        .await
        .get(session_id)
        .and_then(|member| member.swarm_id.clone())
}

/// Ensure the seeding session can actually drive the graph it just created.
///
/// Deep-mode sessions are frequently solo `agent`s with no coordinator elected,
/// yet `assign_task` / `assign_next` / `run_plan` are coordinator-gated. Without
/// this, a fresh deep-mode agent can seed a task graph but then cannot dispatch
/// any of it. We elect the seeder as coordinator when the swarm has no *live*
/// coordinator, mirroring the self-promote rule used by `assign_role`. A live,
/// non-headless coordinator is left untouched so a real coordinator is never
/// displaced by a worker that happens to seed.
///
/// Returns true when the seeder was (or already is) the coordinator afterwards.
async fn ensure_seeder_can_coordinate(
    swarm_id: &str,
    seeder_session_id: &str,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
) -> bool {
    // 1. Read the current coordinator id without holding the lock across the
    //    liveness check (matches the non-nested lock pattern used elsewhere).
    let current = swarm_coordinators.read().await.get(swarm_id).cloned();
    match &current {
        Some(coord) if coord == seeder_session_id => return true,
        _ => {}
    }

    // 2. Decide whether the existing coordinator is still a live driver.
    let coordinator_is_live = match &current {
        Some(coord) => {
            let members = swarm_members.read().await;
            members
                .get(coord)
                .map(|member| !member.event_tx.is_closed() && !member.is_headless)
                .unwrap_or(false)
        }
        None => false,
    };
    if coordinator_is_live {
        return false;
    }

    // 3. Promote the seeder; demote any prior (stale) coordinator member. Re-check
    //    under the write lock that the coordinator is still the one we inspected
    //    (compare-and-swap): two concurrent seeders race here, and the loser must
    //    not silently displace the winner it never liveness-checked.
    let prior = {
        let mut coordinators = swarm_coordinators.write().await;
        if coordinators.get(swarm_id) != current.as_ref() {
            // Someone else changed the coordinator between our read and write.
            return coordinators.get(swarm_id).map(String::as_str) == Some(seeder_session_id);
        }
        coordinators.insert(swarm_id.to_string(), seeder_session_id.to_string())
    };
    {
        let mut members = swarm_members.write().await;
        if let Some(member) = members.get_mut(seeder_session_id) {
            member.role = "coordinator".to_string();
        }
        if let Some(prior) = prior
            && prior != seeder_session_id
            && let Some(member) = members.get_mut(&prior)
        {
            member.role = "agent".to_string();
        }
    }
    true
}

/// Auto-claim a queued node for the participant that is trying to mutate it.
///
/// Seeded nodes are unowned until dispatch, but the deep-mode contract tells the
/// seeding agent to `expand_node`/`complete_node` its own nodes, and the assign
/// path refuses self-assignment — so without this a solo deep seeder could never
/// legally touch any node it seeded (observed live as "Complete rejected: actor
/// does not own node"). Similarly, assignment to a client-attached worker leaves
/// the item `queued` (the server-run flip to `running` is skipped when a live
/// client owns the turn), so the assignee's own complete/expand would bounce with
/// "invalid state Queued".
///
/// Claiming is safe only when the node is genuinely available to this actor:
/// queued, with every dependency done (enforced by `dispatch`), and either
/// unowned or already assigned to this same actor. A node owned by someone else
/// is never touched — the engine's `NotOwner` check still applies.
fn claim_queued_node_for_actor(graph: &mut TaskGraph, node_id: &str, actor: &str) {
    let claimable = graph.get(node_id).is_some_and(|node| {
        node.status == NodeStatus::Queued
            && node.owner.as_deref().is_none_or(|owner| owner == actor)
    });
    if claimable {
        // `dispatch` re-validates queued status and dependency satisfaction; if
        // deps are not done the claim is skipped and the engine op reports the
        // real error.
        let _ = dag::dispatch(graph, node_id, actor);
    }
}

#[path = "comm_graph_seed.rs"]
mod seed;
pub(super) use seed::handle_comm_seed_graph;

/// Decompose a node the caller owns into a child sub-DAG.
#[expect(
    clippy::too_many_arguments,
    reason = "swarm op threads runtime handles"
)]
pub(super) async fn handle_comm_expand_node(
    id: u64,
    req_session_id: String,
    node_id: String,
    children: Vec<TaskGraphNodeSpec>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let Some(swarm_id) = swarm_id_for(&req_session_id, swarm_members).await else {
        err(client_event_tx, id, "Not in a swarm.".to_string());
        return;
    };
    let specs: Vec<NodeSpec> = children.into_iter().map(spec_from_wire).collect();
    let count = specs.len();

    let result = {
        let mut plans = swarm_plans.write().await;
        let Some(plan) = plans.get_mut(&swarm_id) else {
            err(client_event_tx, id, "No plan for this swarm.".to_string());
            return;
        };
        let mut graph = to_task_graph(plan);
        claim_queued_node_for_actor(&mut graph, &node_id, &req_session_id);
        match dag::expand_node(&mut graph, &node_id, &req_session_id, specs) {
            Ok(_) => match graph_size_error(&graph) {
                Some(message) => Err(message),
                None => {
                    apply_task_graph(plan, &graph);
                    plan.version += 1;
                    Ok(())
                }
            },
            Err(e) => Err(e.to_string()),
        }
    };

    match result {
        Ok(()) => {
            finalize(
                id,
                &swarm_id,
                &req_session_id,
                "task_graph_expand",
                count,
                client_event_tx,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                swarm_event_tx,
            )
            .await;
            let _ = client_event_tx.send(ServerEvent::Done { id });
        }
        Err(e) => err(client_event_tx, id, format!("Expand rejected: {e}")),
    }
}

/// Complete a node the caller owns with a typed handoff artifact.
#[expect(
    clippy::too_many_arguments,
    reason = "swarm op threads runtime handles"
)]
pub(super) async fn handle_comm_complete_node(
    id: u64,
    req_session_id: String,
    node_id: String,
    artifact_json: String,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let Some(swarm_id) = swarm_id_for(&req_session_id, swarm_members).await else {
        err(client_event_tx, id, "Not in a swarm.".to_string());
        return;
    };

    let artifact: HandoffArtifact = match serde_json::from_str(&artifact_json) {
        Ok(artifact) => artifact,
        Err(e) => {
            err(client_event_tx, id, format!("Invalid artifact JSON: {e}"));
            return;
        }
    };

    let result = {
        let mut plans = swarm_plans.write().await;
        let Some(plan) = plans.get_mut(&swarm_id) else {
            err(client_event_tx, id, "No plan for this swarm.".to_string());
            return;
        };
        let mut graph = to_task_graph(plan);
        claim_queued_node_for_actor(&mut graph, &node_id, &req_session_id);
        match dag::complete_node(&mut graph, &node_id, &req_session_id, artifact) {
            Ok(()) => {
                apply_task_graph(plan, &graph);
                plan.version += 1;
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    };

    match result {
        Ok(()) => {
            finalize(
                id,
                &swarm_id,
                &req_session_id,
                "task_graph_complete",
                1,
                client_event_tx,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                swarm_event_tx,
            )
            .await;
            let _ = client_event_tx.send(ServerEvent::Done { id });
        }
        Err(e) => err(client_event_tx, id, format!("Complete rejected: {e}")),
    }
}

/// Inject gap/fix nodes from a gate the caller owns.
#[expect(
    clippy::too_many_arguments,
    reason = "swarm op threads runtime handles"
)]
pub(super) async fn handle_comm_inject_gap(
    id: u64,
    req_session_id: String,
    gate_id: String,
    nodes: Vec<TaskGraphNodeSpec>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    swarm_event_tx: &broadcast::Sender<SwarmEvent>,
) {
    let Some(swarm_id) = swarm_id_for(&req_session_id, swarm_members).await else {
        err(client_event_tx, id, "Not in a swarm.".to_string());
        return;
    };
    let specs: Vec<NodeSpec> = nodes.into_iter().map(spec_from_wire).collect();
    let count = specs.len();

    let result = {
        let mut plans = swarm_plans.write().await;
        let Some(plan) = plans.get_mut(&swarm_id) else {
            err(client_event_tx, id, "No plan for this swarm.".to_string());
            return;
        };
        let mut graph = to_task_graph(plan);
        claim_queued_node_for_actor(&mut graph, &gate_id, &req_session_id);
        match dag::inject_from_gate(&mut graph, &gate_id, &req_session_id, specs) {
            Ok(_) => match graph_size_error(&graph) {
                Some(message) => Err(message),
                None => {
                    apply_task_graph(plan, &graph);
                    plan.version += 1;
                    Ok(())
                }
            },
            Err(e) => Err(e.to_string()),
        }
    };

    match result {
        Ok(()) => {
            finalize(
                id,
                &swarm_id,
                &req_session_id,
                "task_graph_inject_gap",
                count,
                client_event_tx,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                swarm_event_tx,
            )
            .await;
            let _ = client_event_tx.send(ServerEvent::Done { id });
        }
        Err(e) => err(client_event_tx, id, format!("Inject rejected: {e}")),
    }
}
