//! Server-owned spawn working-directory resolution.

use super::{SessionAgents, SwarmMember};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(super) async fn resolve_spawn_working_dir(
    requested_working_dir: Option<String>,
    req_session_id: &str,
    sessions: &SessionAgents,
    swarm_members: &Arc<RwLock<HashMap<String, SwarmMember>>>,
) -> Option<String> {
    if let Some(explicit) = requested_working_dir
        .filter(|dir| !dir.trim().is_empty())
        .filter(|dir| usable_working_dir(dir, "explicit"))
    {
        return Some(explicit);
    }

    if let Some(agent_dir) = {
        let agent_sessions = sessions.read().await;
        agent_sessions.get(req_session_id).and_then(|agent| {
            agent
                .try_lock()
                .ok()
                .and_then(|agent_guard| agent_guard.working_dir().map(str::to_string))
        })
    } && !agent_dir.trim().is_empty()
    {
        return Some(agent_dir);
    }

    swarm_members
        .read()
        .await
        .get(req_session_id)
        .and_then(|member| member.working_dir.as_ref())
        .map(|dir| dir.display().to_string())
        .filter(|dir| !dir.trim().is_empty())
}

fn usable_working_dir(dir: &str, source: &str) -> bool {
    let path = Path::new(dir.trim());
    if path.is_dir() {
        return true;
    }
    crate::logging::warn(&format!(
        "swarm spawn: ignoring {source} working_dir {dir:?} because it is not an existing \
         directory; falling back to the requesting session's directory"
    ));
    false
}
