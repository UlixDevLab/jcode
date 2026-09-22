//! Opt-in, pull-only Hermes transport for the narrow `jcode_control.v1` action set.
//!
//! Hermes authenticates transport records. This module still validates the target,
//! configured principal/root/route binding, and locally durable receipt before it
//! ever touches a Jcode session. It is deliberately not a listener.

mod executor;
mod transport;

use super::Server;
use crate::agent::Agent;
use crate::jcode_control::ControlReceiptStore;
use crate::provider::Provider;
use std::sync::Arc;
use tokio::sync::Mutex;

use transport::{
    HermesControlClient, HermesControlConfig, HermesControlRecord, HermesResult, HermesSubmission,
};

pub(super) fn spawn_if_configured(server: &Server) {
    let Some(config) = HermesControlConfig::from_environment() else {
        crate::logging::info(
            "Hermes control disabled: explicit transport/node/principal/root/route configuration incomplete",
        );
        return;
    };
    let Ok(receipts) = ControlReceiptStore::from_jcode_home() else {
        crate::logging::warn("Hermes control disabled: local receipt authority unavailable");
        return;
    };
    let client = HermesControlClient::new(config);
    // Control is another pull-only ingress consumer. Sharing this lifecycle
    // cancellation signal ensures shutdown never leaves its poll task running.
    let cancellation = server.hermes_gate_ingress_cancellation.clone();
    let server = server.clone_for_jcode_control();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(client.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = ticker.tick() => {
                    if let Ok(records) = client.pending().await {
                        for record in records {
                            let _ = executor::process_record(&server, &client, &receipts, record).await;
                        }
                    }
                }
            }
        }
    });
}

/// The server fields needed by one executor are cloned once, avoiding a broad
/// public control API and keeping transport ownership inside this submodule.
#[derive(Clone)]
pub(super) struct JcodeControlServer {
    pub(super) provider: Arc<dyn Provider>,
    pub(super) sessions:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<Mutex<Agent>>>>>,
    pub(super) soft_interrupt_queues: super::SessionInterruptQueues,
    pub(super) client_connections:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, super::ClientConnectionInfo>>>,
    pub(super) swarm_members:
        Arc<tokio::sync::RwLock<std::collections::HashMap<String, super::SwarmMember>>>,
    pub(super) swarms_by_id: Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, std::collections::HashSet<String>>>,
    >,
    pub(super) event_history:
        Arc<tokio::sync::RwLock<std::collections::VecDeque<super::SwarmEvent>>>,
    pub(super) event_counter: Arc<std::sync::atomic::AtomicU64>,
    pub(super) swarm_event_tx: tokio::sync::broadcast::Sender<super::SwarmEvent>,
}

impl Server {
    fn clone_for_jcode_control(&self) -> JcodeControlServer {
        JcodeControlServer {
            provider: Arc::clone(&self.provider),
            sessions: Arc::clone(&self.sessions),
            soft_interrupt_queues: Arc::clone(&self.soft_interrupt_queues),
            client_connections: Arc::clone(&self.client_connections),
            swarm_members: Arc::clone(&self.swarm_state.members),
            swarms_by_id: Arc::clone(&self.swarm_state.swarms_by_id),
            event_history: Arc::clone(&self.event_history),
            event_counter: Arc::clone(&self.event_counter),
            swarm_event_tx: self.swarm_event_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
