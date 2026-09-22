//! Materialized task-graph seeding handler.

use super::*;
use crate::decomposition_materializer::{PacketStore, reconcile_packet};

/// Seed (or re-seed) the swarm task DAG from a batch of node specs.
#[expect(
    clippy::too_many_arguments,
    reason = "swarm op threads runtime handles"
)]
pub(crate) async fn handle_comm_seed_graph(
    id: u64,
    req_session_id: String,
    mode: Option<String>,
    nodes: Vec<TaskGraphNodeSpec>,
    client_event_tx: &mpsc::UnboundedSender<ServerEvent>,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    event_history: &Arc<RwLock<std::collections::VecDeque<SwarmEvent>>>,
    event_counter: &Arc<std::sync::atomic::AtomicU64>,
    runtime: GraphSeedRuntime<'_>,
) {
    let Some(swarm_id) = swarm_id_for(&req_session_id, swarm_members).await else {
        err(client_event_tx, id, "Not in a swarm.".to_string());
        return;
    };

    // A deep-mode seeder is usually a solo agent. Elect it coordinator (when no
    // live coordinator exists) so it can actually dispatch the graph it seeds via
    // the coordinator-gated assign/run_plan paths.
    ensure_seeder_can_coordinate(
        &swarm_id,
        &req_session_id,
        swarm_members,
        swarm_coordinators,
    )
    .await;

    let prepared_packet = match PacketStore::durable().active_prepared_for(&req_session_id) {
        Ok(packet) => packet,
        Err(error) => {
            err(
                client_event_tx,
                id,
                format!("Seed rejected: cannot load prepared packet: {error}"),
            );
            return;
        }
    };
    if let Some(packet) = &prepared_packet
        && !packet.request_matches(mode.as_deref(), &nodes)
    {
        err(
            client_event_tx,
            id,
            "Seed rejected: requested graph does not match its prepared decomposition packet."
                .to_string(),
        );
        return;
    }
    let specs: Vec<NodeSpec> = prepared_packet
        .as_ref()
        .map(|packet| packet.requested_nodes())
        .unwrap_or(nodes)
        .into_iter()
        .map(spec_from_wire)
        .collect();
    let count = specs.len();

    // Resolve the plan mode. The model is *asked* to pass `mode:"deep"` when it is
    // running at `swarm-deep` effort, but it frequently forgets. Rather than
    // silently downgrading a deep-effort session to light (which disables the
    // gates + artifact validation that define deep mode), default the mode from
    // the seeder's recorded reasoning effort when the caller did not specify one.
    // An explicit `mode` always wins so a caller can still opt into light.
    let resolved_mode = prepared_packet
        .as_ref()
        .map(|packet| packet.mode.clone())
        .or(mode)
        .or_else(|| {
            crate::session_effort::session_effort(&req_session_id)
                .filter(|effort| crate::prompt::is_deep_swarm_effort(effort))
                .map(|_| "deep".to_string())
        });

    let result = {
        let mut plans = swarm_plans.write().await;
        let plan = plans
            .entry(swarm_id.clone())
            .or_insert_with(VersionedPlan::new);
        if let Some(packet) = &prepared_packet {
            if !packet.request_matches(Some(&packet.mode), &packet.requested_nodes()) {
                return err(
                    client_event_tx,
                    id,
                    "Seed rejected: invalid prepared packet.".to_string(),
                );
            }
            plan.participants.insert(req_session_id.clone());
            reconcile_packet(packet, plan)
                .map(|_| ())
                .map_err(|error| error.to_string())
        } else {
            if let Some(mode) = resolved_mode {
                // Guard against silent rigor downgrades: re-seeding an existing deep
                // plan as light would strip the gates + artifact validation from all
                // nodes already in flight. Deepening (light -> deep) or re-stating
                // the same mode is fine; only the downgrade of a non-empty deep plan
                // is rejected.
                let downgrades_deep = plan.mode.eq_ignore_ascii_case("deep")
                    && !mode.eq_ignore_ascii_case("deep")
                    && !plan.items.is_empty();
                if downgrades_deep {
                    err(
                        client_event_tx,
                        id,
                        "Seed rejected: this swarm already has a non-empty deep-mode plan; \
                     seeding with mode=light would silently strip its gates and artifact \
                     validation. Omit `mode` to keep deep, or finish/clear the current plan first."
                            .to_string(),
                    );
                    return;
                }
                plan.mode = mode;
            }
            plan.participants.insert(req_session_id.clone());
            let mut graph = to_task_graph(plan);
            let before = graph.clone();
            match dag::seed(&mut graph, specs) {
                Ok(()) => match graph_size_error(&graph) {
                    Some(message) => Err(message),
                    None => {
                        if graph != before {
                            apply_task_graph(plan, &graph);
                            plan.version += 1;
                        }
                        Ok(())
                    }
                },
                Err(e) => Err(e.to_string()),
            }
        }
    };

    match result {
        Ok(()) => {
            if let Some(packet) = &prepared_packet {
                let plan = swarm_plans.read().await.get(&swarm_id).cloned();
                let Some(plan) = plan else {
                    err(
                        client_event_tx,
                        id,
                        "Seed rejected: materialized plan disappeared before durable read-back."
                            .to_string(),
                    );
                    return;
                };
                if let Err(error) = materialization::require_spec_scout_and_create_gate(
                    packet,
                    &plan,
                    &req_session_id,
                    &runtime,
                )
                .await
                {
                    err(
                        client_event_tx,
                        id,
                        format!(
                            "Seed rejected: APPLIED receipt or approval gate was not recorded: {error}"
                        ),
                    );
                    return;
                }
            }
            finalize(
                id,
                &swarm_id,
                &req_session_id,
                "task_graph_seed",
                count,
                client_event_tx,
                swarm_members,
                swarms_by_id,
                swarm_plans,
                swarm_coordinators,
                event_history,
                event_counter,
                runtime.swarm_event_tx,
            )
            .await;
            let _ = client_event_tx.send(ServerEvent::Done { id });
        }
        Err(e) => err(client_event_tx, id, format!("Seed rejected: {e}")),
    }
}
