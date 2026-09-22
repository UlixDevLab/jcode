//! Best-effort daemon-start recovery for already-restored gate targets.
//!
//! The durable ledger remains the authorization boundary. This coordinator only
//! enumerates answered, unconsumed directives after the server is live and
//! offers each one to an agent that is already in the server registry. It never
//! restores missing sessions, so a missing target remains unconsumed for a
//! later exact-session restoration owner.

use super::{SessionAgents, SwarmMember};
use crate::gates::GateStore;
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct GateStartupRecoveryStats {
    pub(super) discovered: usize,
    pub(super) delivered: usize,
    pub(super) deferred_for_restore: usize,
    pub(super) repaired_after_crash: usize,
    pub(super) no_directive: usize,
    pub(super) delivery_failures: usize,
    pub(super) scan_failed: bool,
}

/// Recover durable directives without allowing a ledger read failure to block
/// an already-listening daemon. The warning is deliberately bounded because a
/// malformed ledger entry can otherwise make startup logs unbounded too.
pub(super) async fn recover_unconsumed_gate_directives_on_startup(
    sessions: &SessionAgents,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> GateStartupRecoveryStats {
    let store = match GateStore::from_jcode_home() {
        Ok(store) => store,
        Err(error) => {
            crate::logging::warn(&format!(
                "Gate startup recovery skipped: unable to open durable ledger: {}",
                bounded_error(&error.to_string())
            ));
            return GateStartupRecoveryStats {
                scan_failed: true,
                ..Default::default()
            };
        }
    };

    match recover_unconsumed_gate_directives_for_existing_sessions(&store, sessions, swarm_members)
        .await
    {
        Ok(stats) => {
            if stats.discovered > 0 || stats.delivery_failures > 0 {
                crate::logging::info(&format!(
                    "Gate startup recovery: discovered={}, delivered={}, deferred={}, repaired={}, no_directive={}, delivery_failures={}",
                    stats.discovered,
                    stats.delivered,
                    stats.deferred_for_restore,
                    stats.repaired_after_crash,
                    stats.no_directive,
                    stats.delivery_failures,
                ));
            }
            stats
        }
        Err(error) => {
            crate::logging::warn(&format!(
                "Gate startup recovery scan failed; daemon remains ready: {}",
                bounded_error(&error.to_string())
            ));
            GateStartupRecoveryStats {
                scan_failed: true,
                ..Default::default()
            }
        }
    }
}

/// Scan only validated answered/unconsumed directives and deliver them only to
/// exact session IDs that are already restored in the server registry.
pub(super) async fn recover_unconsumed_gate_directives_for_existing_sessions(
    store: &GateStore,
    sessions: &SessionAgents,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Result<GateStartupRecoveryStats> {
    let directives = store.load_unconsumed_directives()?;
    let mut stats = GateStartupRecoveryStats {
        discovered: directives.len(),
        ..Default::default()
    };

    for directive in directives {
        let agent = sessions.read().await.get(&directive.session_id).cloned();
        let Some(agent) = agent else {
            stats.deferred_for_restore += 1;
            continue;
        };

        let event_tx = super::state::session_event_fanout_sender(
            directive.session_id.clone(),
            Arc::clone(swarm_members),
        );
        match super::gate_resume::consume_and_deliver_to_restored_session(
            store,
            &directive.gate_id,
            &directive.session_id,
            Utc::now(),
            agent,
            event_tx,
        )
        .await
        {
            Ok(super::gate_resume::GateResumeRestoredOutcome::DeliveredRestoredSession) => {
                stats.delivered += 1;
            }
            Ok(super::gate_resume::GateResumeRestoredOutcome::RepairedAfterCrash) => {
                stats.repaired_after_crash += 1;
            }
            Ok(super::gate_resume::GateResumeRestoredOutcome::NoDirective) => {
                stats.no_directive += 1;
            }
            Err(error) => {
                stats.delivery_failures += 1;
                crate::logging::warn(&format!(
                    "Gate startup recovery delivery failed for session {} gate {}: {}",
                    directive.session_id,
                    directive.gate_id,
                    bounded_error(&error.to_string())
                ));
            }
        }
    }

    Ok(stats)
}

fn bounded_error(error: &str) -> String {
    const LIMIT: usize = 240;
    let mut bounded: String = error.chars().take(LIMIT).collect();
    if error.chars().nth(LIMIT).is_some() {
        bounded.push('…');
    }
    bounded
}

#[cfg(test)]
#[path = "gate_recovery_startup_tests.rs"]
mod tests;
