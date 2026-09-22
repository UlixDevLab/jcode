use crate::{
    CanonicalArtifactHash, EvidenceRecord, MissionArtifact, MissionArtifactError, MissionId,
    MissionRevisionDraft,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionCas {
    pub mission_id: MissionId,
    pub expected_revision: u64,
    pub expected_state_hash: CanonicalArtifactHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionTransition {
    Revise { draft: MissionRevisionDraft },
    StartWave { wave_id: String },
    CompleteWave { wave_id: String },
    RecordEvidence { evidence: EvidenceRecord },
}

impl MissionTransition {
    pub fn apply(self, artifact: &mut MissionArtifact) -> Result<(), MissionTransitionError> {
        match self {
            Self::Revise { draft } => artifact.revise(draft),
            Self::StartWave { wave_id } => artifact.start_wave(&wave_id),
            Self::CompleteWave { wave_id } => artifact.complete_wave(&wave_id),
            Self::RecordEvidence { evidence } => artifact.record_evidence(evidence),
        }
        .map_err(MissionTransitionError::InvalidTransition)
    }
}

impl MissionCas {
    pub fn verify(&self, artifact: &MissionArtifact) -> Result<(), MissionTransitionError> {
        if &self.mission_id != artifact.mission_id() {
            return Err(MissionTransitionError::MissionMismatch);
        }
        if self.expected_revision != artifact.active_revision().revision {
            return Err(MissionTransitionError::StaleRevision {
                expected: self.expected_revision,
                actual: artifact.active_revision().revision,
            });
        }
        if &self.expected_state_hash != artifact.state_hash() {
            return Err(MissionTransitionError::StaleStateHash);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionTransitionError {
    MissionMismatch,
    StaleRevision { expected: u64, actual: u64 },
    StaleStateHash,
    InvalidTransition(MissionArtifactError),
}

impl fmt::Display for MissionTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissionMismatch => formatter.write_str("mission CAS mission ID mismatch"),
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "mission CAS revision mismatch: expected {expected}, got {actual}"
            ),
            Self::StaleStateHash => formatter.write_str("mission CAS state hash mismatch"),
            Self::InvalidTransition(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MissionTransitionError {}
