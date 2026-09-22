use super::{
    CoordinatorSpawnIdentity, SessionAgents, SwarmSpawnSelection, VersionedPlan,
    create_headless_session,
};
use crate::provider::Provider;
use crate::server::{SessionInterruptQueues, SwarmMember};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

const CONTEXT_SCOUT_LUNA_MODEL: &str = "gpt-5.6-luna";

/// Fixed selection for the internal read-only context scout. The model is the
/// active fast Luna route while the coordinator's account and auth route remain
/// authoritative, matching ordinary spawned-worker inheritance.
pub(super) fn context_scout_luna_selection(
    coordinator: &CoordinatorSpawnIdentity,
) -> SwarmSpawnSelection {
    SwarmSpawnSelection {
        model: Some(CONTEXT_SCOUT_LUNA_MODEL.to_string()),
        provider_key: coordinator.provider_key.clone(),
        route_api_method: coordinator.route_api_method.clone(),
    }
}

/// Create an internal read-only ContextScout worker. The fixed model, real
/// project memory, and disabled self-development surface are enforced here,
/// rather than inferred from a label or a user-configurable worker model.
#[allow(
    dead_code,
    reason = "A0.2b will bind this factory to fresh-turn dispatch; this slice must not dispatch it yet"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "context scout creation shares the normal headless session dependencies"
)]
pub(super) async fn create_context_scout_luna_session(
    sessions: &SessionAgents,
    global_session_id: &Arc<RwLock<String>>,
    provider_template: &Arc<dyn Provider>,
    command: &str,
    coordinator: &CoordinatorSpawnIdentity,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
    swarms_by_id: &Arc<RwLock<HashMap<String, HashSet<String>>>>,
    swarm_coordinators: &Arc<RwLock<HashMap<String, String>>>,
    swarm_plans: &Arc<RwLock<HashMap<String, VersionedPlan>>>,
    soft_interrupt_queues: &SessionInterruptQueues,
    effort_override: Option<String>,
    mcp_pool: Option<Arc<crate::mcp::SharedMcpPool>>,
    report_back_to_session_id: Option<String>,
) -> anyhow::Result<String> {
    let selection = context_scout_luna_selection(coordinator);
    let created = create_headless_session(
        sessions,
        global_session_id,
        provider_template,
        command,
        swarm_members,
        swarms_by_id,
        swarm_coordinators,
        None,
        swarm_plans,
        soft_interrupt_queues,
        false,
        selection.model,
        selection.provider_key,
        selection.route_api_method,
        effort_override,
        mcp_pool,
        report_back_to_session_id,
        super::super::headless::HeadlessMemoryScope::RealProject,
    )
    .await?;

    let session_id = serde_json::from_str::<serde_json::Value>(&created)
        .ok()
        .and_then(|value| {
            value
                .get("session_id")
                .and_then(|session_id| session_id.as_str())
                .map(str::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to parse context scout session id"))?;
    let agent = sessions
        .read()
        .await
        .get(&session_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Context scout session {session_id} was not registered"))?;
    // Headless creation finishes registration before returning and this factory
    // is not yet scheduled, so installing the profile cannot expose one normal
    // worker turn before the strict allow-list takes effect.
    agent.lock().await.apply_context_scout_luna_profile();

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_scout_luna_selection_forces_fast_model_and_inherits_account_route() {
        let selection = context_scout_luna_selection(&CoordinatorSpawnIdentity {
            model: Some("gpt-5.6-terra".to_string()),
            provider_key: Some("openai-oauth".to_string()),
            route_api_method: Some("openai-oauth".to_string()),
            is_canary: false,
        });

        assert_eq!(selection.model.as_deref(), Some(CONTEXT_SCOUT_LUNA_MODEL));
        assert_eq!(selection.provider_key.as_deref(), Some("openai-oauth"));
        assert_eq!(selection.route_api_method.as_deref(), Some("openai-oauth"));
    }
}
