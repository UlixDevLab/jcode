//! Durable decomposition packet persistence and root preparation helpers.

use super::{
    AppliedReceipt, DecompositionPacket, MaterializerError, MissionBinding, PrepareOutcome,
    RequirementCoverageSummary, SpecScoutResult, StableNode,
};
use crate::plan::VersionedPlan;
use crate::protocol::TaskGraphNodeSpec;
use crate::protocol::{PlanGraphStatus, ServerEvent};
use crate::tool::ToolContext;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const STORE_DIR: &str = "swarm-decomposition";
const HASH_PREFIX: &str = "sha256:decomposition:v1:";

#[path = "decomposition_spec_scout_store.rs"]
mod spec_scout_store;

pub(crate) fn seed_node_id_collision(response: &ServerEvent) -> Option<&str> {
    let ServerEvent::Error { message, .. } = response else {
        return None;
    };
    let (_, tail) = message.split_once("duplicate node id '")?;
    let (id, _) = tail.split_once('\'')?;
    (!id.is_empty()).then_some(id)
}

pub(crate) fn plan_graph_node_ids(summary: &PlanGraphStatus) -> HashSet<String> {
    summary
        .ready_ids
        .iter()
        .chain(&summary.blocked_ids)
        .chain(&summary.active_ids)
        .chain(&summary.completed_ids)
        .chain(&summary.failed_ids)
        .chain(&summary.cycle_ids)
        .chain(&summary.unresolved_dependency_ids)
        .cloned()
        .collect()
}

pub(crate) fn seed_retry_scope(ctx: &ToolContext) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ctx.session_id.hash(&mut hasher);
    ctx.message_id.hash(&mut hasher);
    format!("seed-{:08x}", hasher.finish() as u32)
}

pub(crate) struct PreparedMaterialization {
    packet: DecompositionPacket,
    outcome: PrepareOutcome,
}

impl PreparedMaterialization {
    pub(crate) fn requires_seed(&self) -> bool {
        self.outcome != PrepareOutcome::InactiveRevision
    }

    pub(crate) fn seed_request(&self) -> crate::protocol::Request {
        crate::protocol::Request::CommSeedGraph {
            id: 1,
            session_id: self.packet.session_id.clone(),
            mode: Some(self.packet.mode.clone()),
            nodes: self.packet.requested_nodes(),
        }
    }

    pub(crate) fn outcome_message(&self) -> String {
        match self.outcome {
            PrepareOutcome::InactiveRevision => format!(
                "Prepared inactive decomposition packet revision {} ({}).",
                self.packet.revision, self.packet.canonical_hash
            ),
            PrepareOutcome::Prepared => format!(
                "Materialized decomposition packet revision {} ({}) into a task graph.",
                self.packet.revision, self.packet.canonical_hash
            ),
            PrepareOutcome::Replay => format!(
                "Reconciled decomposition packet revision {} ({}) into a task graph.",
                self.packet.revision, self.packet.canonical_hash
            ),
        }
    }
}

pub(crate) fn prepare_root_materialization(
    ctx: &ToolContext,
    root_prompt: Option<String>,
    material_scope: Option<String>,
    revision: Option<usize>,
    mode: Option<String>,
    nodes: Option<Vec<TaskGraphNodeSpec>>,
) -> Result<PreparedMaterialization> {
    let session = crate::session::Session::load(&ctx.session_id).map_err(|error| {
        anyhow::anyhow!("cannot establish materialize_graph authority: {error}")
    })?;
    if session.parent_id.is_some() {
        anyhow::bail!("materialize_graph requires the root coordinator");
    }
    let root_prompt = root_prompt
        .ok_or_else(|| anyhow::anyhow!("'message' is required for materialize_graph"))?;
    let material_scope = material_scope
        .ok_or_else(|| anyhow::anyhow!("'reason' is required for materialize_graph"))?;
    let nodes =
        nodes.ok_or_else(|| anyhow::anyhow!("'nodes' is required for materialize_graph"))?;
    ctx.working_dir
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("materialize_graph requires a project working directory"))?;
    // A root may materialize an ordinary scoped graph without a separate
    // declaration or approval artifact. Root/session ownership and graph
    // validation remain the admission boundary.
    let brief_context = session
        .current_mission_brief
        .as_ref()
        .map(|brief| brief.outcome.as_str())
        .unwrap_or("no current brief");
    let packet = DecompositionPacket::new(
        root_prompt,
        MissionBinding {
            mission_id: "root-session".to_string(),
            revision: 1,
            artifact_hash: format!("root-context:{brief_context}"),
        },
        ctx.session_id.clone(),
        revision.unwrap_or(1) as u64,
        material_scope,
        mode,
        nodes,
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let outcome = PacketStore::durable()
        .prepare(&packet)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(PreparedMaterialization { packet, outcome })
}
pub(crate) struct PacketStore {
    root: PathBuf,
}

impl PacketStore {
    pub(crate) fn durable() -> Self {
        Self::at(crate::storage::durable_state_dir().join(STORE_DIR))
    }

    pub(crate) fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn prepared_path(&self, session_id: &str, revision: u64) -> PathBuf {
        self.root.join(format!(
            "{}-{}.prepared.json",
            sanitize(session_id),
            revision
        ))
    }

    pub(crate) fn applied_path(&self, session_id: &str, revision: u64) -> PathBuf {
        self.root.join(format!(
            "{}-{}.applied.json",
            sanitize(session_id),
            revision
        ))
    }

    pub(crate) fn prepare(
        &self,
        packet: &DecompositionPacket,
    ) -> Result<PrepareOutcome, MaterializerError> {
        fs::create_dir_all(&self.root)?;
        let path = self.prepared_path(&packet.session_id, packet.revision);
        if path.exists() {
            let existing: DecompositionPacket = read_json(&path)?;
            if existing.canonical_hash != packet.canonical_hash || existing != *packet {
                return Err(MaterializerError::PacketConflict {
                    revision: packet.revision,
                });
            }
            return Ok(if packet.is_active() {
                PrepareOutcome::Replay
            } else {
                PrepareOutcome::InactiveRevision
            });
        }
        write_json(&path, packet)?;
        Ok(if packet.is_active() {
            PrepareOutcome::Prepared
        } else {
            PrepareOutcome::InactiveRevision
        })
    }

    pub(crate) fn active_prepared_for(
        &self,
        session_id: &str,
    ) -> Result<Option<DecompositionPacket>, MaterializerError> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Ok(None);
        };
        let mut packets = entries
            .flatten()
            .filter_map(|entry| read_json::<DecompositionPacket>(&entry.path()).ok())
            .filter(|packet| packet.session_id == session_id && packet.is_active())
            .collect::<Vec<_>>();
        packets.sort_by_key(|packet| packet.revision);
        Ok(packets.pop())
    }

    pub(crate) fn record_applied(
        &self,
        packet: &DecompositionPacket,
        plan: &VersionedPlan,
    ) -> Result<(), MaterializerError> {
        if !packet.matches_plan(plan) {
            return Err(MaterializerError::GraphMismatch);
        }
        let receipt = AppliedReceipt {
            session_id: packet.session_id.clone(),
            revision: packet.revision,
            packet_hash: packet.canonical_hash.clone(),
            structural_digest: packet.structural_digest.clone(),
        };
        let path = self.applied_path(&packet.session_id, packet.revision);
        if path.exists() {
            let existing: AppliedReceipt = read_json(&path)?;
            if existing != receipt {
                return Err(MaterializerError::PacketConflict {
                    revision: packet.revision,
                });
            }
            return Ok(());
        }
        write_json(&path, &receipt)
    }

    #[cfg(test)]
    pub(crate) fn receipt(&self, session_id: &str, revision: u64) -> Option<AppliedReceipt> {
        read_json(&self.applied_path(session_id, revision)).ok()
    }
}

pub(super) fn normalize_mode(mode: Option<&str>) -> Result<String, MaterializerError> {
    match mode.unwrap_or("light").trim().to_ascii_lowercase().as_str() {
        "light" => Ok("light".to_string()),
        "deep" => Ok("deep".to_string()),
        _ => Err(MaterializerError::InvalidMode),
    }
}

pub(super) fn normalize_nodes(
    mut nodes: Vec<StableNode>,
) -> Result<Vec<StableNode>, MaterializerError> {
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    for node in &mut nodes {
        node.depends_on.sort();
        node.depends_on.dedup();
    }
    if nodes.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(MaterializerError::Graph("duplicate node id".to_string()));
    }
    Ok(nodes)
}

pub(super) fn canonical_manifest(nodes: &[StableNode]) -> Vec<u8> {
    serde_json::to_vec(nodes).expect("stable node manifest serializes")
}

pub(super) fn canonical_packet(
    root_prompt: &str,
    mission: &MissionBinding,
    session_id: &str,
    revision: u64,
    material_scope: &str,
    mode: &str,
    nodes: &[StableNode],
    structural_digest: &str,
    coverage: &RequirementCoverageSummary,
) -> Vec<u8> {
    if coverage.snapshot_present {
        serde_json::to_vec(&(
            "jcode.decomposition-packet/canonical/v2",
            root_prompt,
            mission,
            session_id,
            revision,
            material_scope,
            mode,
            nodes,
            structural_digest,
            coverage,
        ))
    } else {
        serde_json::to_vec(&(
            "jcode.decomposition-packet/canonical/v1",
            root_prompt,
            mission,
            session_id,
            revision,
            material_scope,
            mode,
            nodes,
            structural_digest,
        ))
    }
    .expect("decomposition packet canonical fields serialize")
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{HASH_PREFIX}{:x}", Sha256::digest(bytes))
}

fn sanitize(session_id: &str) -> String {
    session_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, MaterializerError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), MaterializerError> {
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(value)?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(session_id: &str, working_dir: &Path) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            message_id: "materialize-test".to_string(),
            tool_call_id: "materialize-test".to_string(),
            working_dir: Some(working_dir.to_path_buf()),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            structural_review_authority: None,
            execution_mode: crate::tool::ToolExecutionMode::Direct,
        }
    }
    fn nodes() -> Vec<TaskGraphNodeSpec> {
        vec![TaskGraphNodeSpec {
            id: "node".to_string(),
            content: "implement scope".to_string(),
            kind: None,
            depends_on: Vec::new(),
            priority: 1,
        }]
    }
    #[test]
    fn undeclared_root_materializes_but_child_is_denied() {
        let _lock = crate::storage::lock_test_env();
        let temp = tempfile::tempdir().expect("tempdir");
        let previous_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_HOME", temp.path());
        let mut root =
            crate::session::Session::create_with_id("materialize-root".into(), None, None);
        root.save().expect("save root");
        assert!(
            prepare_root_materialization(
                &context(&root.id, temp.path()),
                Some("scope".into()),
                Some("scope".into()),
                None,
                Some("light".into()),
                Some(nodes())
            )
            .is_ok()
        );
        let mut child = crate::session::Session::create_with_id(
            "materialize-child".into(),
            Some(root.id.clone()),
            None,
        );
        child.save().expect("save child");
        let denied = match prepare_root_materialization(
            &context(&child.id, temp.path()),
            Some("scope".into()),
            Some("scope".into()),
            None,
            Some("light".into()),
            Some(nodes()),
        ) {
            Ok(_) => panic!("child materialization must be denied"),
            Err(error) => error,
        };
        assert!(denied.to_string().contains("root coordinator"));
        match previous_home {
            Some(value) => crate::env::set_var("JCODE_HOME", value),
            None => crate::env::remove_var("JCODE_HOME"),
        }
    }
}
