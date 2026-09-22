//! Wire-only mission dispatch binding.
//!
//! This type deliberately uses primitives rather than mission-core domain types:
//! protocol and mission-core are sibling leaves.  Its fields are frozen by the
//! R1v2 approval packet and an absent binding remains compatible with older
//! `comm_spawn` frames.

use serde::{Deserialize, Serialize};

/// Exact mission authorization attached to a prospective swarm dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionDispatchBinding {
    pub mission_id: String,
    pub revision: u64,
    pub artifact_hash: String,
    /// The approved active-revision plan hash. Missing values from frames sent
    /// before admission existed decode as empty and fail closed at admission.
    #[serde(default)]
    pub plan_hash: String,
    /// The approval's exact digest of its approved scope. As with plan_hash,
    /// an older binding can decode but cannot authorize a new worker.
    #[serde(default)]
    pub scope_digest: String,
    pub wave_id: String,
    pub task_id: String,
    /// Exact write paths requested for this dispatch.
    pub write_set: Vec<String>,
    /// Exact read-only paths requested by an internal context scout. Root
    /// dispatches must leave this empty, and scouts must leave write_set empty.
    #[serde(default)]
    pub read_set: Vec<String>,
    pub entity_ids: Vec<String>,
    pub role: String,
    pub spawn_mode: String,
}
