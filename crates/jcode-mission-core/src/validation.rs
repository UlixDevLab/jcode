use crate::artifact::{ARTIFACT_ENTITY, SCOPE_ENTITY};
use crate::canonical::{CanonicalArtifactHash, CanonicalWriter};
use crate::{
    MISSION_SCHEMA_VERSION, MissionArtifact, MissionRevision, MissionStateError, MissionStatus,
    ScopeEntry,
};
use std::collections::BTreeSet;
use std::fmt;

impl MissionArtifact {
    pub fn validate(&self) -> Result<(), MissionArtifactError> {
        if self.schema_version != MISSION_SCHEMA_VERSION {
            return Err(MissionArtifactError::UnsupportedSchema);
        }
        if self.project_root_id.is_empty() {
            return Err(MissionArtifactError::EmptyProjectRoot);
        }
        if self.revisions.is_empty() || self.mission_id.as_str().is_empty() {
            return Err(MissionArtifactError::MissingRevision);
        }
        for (index, revision) in self.revisions.iter().enumerate() {
            revision
                .verify_canonical_hash()
                .map_err(MissionArtifactError::InvalidRevision)?;
            let expected = (index as u64) + 1;
            if revision.mission_id() != &self.mission_id
                || revision.revision() != expected
                || revision.parent_revision() != expected.checked_sub(1).filter(|item| *item > 0)
            {
                return Err(MissionArtifactError::InvalidRevisionSequence);
            }
        }
        if self.active_revision != self.active()?.reference() {
            return Err(MissionArtifactError::ActiveRevisionMismatch);
        }
        self.validate_waves()?;
        self.validate_approval()?;
        self.validate_evidence()?;
        if self.canonical_hash != self.computed_state_hash() {
            return Err(MissionArtifactError::CanonicalHashMismatch);
        }
        Ok(())
    }

    pub(crate) fn seal(&mut self) -> Result<(), MissionArtifactError> {
        self.validate_without_hash()?;
        self.canonical_hash = self.computed_state_hash();
        Ok(())
    }

    pub(crate) fn active(&self) -> Result<&MissionRevision, MissionArtifactError> {
        self.revisions
            .last()
            .ok_or(MissionArtifactError::MissingRevision)
    }

    pub(crate) fn scope_digest_for(
        &self,
        ids: &[String],
    ) -> Result<CanonicalArtifactHash, MissionArtifactError> {
        let mut unique = BTreeSet::new();
        let mut writer = CanonicalWriter::default();
        writer.u64(ids.len() as u64);
        for id in ids {
            if !unique.insert(id) {
                return Err(MissionArtifactError::InvalidApproval);
            }
            let wave = self
                .waves
                .iter()
                .find(|wave| wave.id == *id)
                .ok_or(MissionArtifactError::UnknownWave)?;
            writer.text(id);
            writer.u64(wave.scope.len() as u64);
            for entry in &wave.scope {
                writer.text(&entry.relative_path);
                writer.text(&entry.entity_id);
                writer.u8(entry.write_set as u8);
            }
        }
        Ok(CanonicalArtifactHash::from_canonical_bytes(
            SCOPE_ENTITY,
            &writer.into_bytes(),
        ))
    }

    fn validate_without_hash(&self) -> Result<(), MissionArtifactError> {
        let mut copy = self.clone();
        copy.canonical_hash = copy.computed_state_hash();
        copy.validate()
    }

    fn validate_waves(&self) -> Result<(), MissionArtifactError> {
        let mut wave_ids = BTreeSet::new();
        let mut scopes = BTreeSet::new();
        for wave in &self.waves {
            if wave.id.is_empty() || !wave_ids.insert(&wave.id) {
                return Err(MissionArtifactError::InvalidWave);
            }
            for entry in &wave.scope {
                validate_scope(entry)?;
                if !scopes.insert(&entry.relative_path) {
                    return Err(MissionArtifactError::DuplicateScope);
                }
            }
        }
        Ok(())
    }

    fn validate_approval(&self) -> Result<(), MissionArtifactError> {
        match (&self.approval, self.status) {
            (None, MissionStatus::Declared) => Ok(()),
            (None, _) => Err(MissionArtifactError::MissingApproval),
            (Some(_), MissionStatus::Declared) => Err(MissionArtifactError::InvalidApproval),
            (Some(approval), _) => {
                if approval.mission_id != self.mission_id
                    || approval.revision != self.active_revision.revision
                    || approval.plan_hash != *self.active()?.canonical_hash()
                    || approval.approver.is_empty()
                    || approval.approved_wave_ids.is_empty()
                    || approval.scope_digest
                        != self.scope_digest_for(&approval.approved_wave_ids)?
                {
                    Err(MissionArtifactError::InvalidApproval)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn validate_evidence(&self) -> Result<(), MissionArtifactError> {
        let mut ids = BTreeSet::new();
        let active_requirement_ids: BTreeSet<&str> = self
            .active()?
            .constraints()
            .iter()
            .map(|constraint| constraint.id.as_str())
            .chain(
                self.active()?
                    .success_criteria()
                    .iter()
                    .map(|criterion| criterion.id.as_str()),
            )
            .filter(|id| !id.is_empty())
            .collect();
        for item in &self.evidence {
            let legacy_evidence = item.requirement_id.is_empty() && item.wave_id.is_empty();
            if !matches!(
                item.acceptance_id.as_str(),
                "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H" | "I"
            ) || item.reference.is_empty()
                || !ids.insert(&item.acceptance_id)
                || (!legacy_evidence
                    && (!active_requirement_ids.contains(item.requirement_id.as_str())
                        || !self.waves.iter().any(|wave| wave.id == item.wave_id)))
                || (legacy_evidence && !active_requirement_ids.is_empty())
            {
                return Err(MissionArtifactError::InvalidEvidence);
            }
        }
        Ok(())
    }

    fn computed_state_hash(&self) -> CanonicalArtifactHash {
        let mut writer = CanonicalWriter::default();
        writer.u32(self.schema_version);
        writer.text(self.mission_id.as_str());
        writer.text(&self.project_root_id);
        writer.u64(self.active_revision.revision);
        writer.u64(self.revisions.len() as u64);
        for revision in &self.revisions {
            writer.text(revision.canonical_hash().as_str());
        }
        match &self.approval {
            None => writer.u8(0),
            Some(value) => {
                writer.u8(1);
                writer.text(value.mission_id.as_str());
                writer.u64(value.revision);
                writer.text(value.plan_hash.as_str());
                writer.u64(value.approved_wave_ids.len() as u64);
                for id in &value.approved_wave_ids {
                    writer.text(id);
                }
                writer.text(value.scope_digest.as_str());
                writer.text(&value.approver);
                writer.i64(value.approved_at.timestamp());
                writer.u32(value.approved_at.timestamp_subsec_nanos());
            }
        }
        writer.u64(self.waves.len() as u64);
        for wave in &self.waves {
            writer.text(&wave.id);
            writer.u8(wave.status as u8);
            writer.u64(wave.scope.len() as u64);
            for entry in &wave.scope {
                writer.text(&entry.relative_path);
                writer.text(&entry.entity_id);
                writer.u8(entry.write_set as u8);
            }
        }
        writer.u64(self.evidence.len() as u64);
        for item in &self.evidence {
            writer.text(&item.acceptance_id);
            if !item.requirement_id.is_empty() || !item.wave_id.is_empty() {
                writer.text(&item.requirement_id);
                writer.text(&item.wave_id);
            }
            writer.text(&item.reference);
            writer.i64(item.recorded_at.timestamp());
            writer.u32(item.recorded_at.timestamp_subsec_nanos());
        }
        writer.u8(self.status as u8);
        CanonicalArtifactHash::from_canonical_bytes(ARTIFACT_ENTITY, &writer.into_bytes())
    }
}

fn validate_scope(entry: &ScopeEntry) -> Result<(), MissionArtifactError> {
    if entry.relative_path.is_empty()
        || entry.entity_id.is_empty()
        || entry.relative_path.contains('\\')
        || entry
            .relative_path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        Err(MissionArtifactError::InvalidScope)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionArtifactError {
    UnsupportedSchema,
    EmptyProjectRoot,
    MissingRevision,
    InvalidRevision(crate::MissionRevisionError),
    InvalidRevisionSequence,
    ActiveRevisionMismatch,
    CanonicalHashMismatch,
    InvalidWave,
    DuplicateScope,
    UnknownWave,
    WaveNotApproved,
    InvalidWaveTransition,
    AlreadyApproved,
    EmptyApprover,
    MissingApproval,
    InvalidApproval,
    InvalidAcceptanceId,
    InvalidEvidence,
    InvalidScope,
    InvalidState(MissionStateError),
    RevisionAfterExecutionStarted,
    RevisionWithEvidence,
    RevisionNotMaterial,
}

impl fmt::Display for MissionArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedSchema => "unsupported mission artifact schema",
            Self::EmptyProjectRoot => "mission artifact project root is empty",
            Self::MissingRevision => "mission artifact requires an active revision",
            Self::InvalidRevision(_) => "mission artifact contains an invalid revision",
            Self::InvalidRevisionSequence => "mission artifact revision sequence is invalid",
            Self::ActiveRevisionMismatch => "mission artifact active revision is invalid",
            Self::CanonicalHashMismatch => "mission artifact canonical hash mismatch",
            Self::InvalidWave => "mission wave is invalid",
            Self::DuplicateScope => "mission scope is duplicated",
            Self::UnknownWave => "mission wave is unknown",
            Self::WaveNotApproved => "mission wave is not approved",
            Self::InvalidWaveTransition => "mission wave transition is invalid",
            Self::AlreadyApproved => "mission artifact is already approved",
            Self::EmptyApprover => "mission approver is empty",
            Self::MissingApproval => "mission artifact approval is required",
            Self::InvalidApproval => "mission artifact approval is invalid",
            Self::InvalidAcceptanceId => "mission acceptance ID is invalid",
            Self::InvalidEvidence => "mission evidence is invalid",
            Self::InvalidScope => "mission scope is not normalized",
            Self::InvalidState(_) => "mission state is invalid",
            Self::RevisionAfterExecutionStarted => {
                "mission revision is unavailable after wave execution has started or completed"
            }
            Self::RevisionWithEvidence => {
                "mission revision is unavailable while evidence is bound to the active revision"
            }
            Self::RevisionNotMaterial => "mission revision does not change the active declaration",
        })
    }
}
impl std::error::Error for MissionArtifactError {}
