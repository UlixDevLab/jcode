//! Narrow server-owned authority for Todo-cycle structural review.
//!
//! The caller's identity is captured in an opaque capability while the server
//! owns the live membership and coordinator state. Tool arguments therefore
//! cannot assert an actor or a reviewer role.

use super::SwarmMember;
use anyhow::{Result, bail};
use async_trait::async_trait;
use jcode_tool_core::{
    StructuralReviewAuthority, StructuralReviewCandidate, StructuralReviewDisposition,
    StructuralReviewOpen, StructuralReviewReceipt, StructuralReviewSubmission,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

const STORE_DIR: &str = "structural-reviews";

#[derive(Clone)]
pub(super) struct StructuralReviewRuntime {
    members: Arc<RwLock<HashMap<String, SwarmMember>>>,
    coordinators: Arc<RwLock<HashMap<String, String>>>,
    state: Arc<Mutex<ReviewState>>,
    store_root: Option<PathBuf>,
}

#[derive(Default)]
struct ReviewState {
    next_generation: u64,
    records: BTreeMap<String, StoredReview>,
    active_designations: BTreeMap<ReviewKey, ActiveDesignation>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReviewKey {
    owner_session_id: String,
    cycle_id: u64,
}

#[derive(Clone, Debug)]
struct ActiveDesignation {
    request_id: String,
    reviewer_session_id: String,
    generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredReview {
    request_id: String,
    owner_session_id: String,
    reviewer_session_id: String,
    candidate: StoredCandidate,
    designation_generation: u64,
    disposition: Option<StoredDisposition>,
    refactor_obligations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredCandidate {
    project_root: String,
    cycle_id: u64,
    baseline: String,
    candidate_paths: Vec<String>,
    candidate_digest: String,
    policy_version: String,
    source_changed: bool,
    risk_signals: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredDisposition {
    Cohesive,
    RefactorRequired,
}

impl From<StructuralReviewCandidate> for StoredCandidate {
    fn from(value: StructuralReviewCandidate) -> Self {
        Self {
            project_root: value.project_root,
            cycle_id: value.cycle_id,
            baseline: value.baseline,
            candidate_paths: value.candidate_paths,
            candidate_digest: value.candidate_digest,
            policy_version: value.policy_version,
            source_changed: value.source_changed,
            risk_signals: value.risk_signals,
        }
    }
}

impl From<StoredCandidate> for StructuralReviewCandidate {
    fn from(value: StoredCandidate) -> Self {
        Self {
            project_root: value.project_root,
            cycle_id: value.cycle_id,
            baseline: value.baseline,
            candidate_paths: value.candidate_paths,
            candidate_digest: value.candidate_digest,
            policy_version: value.policy_version,
            source_changed: value.source_changed,
            risk_signals: value.risk_signals,
        }
    }
}

impl From<StructuralReviewDisposition> for StoredDisposition {
    fn from(value: StructuralReviewDisposition) -> Self {
        match value {
            StructuralReviewDisposition::Cohesive => Self::Cohesive,
            StructuralReviewDisposition::RefactorRequired => Self::RefactorRequired,
        }
    }
}

impl From<StoredDisposition> for StructuralReviewDisposition {
    fn from(value: StoredDisposition) -> Self {
        match value {
            StoredDisposition::Cohesive => Self::Cohesive,
            StoredDisposition::RefactorRequired => Self::RefactorRequired,
        }
    }
}

impl StructuralReviewRuntime {
    pub(super) fn new(
        members: Arc<RwLock<HashMap<String, SwarmMember>>>,
        coordinators: Arc<RwLock<HashMap<String, String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            members,
            coordinators,
            state: Arc::new(Mutex::new(ReviewState::default())),
            store_root: None,
        })
    }

    pub(super) fn capability(
        self: &Arc<Self>,
        actor_session_id: String,
    ) -> Arc<dyn StructuralReviewAuthority> {
        Arc::new(ActorBoundStructuralReviewAuthority {
            actor_session_id,
            runtime: Arc::clone(self),
        })
    }

    async fn member_in_same_swarm(
        &self,
        actor_session_id: &str,
        other_session_id: &str,
    ) -> Result<(String, SwarmMember)> {
        let members = self.members.read().await;
        let actor = members.get(actor_session_id).ok_or_else(|| {
            anyhow::anyhow!("structural review actor is not a live server member")
        })?;
        if matches!(
            actor.status.as_str(),
            "completed" | "failed" | "stopped" | "crashed" | "cancelled"
        ) {
            bail!("structural review actor is not live");
        }
        let swarm_id = actor
            .swarm_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("structural review actor is not in a swarm"))?;
        let other = members.get(other_session_id).cloned().ok_or_else(|| {
            anyhow::anyhow!("structural review target is not a live server member")
        })?;
        if other.swarm_id.as_deref() != Some(swarm_id.as_str()) {
            bail!("structural review target is outside the coordinator swarm");
        }
        if matches!(
            other.status.as_str(),
            "completed" | "failed" | "stopped" | "crashed" | "cancelled"
        ) {
            bail!("structural review target is not live");
        }
        Ok((swarm_id, other))
    }

    async fn actor_is_current_coordinator(&self, actor_session_id: &str) -> Result<()> {
        let (swarm_id, _) = self
            .member_in_same_swarm(actor_session_id, actor_session_id)
            .await?;
        let coordinators = self.coordinators.read().await;
        if coordinators.get(&swarm_id).map(String::as_str) != Some(actor_session_id) {
            bail!("structural review request requires the current server coordinator");
        }
        Ok(())
    }

    async fn actor_is_live_designated_reviewer(
        &self,
        actor_session_id: &str,
        record: &StoredReview,
    ) -> Result<()> {
        if record.reviewer_session_id != actor_session_id {
            bail!("structural review verdict must come from the designated reviewer");
        }
        if record.owner_session_id == actor_session_id {
            bail!("structural review owner cannot approve its own implementation");
        }
        let (swarm_id, member) = self
            .member_in_same_swarm(actor_session_id, &record.owner_session_id)
            .await?;
        if member.session_id != record.owner_session_id {
            bail!("structural review owner identity changed");
        }
        let coordinators = self.coordinators.read().await;
        if coordinators.get(&swarm_id).map(String::as_str) == Some(actor_session_id) {
            bail!("structural review coordinator cannot approve its own request");
        }
        Ok(())
    }

    fn store_root(&self) -> Result<PathBuf> {
        Ok(self
            .store_root
            .clone()
            .map(|root| root.join(STORE_DIR))
            .unwrap_or(crate::storage::jcode_dir()?.join(STORE_DIR)))
    }

    fn record_path(&self, request_id: &str) -> Result<PathBuf> {
        Ok(self.store_root()?.join(format!("{request_id}.json")))
    }

    fn persist(&self, record: &StoredReview) -> Result<()> {
        let root = self.store_root()?;
        std::fs::create_dir_all(&root)?;
        crate::storage::write_json_fast(&self.record_path(&record.request_id)?, record)
    }

    fn load_persisted(&self, request_id: &str) -> Result<StoredReview> {
        let path = self.record_path(request_id)?;
        crate::storage::read_json(&path).map_err(|error| {
            anyhow::anyhow!(
                "structural review receipt is missing or corrupt at {}: {error}",
                path.display()
            )
        })
    }

    fn persist_ledger_entry(&self, record: &StoredReview) -> Result<()> {
        // Deliberately audit/planning-only. Validation never reads this ledger.
        if record.disposition != Some(StoredDisposition::RefactorRequired) {
            return Ok(());
        }
        let root = self
            .store_root
            .clone()
            .unwrap_or(crate::storage::jcode_dir()?);
        std::fs::create_dir_all(&root)?;
        let path = root.join("structural-quality-ledger.json");
        let mut ledger: BTreeMap<String, serde_json::Value> =
            crate::storage::read_json(&path).unwrap_or_default();
        let cumulative_additions = record
            .candidate
            .risk_signals
            .iter()
            .find_map(|signal| signal.strip_prefix("cumulative-source-additions:"))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        for component in &record.candidate.candidate_paths {
            ledger.insert(
                format!("{}::{component}", record.candidate.project_root),
                serde_json::json!({
                    "project_root": record.candidate.project_root,
                    "path": component,
                    "fingerprint": record.candidate.candidate_digest,
                    "high_water": record.candidate.risk_signals.iter().find(|signal| signal.contains(component)).cloned(),
                    "cumulative_additions": cumulative_additions,
                    "disposition": "refactor_required",
                    "last_receipt": record.request_id,
                    "next_touch_action": record.refactor_obligations,
                }),
            );
        }
        crate::storage::write_json_fast(&path, &ledger)
    }

    fn receipt(record: &StoredReview) -> Result<StructuralReviewReceipt> {
        let disposition = record
            .disposition
            .ok_or_else(|| anyhow::anyhow!("structural review is still pending"))?;
        Ok(StructuralReviewReceipt {
            request_id: record.request_id.clone(),
            owner_session_id: record.owner_session_id.clone(),
            reviewer_session_id: record.reviewer_session_id.clone(),
            candidate: record.candidate.clone().into(),
            disposition: disposition.into(),
            refactor_obligations: record.refactor_obligations.clone(),
        })
    }
}

struct ActorBoundStructuralReviewAuthority {
    actor_session_id: String,
    runtime: Arc<StructuralReviewRuntime>,
}

#[async_trait]
impl StructuralReviewAuthority for ActorBoundStructuralReviewAuthority {
    async fn open_structural_review(
        &self,
        request: StructuralReviewOpen,
    ) -> Result<StructuralReviewReceipt> {
        if !request.candidate.source_changed {
            bail!("structural review is only required for source-changing cycles");
        }
        self.runtime
            .actor_is_current_coordinator(&self.actor_session_id)
            .await?;
        if request.owner_session_id == request.reviewer_session_id {
            bail!("structural review owner and reviewer must be distinct");
        }
        self.runtime
            .member_in_same_swarm(&self.actor_session_id, &request.owner_session_id)
            .await?;
        self.runtime
            .member_in_same_swarm(&self.actor_session_id, &request.reviewer_session_id)
            .await?;

        let (swarm_id, _) = self
            .runtime
            .member_in_same_swarm(&self.actor_session_id, &request.owner_session_id)
            .await?;
        if self
            .runtime
            .coordinators
            .read()
            .await
            .get(&swarm_id)
            .map(String::as_str)
            == Some(request.reviewer_session_id.as_str())
        {
            bail!("structural review reviewer must be independent from the coordinator");
        }

        let mut state = self
            .runtime
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.next_generation += 1;
        let generation = state.next_generation;
        let key = ReviewKey {
            owner_session_id: request.owner_session_id.clone(),
            cycle_id: request.candidate.cycle_id,
        };
        if let Some(prior_designation) = state.active_designations.get(&key) {
            if let Some(prior) = state.records.get(&prior_designation.request_id) {
                if prior.disposition == Some(StoredDisposition::RefactorRequired)
                    && prior.candidate.candidate_digest == request.candidate.candidate_digest
                {
                    bail!(
                        "recorded refactor obligations require a changed candidate before fresh review"
                    );
                }
            }
        }
        let request_id = format!("structural-review-{}", uuid::Uuid::new_v4());
        let record = StoredReview {
            request_id: request_id.clone(),
            owner_session_id: request.owner_session_id,
            reviewer_session_id: request.reviewer_session_id.clone(),
            candidate: request.candidate.into(),
            designation_generation: generation,
            disposition: None,
            refactor_obligations: Vec::new(),
        };
        // Write pending state before publishing its designation. If this fails,
        // there is no live record that can later become an in-memory approval.
        self.runtime.persist(&record)?;
        // Replacing a designation invalidates the prior request before either
        // record can be submitted. The generation is checked again below.
        state.active_designations.insert(
            key,
            ActiveDesignation {
                request_id: request_id.clone(),
                reviewer_session_id: request.reviewer_session_id,
                generation,
            },
        );
        state.records.insert(request_id, record.clone());
        Ok(StructuralReviewReceipt {
            request_id: record.request_id,
            owner_session_id: record.owner_session_id,
            reviewer_session_id: record.reviewer_session_id,
            candidate: record.candidate.into(),
            // An open request is not an approval. The tool uses this only to
            // report its durable server-authored request id.
            disposition: StructuralReviewDisposition::RefactorRequired,
            refactor_obligations: Vec::new(),
        })
    }

    async fn submit_structural_review(
        &self,
        submission: StructuralReviewSubmission,
    ) -> Result<StructuralReviewReceipt> {
        let record = {
            let state = self
                .runtime
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let record = state
                .records
                .get(&submission.request_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("structural review request is unknown"))?;
            let key = ReviewKey {
                owner_session_id: record.owner_session_id.clone(),
                cycle_id: record.candidate.cycle_id,
            };
            let designation = state.active_designations.get(&key).ok_or_else(|| {
                anyhow::anyhow!("structural review designation is no longer active")
            })?;
            if designation.request_id != record.request_id
                || designation.reviewer_session_id != record.reviewer_session_id
                || designation.generation != record.designation_generation
            {
                bail!("structural review designation was replaced or became stale");
            }
            if record.disposition.is_some() {
                bail!("structural review request already has a verdict");
            }
            record
        };
        self.runtime
            .actor_is_live_designated_reviewer(&self.actor_session_id, &record)
            .await?;

        let updated = {
            let mut state = self
                .runtime
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let key = ReviewKey {
                owner_session_id: record.owner_session_id.clone(),
                cycle_id: record.candidate.cycle_id,
            };
            let designation = state.active_designations.get(&key).ok_or_else(|| {
                anyhow::anyhow!("structural review designation is no longer active")
            })?;
            if designation.request_id != record.request_id
                || designation.reviewer_session_id != record.reviewer_session_id
                || designation.generation != record.designation_generation
            {
                bail!("structural review designation was replaced or became stale");
            }
            let stored = state
                .records
                .get(&submission.request_id)
                .ok_or_else(|| anyhow::anyhow!("structural review request disappeared"))?;
            if stored.disposition.is_some() {
                bail!("structural review request already has a verdict");
            }
            let mut updated = stored.clone();
            updated.disposition = Some(submission.disposition.into());
            updated.refactor_obligations = submission.refactor_obligations;
            // Keep the lock through the synchronous atomic write so a failed
            // write leaves the only authoritative in-memory record pending.
            self.runtime.persist(&updated)?;
            self.runtime.persist_ledger_entry(&updated)?;
            state
                .records
                .insert(submission.request_id.clone(), updated.clone());
            updated
        };
        StructuralReviewRuntime::receipt(&updated)
    }

    async fn validate_structural_review(
        &self,
        candidate: StructuralReviewCandidate,
    ) -> Result<StructuralReviewReceipt> {
        if !candidate.source_changed {
            bail!("structural review validation is not applicable to read-only cycles");
        }
        let record = {
            let state = self
                .runtime
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let key = ReviewKey {
                owner_session_id: self.actor_session_id.clone(),
                cycle_id: candidate.cycle_id,
            };
            let designation = state.active_designations.get(&key).ok_or_else(|| {
                anyhow::anyhow!("missing mandatory independent structural review")
            })?;
            let record = state
                .records
                .get(&designation.request_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("structural review receipt is missing"))?;
            if record.candidate != StoredCandidate::from(candidate) {
                bail!("structural review candidate changed after review");
            }
            record
        };
        self.runtime
            .actor_is_live_designated_reviewer(&record.reviewer_session_id, &record)
            .await?;
        let persisted = self.runtime.load_persisted(&record.request_id)?;
        if persisted != record {
            bail!("structural review durable receipt does not match active designation");
        }
        let receipt = StructuralReviewRuntime::receipt(&record)?;
        if receipt.disposition != StructuralReviewDisposition::Cohesive {
            bail!("structural review requires the recorded refactor obligations before closure");
        }
        Ok(receipt)
    }
}

#[cfg(test)]
#[path = "structural_review_tests.rs"]
mod tests;
