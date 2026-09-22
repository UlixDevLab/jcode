use crate::canonical::CanonicalArtifactHash;
use crate::revision::{MissionId, MissionRevision, MissionRevisionDraft, MissionRevisionRef};
use crate::state::MissionState;
use crate::validation::MissionArtifactError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(crate) const ARTIFACT_ENTITY: &str = "mission-artifact";
pub(crate) const SCOPE_ENTITY: &str = "mission-scope";
pub const MISSION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum MissionStatus {
    Declared = 1,
    Approved = 2,
    InProgress = 3,
    Completed = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum WaveStatus {
    Pending = 1,
    Active = 2,
    Completed = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeEntry {
    pub relative_path: String,
    pub entity_id: String,
    pub write_set: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionWave {
    pub id: String,
    pub scope: Vec<ScopeEntry>,
    pub status: WaveStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub acceptance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub requirement_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wave_id: String,
    pub reference: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionApproval {
    pub mission_id: MissionId,
    pub revision: u64,
    pub plan_hash: CanonicalArtifactHash,
    pub approved_wave_ids: Vec<String>,
    pub scope_digest: CanonicalArtifactHash,
    pub approver: String,
    pub approved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissionArtifact {
    pub(crate) schema_version: u32,
    pub(crate) mission_id: MissionId,
    pub(crate) project_root_id: String,
    pub(crate) active_revision: MissionRevisionRef,
    pub(crate) revisions: Vec<MissionRevision>,
    pub(crate) approval: Option<MissionApproval>,
    pub(crate) waves: Vec<MissionWave>,
    pub(crate) evidence: Vec<EvidenceRecord>,
    pub(crate) status: MissionStatus,
    pub(crate) canonical_hash: CanonicalArtifactHash,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionArtifactWire {
    schema_version: u32,
    mission_id: MissionId,
    project_root_id: String,
    active_revision: MissionRevisionRef,
    revisions: Vec<MissionRevision>,
    approval: Option<MissionApproval>,
    waves: Vec<MissionWave>,
    evidence: Vec<EvidenceRecord>,
    status: MissionStatus,
    canonical_hash: CanonicalArtifactHash,
}

impl<'de> Deserialize<'de> for MissionArtifact {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MissionArtifactWire::deserialize(deserializer)?;
        let artifact = Self {
            schema_version: wire.schema_version,
            mission_id: wire.mission_id,
            project_root_id: wire.project_root_id,
            active_revision: wire.active_revision,
            revisions: wire.revisions,
            approval: wire.approval,
            waves: wire.waves,
            evidence: wire.evidence,
            status: wire.status,
            canonical_hash: wire.canonical_hash,
        };
        artifact.validate().map_err(serde::de::Error::custom)?;
        Ok(artifact)
    }
}

impl MissionArtifact {
    pub fn from_state(
        project_root_id: impl Into<String>,
        state: &MissionState,
        waves: Vec<MissionWave>,
    ) -> Result<Self, MissionArtifactError> {
        state
            .validate()
            .map_err(MissionArtifactError::InvalidState)?;
        let mission_id = state
            .mission_id()
            .ok_or(MissionArtifactError::MissingRevision)?;
        let active = state
            .active_revision()
            .ok_or(MissionArtifactError::MissingRevision)?;
        let mut artifact = Self {
            schema_version: MISSION_SCHEMA_VERSION,
            mission_id: mission_id.clone(),
            project_root_id: project_root_id.into(),
            active_revision: active.reference(),
            revisions: state.revisions().to_vec(),
            approval: None,
            waves,
            evidence: Vec::new(),
            status: MissionStatus::Declared,
            canonical_hash: CanonicalArtifactHash::from_canonical_bytes(ARTIFACT_ENTITY, &[]),
        };
        artifact.seal()?;
        Ok(artifact)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn mission_id(&self) -> &MissionId {
        &self.mission_id
    }
    pub fn project_root_id(&self) -> &str {
        &self.project_root_id
    }
    pub fn active_revision(&self) -> &MissionRevisionRef {
        &self.active_revision
    }
    pub fn revisions(&self) -> &[MissionRevision] {
        &self.revisions
    }
    pub fn approval(&self) -> Option<&MissionApproval> {
        self.approval.as_ref()
    }
    pub fn waves(&self) -> &[MissionWave] {
        &self.waves
    }
    pub fn evidence(&self) -> &[EvidenceRecord] {
        &self.evidence
    }
    pub const fn status(&self) -> MissionStatus {
        self.status
    }
    pub fn state_hash(&self) -> &CanonicalArtifactHash {
        &self.canonical_hash
    }

    pub fn approve(
        &mut self,
        approver: impl Into<String>,
        approved_wave_ids: Vec<String>,
        approved_at: DateTime<Utc>,
    ) -> Result<(), MissionArtifactError> {
        if self.approval.is_some() {
            return Err(MissionArtifactError::AlreadyApproved);
        }
        let approver = approver.into();
        if approver.is_empty() {
            return Err(MissionArtifactError::EmptyApprover);
        }
        let scope_digest = self.scope_digest_for(&approved_wave_ids)?;
        let plan_hash = self.active()?.canonical_hash().clone();
        self.approval = Some(MissionApproval {
            mission_id: self.mission_id.clone(),
            revision: self.active_revision.revision,
            plan_hash,
            approved_wave_ids,
            scope_digest,
            approver,
            approved_at,
        });
        self.status = MissionStatus::Approved;
        self.seal()
    }

    /// Append an unapproved revision without changing the existing wave records.
    ///
    /// Execution and evidence are bound to the currently active revision. A
    /// changed revision would otherwise strand active waves or make recorded
    /// requirement evidence invalid, so those lifecycle states fail closed.
    pub fn revise(&mut self, draft: MissionRevisionDraft) -> Result<(), MissionArtifactError> {
        if !matches!(
            self.status,
            MissionStatus::Declared | MissionStatus::Approved
        ) {
            return Err(MissionArtifactError::RevisionAfterExecutionStarted);
        }
        if !self.evidence.is_empty() {
            return Err(MissionArtifactError::RevisionWithEvidence);
        }

        let previous_revision = self.active_revision.revision;
        let mut state = MissionState::default();
        for revision in self.revisions.iter().cloned() {
            state
                .append_persisted(revision)
                .map_err(MissionArtifactError::InvalidState)?;
        }
        let active = state
            .submit(draft)
            .map_err(MissionArtifactError::InvalidState)?;
        if active.revision() == previous_revision {
            return Err(MissionArtifactError::RevisionNotMaterial);
        }

        self.revisions = state.revisions().to_vec();
        self.active_revision = active.reference();
        self.approval = None;
        self.status = MissionStatus::Declared;
        self.seal()
    }

    pub fn start_wave(&mut self, wave_id: &str) -> Result<(), MissionArtifactError> {
        self.require_approved_wave(wave_id)?;
        let wave = self.wave_mut(wave_id)?;
        if wave.status != WaveStatus::Pending {
            return Err(MissionArtifactError::InvalidWaveTransition);
        }
        wave.status = WaveStatus::Active;
        self.status = MissionStatus::InProgress;
        self.seal()
    }

    pub fn complete_wave(&mut self, wave_id: &str) -> Result<(), MissionArtifactError> {
        self.require_approved_wave(wave_id)?;
        let wave = self.wave_mut(wave_id)?;
        if wave.status != WaveStatus::Active {
            return Err(MissionArtifactError::InvalidWaveTransition);
        }
        wave.status = WaveStatus::Completed;
        if self
            .waves
            .iter()
            .all(|item| item.status == WaveStatus::Completed)
        {
            self.status = MissionStatus::Completed;
        }
        self.seal()
    }

    pub fn record_evidence(
        &mut self,
        evidence: EvidenceRecord,
    ) -> Result<(), MissionArtifactError> {
        if !is_acceptance_id(&evidence.acceptance_id) {
            return Err(MissionArtifactError::InvalidAcceptanceId);
        }
        let legacy_evidence = evidence.requirement_id.is_empty() && evidence.wave_id.is_empty();
        if evidence.reference.is_empty()
            || self
                .evidence
                .iter()
                .any(|item| item.acceptance_id == evidence.acceptance_id)
            || (!legacy_evidence
                && (!self
                    .active_requirement_ids()
                    .iter()
                    .any(|id| id == &evidence.requirement_id)
                    || !self.waves.iter().any(|wave| wave.id == evidence.wave_id)))
            || (legacy_evidence && !self.active_requirement_ids().is_empty())
        {
            return Err(MissionArtifactError::InvalidEvidence);
        }
        self.evidence.push(evidence);
        self.seal()
    }

    pub fn scope_entries(&self) -> impl Iterator<Item = &ScopeEntry> {
        self.waves.iter().flat_map(|wave| wave.scope.iter())
    }

    fn active_requirement_ids(&self) -> Vec<String> {
        let Ok(revision) = self.active() else {
            return Vec::new();
        };
        revision
            .constraints()
            .iter()
            .map(|constraint| constraint.id.clone())
            .chain(
                revision
                    .success_criteria()
                    .iter()
                    .map(|criterion| criterion.id.clone()),
            )
            .filter(|id| !id.is_empty())
            .collect()
    }

    fn wave_mut(&mut self, wave_id: &str) -> Result<&mut MissionWave, MissionArtifactError> {
        self.waves
            .iter_mut()
            .find(|item| item.id == wave_id)
            .ok_or(MissionArtifactError::UnknownWave)
    }

    fn require_approved_wave(&self, wave_id: &str) -> Result<(), MissionArtifactError> {
        let approval = self
            .approval
            .as_ref()
            .ok_or(MissionArtifactError::MissingApproval)?;
        if !approval.approved_wave_ids.iter().any(|id| id == wave_id) {
            return Err(MissionArtifactError::WaveNotApproved);
        }
        Ok(())
    }
}

fn is_acceptance_id(value: &str) -> bool {
    matches!(value, "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I")
}
