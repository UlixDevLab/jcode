//! Durable, immutable intent for root-owned task-DAG materialization.
//!
//! The packet is deliberately stored beside the existing durable swarm state,
//! but remains a separate authority from `VersionedPlan`: it records what the
//! root intended, while the plan remains the executable graph. A PREPARED
//! packet always precedes graph mutation; APPLIED is a receipt written only
//! after the reconciled graph matches the packet's structural digest.

pub(crate) use crate::decomposition_coverage::{
    RequirementCoverageSummary, WaveRequirementCoverage,
};
use crate::plan::VersionedPlan;
use crate::protocol::TaskGraphNodeSpec;
use jcode_plan::bridge::{apply_task_graph, parse_kind, to_task_graph};
use jcode_plan::dag::{self, NodeSpec};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MissionBinding {
    pub mission_id: String,
    pub revision: u64,
    pub artifact_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StableNode {
    pub id: String,
    pub content: String,
    pub kind: Option<String>,
    pub depends_on: Vec<String>,
    pub priority: u8,
}

impl From<TaskGraphNodeSpec> for StableNode {
    fn from(spec: TaskGraphNodeSpec) -> Self {
        Self {
            id: spec.id,
            content: spec.content,
            kind: spec.kind,
            depends_on: spec.depends_on,
            priority: spec.priority,
        }
    }
}

impl StableNode {
    fn wire(&self) -> TaskGraphNodeSpec {
        TaskGraphNodeSpec {
            id: self.id.clone(),
            content: self.content.clone(),
            kind: self.kind.clone(),
            depends_on: self.depends_on.clone(),
            priority: self.priority,
        }
    }

    fn node_spec(&self) -> NodeSpec {
        NodeSpec {
            id: Some(self.id.clone()),
            content: self.content.clone(),
            kind: parse_kind(self.kind.as_deref()),
            depends_on: self.depends_on.clone(),
            priority: self.priority,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecompositionPacket {
    pub root_prompt: String,
    pub mission: MissionBinding,
    pub session_id: String,
    pub revision: u64,
    pub material_scope: String,
    pub mode: String,
    pub nodes: Vec<StableNode>,
    #[serde(default)]
    pub coverage: RequirementCoverageSummary,
    /// Digest of just the normalized node/edge manifest. This is the receipt's
    /// execution check, distinct from the complete packet hash.
    pub structural_digest: String,
    pub canonical_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AppliedReceipt {
    pub session_id: String,
    pub revision: u64,
    pub packet_hash: String,
    pub structural_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PrepareOutcome {
    Prepared,
    Replay,
    InactiveRevision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReconcileOutcome {
    Inserted,
    Replay,
}

#[derive(Debug)]
pub(crate) enum MaterializerError {
    EmptyRootPrompt,
    EmptyMaterialScope,
    InvalidRevision,
    EmptyGraph,
    InvalidMode,
    PacketConflict { revision: u64 },
    GraphMismatch,
    Storage(std::io::Error),
    Json(serde_json::Error),
    Graph(String),
    InvalidSpecScoutResult(String),
}

impl std::fmt::Display for MaterializerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRootPrompt => {
                formatter.write_str("decomposition packet requires a non-empty root prompt")
            }
            Self::EmptyMaterialScope => {
                formatter.write_str("decomposition packet requires a non-empty material scope")
            }
            Self::InvalidRevision => {
                formatter.write_str("decomposition packet revision must be positive")
            }
            Self::EmptyGraph => {
                formatter.write_str("decomposition packet requires at least one node")
            }
            Self::InvalidMode => {
                formatter.write_str("decomposition packet mode must be deep or light")
            }
            Self::PacketConflict { revision } => write!(
                formatter,
                "decomposition packet revision {revision} conflicts with the immutable prepared packet"
            ),
            Self::GraphMismatch => {
                formatter.write_str("prepared packet does not match the durable task graph")
            }
            Self::Storage(error) => {
                write!(formatter, "failed to persist decomposition packet: {error}")
            }
            Self::Json(error) => write!(
                formatter,
                "failed to encode or decode decomposition packet: {error}"
            ),
            Self::Graph(error) => write!(formatter, "task graph rejected: {error}"),
            Self::InvalidSpecScoutResult(error) => {
                write!(formatter, "invalid mandatory spec scout result: {error}")
            }
        }
    }
}

impl std::error::Error for MaterializerError {}

impl From<std::io::Error> for MaterializerError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<serde_json::Error> for MaterializerError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl DecompositionPacket {
    pub(crate) fn new(
        root_prompt: String,
        mission: MissionBinding,
        session_id: String,
        revision: u64,
        material_scope: String,
        mode: Option<String>,
        nodes: Vec<TaskGraphNodeSpec>,
    ) -> Result<Self, MaterializerError> {
        if root_prompt.trim().is_empty() {
            return Err(MaterializerError::EmptyRootPrompt);
        }
        if material_scope.trim().is_empty() {
            return Err(MaterializerError::EmptyMaterialScope);
        }
        if revision == 0 {
            return Err(MaterializerError::InvalidRevision);
        }
        if nodes.is_empty() {
            return Err(MaterializerError::EmptyGraph);
        }
        let mode = normalize_mode(mode.as_deref())?;
        let nodes = normalize_nodes(nodes.into_iter().map(StableNode::from).collect())?;
        let structural_digest = digest(&canonical_manifest(&nodes));
        let canonical_hash = digest(&canonical_packet(
            &root_prompt,
            &mission,
            &session_id,
            revision,
            &material_scope,
            &mode,
            &nodes,
            &structural_digest,
            &RequirementCoverageSummary::default(),
        ));
        Ok(Self {
            root_prompt,
            mission,
            session_id,
            revision,
            material_scope,
            mode,
            nodes,
            coverage: RequirementCoverageSummary::default(),
            structural_digest,
            canonical_hash,
        })
    }

    pub(crate) fn with_coverage(mut self, coverage: RequirementCoverageSummary) -> Self {
        self.coverage = coverage;
        self.canonical_hash = digest(&canonical_packet(
            &self.root_prompt,
            &self.mission,
            &self.session_id,
            self.revision,
            &self.material_scope,
            &self.mode,
            &self.nodes,
            &self.structural_digest,
            &self.coverage,
        ));
        self
    }

    pub(crate) fn requested_nodes(&self) -> Vec<TaskGraphNodeSpec> {
        self.nodes.iter().map(StableNode::wire).collect()
    }

    pub(crate) fn is_active(&self) -> bool {
        self.revision == 1
    }

    pub(crate) fn request_matches(&self, mode: Option<&str>, nodes: &[TaskGraphNodeSpec]) -> bool {
        let Ok(mode) = normalize_mode(mode) else {
            return false;
        };
        let Ok(nodes) = normalize_nodes(nodes.iter().cloned().map(StableNode::from).collect())
        else {
            return false;
        };
        self.mode == mode && self.nodes == nodes
    }

    pub(crate) fn matches_plan(&self, plan: &VersionedPlan) -> bool {
        let mut graph = to_task_graph(plan);
        let before = graph.clone();
        if dag::seed(
            &mut graph,
            self.nodes.iter().map(StableNode::node_spec).collect(),
        )
        .is_err()
        {
            return false;
        }
        graph == before
    }
}

pub(crate) fn reconcile_packet(
    packet: &DecompositionPacket,
    plan: &mut VersionedPlan,
) -> Result<ReconcileOutcome, MaterializerError> {
    let mut staged = plan.clone();
    let downgrades_deep = staged.mode.eq_ignore_ascii_case("deep")
        && !packet.mode.eq_ignore_ascii_case("deep")
        && !staged.items.is_empty();
    if downgrades_deep {
        return Err(MaterializerError::Graph(
            "cannot downgrade a non-empty deep plan to light".to_string(),
        ));
    }
    staged.mode = packet.mode.clone();
    let mut graph = to_task_graph(&staged);
    let before = graph.clone();
    dag::seed(
        &mut graph,
        packet.nodes.iter().map(StableNode::node_spec).collect(),
    )
    .map_err(|error| MaterializerError::Graph(error.to_string()))?;
    if graph == before {
        if !packet.matches_plan(&staged) {
            return Err(MaterializerError::GraphMismatch);
        }
        return Ok(ReconcileOutcome::Replay);
    }
    apply_task_graph(&mut staged, &graph);
    staged.version += 1;
    if !packet.matches_plan(&staged) {
        return Err(MaterializerError::GraphMismatch);
    }
    *plan = staged;
    Ok(ReconcileOutcome::Inserted)
}

#[path = "decomposition_spec_scout.rs"]
mod spec_scout;
#[path = "decomposition_materializer_store.rs"]
pub(crate) mod storage;
pub(crate) use spec_scout::{SpecScoutContradiction, SpecScoutResult, SpecScoutUnstatedDefault};
pub(crate) use storage::{
    PacketStore, plan_graph_node_ids, prepare_root_materialization, seed_node_id_collision,
    seed_retry_scope,
};
use storage::{canonical_manifest, canonical_packet, digest, normalize_mode, normalize_nodes};

#[cfg(test)]
#[path = "decomposition_materializer_tests.rs"]
mod tests;
