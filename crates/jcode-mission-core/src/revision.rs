use crate::canonical::{CanonicalArtifactHash, CanonicalWriter};
use crate::requirements::{MissionConstraint, MissionSuccessCriterion, valid_requirement_id};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

const ENTITY: &str = "mission-revision";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MissionId(String);

impl MissionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_persisted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionRevisionId {
    pub mission_id: MissionId,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionRevisionRef {
    pub mission_id: MissionId,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum RevisionReason {
    Initial = 1,
    UserRevision = 2,
    EvidenceRevision = 3,
    SessionFork = 4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionSource {
    DirectUserPrompt {
        source_trace_id: Option<String>,
    },
    SessionFork {
        source_revision: MissionRevisionRef,
        source_trace_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRevisionDraft {
    pub(crate) statement: String,
    pub(crate) constraints: Vec<MissionConstraint>,
    pub(crate) success_criteria: Vec<MissionSuccessCriterion>,
    pub(crate) reason: RevisionReason,
    pub(crate) source: RevisionSource,
    pub(crate) created_at: DateTime<Utc>,
}

impl MissionRevisionDraft {
    pub fn new<I, T>(
        statement: impl Into<String>,
        constraints: Vec<MissionConstraint>,
        success_criteria: I,
        reason: RevisionReason,
        source: RevisionSource,
        created_at: DateTime<Utc>,
    ) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<MissionSuccessCriterion>,
    {
        Self {
            statement: statement.into(),
            constraints,
            success_criteria: success_criteria.into_iter().map(Into::into).collect(),
            reason,
            source,
            created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRevision {
    mission_id: MissionId,
    revision: u64,
    parent_revision: Option<u64>,
    statement: String,
    constraints: Vec<MissionConstraint>,
    success_criteria: Vec<MissionSuccessCriterion>,
    reason: RevisionReason,
    source: RevisionSource,
    created_at: DateTime<Utc>,
    canonical_hash: CanonicalArtifactHash,
}

impl MissionRevision {
    #[allow(clippy::too_many_arguments)]
    pub fn new<I, T>(
        mission_id: MissionId,
        revision: u64,
        parent_revision: Option<u64>,
        statement: impl Into<String>,
        constraints: Vec<MissionConstraint>,
        success_criteria: I,
        reason: RevisionReason,
        source: RevisionSource,
        created_at: DateTime<Utc>,
    ) -> Result<Self, MissionRevisionError>
    where
        I: IntoIterator<Item = T>,
        T: Into<MissionSuccessCriterion>,
    {
        let revision = Self {
            mission_id,
            revision,
            parent_revision,
            statement: statement.into(),
            constraints,
            success_criteria: success_criteria.into_iter().map(Into::into).collect(),
            reason,
            source,
            created_at,
            canonical_hash: CanonicalArtifactHash::from_canonical_bytes(ENTITY, &[]),
        };
        revision.validate_fields()?;
        let canonical_hash =
            CanonicalArtifactHash::from_canonical_bytes(ENTITY, &revision.canonical_payload());
        Ok(Self {
            canonical_hash,
            ..revision
        })
    }

    pub fn reference(&self) -> MissionRevisionRef {
        MissionRevisionRef {
            mission_id: self.mission_id.clone(),
            revision: self.revision,
        }
    }

    pub fn mission_id(&self) -> &MissionId {
        &self.mission_id
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn parent_revision(&self) -> Option<u64> {
        self.parent_revision
    }

    pub fn statement(&self) -> &str {
        &self.statement
    }

    pub fn constraints(&self) -> &[MissionConstraint] {
        &self.constraints
    }

    pub fn success_criteria(&self) -> &[MissionSuccessCriterion] {
        &self.success_criteria
    }

    pub const fn reason(&self) -> RevisionReason {
        self.reason
    }

    pub fn source(&self) -> &RevisionSource {
        &self.source
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn canonical_hash(&self) -> &CanonicalArtifactHash {
        &self.canonical_hash
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = format!("jcode.r1/{ENTITY}/canonical/v1\0").into_bytes();
        bytes.extend(self.canonical_payload());
        bytes
    }

    pub fn verify_canonical_hash(&self) -> Result<(), MissionRevisionError> {
        self.validate_fields()?;
        let expected =
            CanonicalArtifactHash::from_canonical_bytes(ENTITY, &self.canonical_payload());
        if self.canonical_hash == expected {
            Ok(())
        } else {
            Err(MissionRevisionError::CanonicalHashMismatch)
        }
    }

    pub(crate) fn same_submission_content(&self, draft: &MissionRevisionDraft) -> bool {
        self.statement == draft.statement
            && self.constraints == draft.constraints
            && self.success_criteria == draft.success_criteria
    }

    fn validate_fields(&self) -> Result<(), MissionRevisionError> {
        if self.mission_id.as_str().is_empty() {
            return Err(MissionRevisionError::EmptyMissionId);
        }
        if self.revision == 0 {
            return Err(MissionRevisionError::ZeroRevision);
        }
        if self.statement.is_empty() {
            return Err(MissionRevisionError::EmptyStatement);
        }
        if self
            .constraints
            .iter()
            .any(|constraint| constraint.text.is_empty())
        {
            return Err(MissionRevisionError::EmptyConstraint);
        }
        if self
            .success_criteria
            .iter()
            .any(|criterion| criterion.text.is_empty())
        {
            return Err(MissionRevisionError::EmptySuccessCriterion);
        }
        if self.constraints.iter().any(|constraint| {
            !constraint.id.is_empty()
                && !valid_requirement_id(&constraint.id, constraint.kind.requirement_prefix())
        }) || self
            .success_criteria
            .iter()
            .any(|criterion| !criterion.id.is_empty() && !valid_requirement_id(&criterion.id, 'C'))
        {
            return Err(MissionRevisionError::InvalidRequirementId);
        }
        if let RevisionSource::SessionFork {
            source_revision, ..
        } = &self.source
            && (source_revision.mission_id.as_str().is_empty() || source_revision.revision == 0)
        {
            return Err(MissionRevisionError::InvalidForkSource);
        }
        Ok(())
    }

    fn canonical_payload(&self) -> Vec<u8> {
        let mut writer = CanonicalWriter::default();
        // Field tags are frozen in ascending declaration order for v1.
        writer.field(1);
        writer.text(self.mission_id.as_str());
        writer.field(2);
        writer.u64(self.revision);
        writer.field(3);
        match self.parent_revision {
            None => writer.u8(0),
            Some(parent_revision) => {
                writer.u8(1);
                writer.u64(parent_revision);
            }
        }
        writer.field(4);
        writer.text(&self.statement);
        writer.field(5);
        writer.u64(self.constraints.len() as u64);
        for constraint in &self.constraints {
            writer.u8(constraint.kind as u8);
            writer.text(&constraint.text);
            if !constraint.id.is_empty() {
                writer.text(&constraint.id);
            }
        }
        writer.field(6);
        writer.u64(self.success_criteria.len() as u64);
        for criterion in &self.success_criteria {
            writer.text(&criterion.text);
            if !criterion.id.is_empty() {
                writer.text(&criterion.id);
            }
        }
        writer.field(7);
        writer.u8(self.reason as u8);
        writer.field(8);
        match &self.source {
            RevisionSource::DirectUserPrompt { source_trace_id } => {
                writer.u8(1);
                writer.option_text(source_trace_id.as_deref());
            }
            RevisionSource::SessionFork {
                source_revision,
                source_trace_id,
            } => {
                writer.u8(2);
                writer.text(source_revision.mission_id.as_str());
                writer.u64(source_revision.revision);
                writer.option_text(source_trace_id.as_deref());
            }
        }
        writer.field(9);
        writer.i64(self.created_at.timestamp());
        writer.u32(self.created_at.timestamp_subsec_nanos());
        writer.into_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionRevisionError {
    EmptyMissionId,
    ZeroRevision,
    EmptyStatement,
    EmptyConstraint,
    EmptySuccessCriterion,
    InvalidForkSource,
    InvalidRequirementId,
    RequirementIdRemoved,
    CanonicalHashMismatch,
}

impl fmt::Display for MissionRevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyMissionId => "mission ID must not be empty",
            Self::ZeroRevision => "mission revision must be positive",
            Self::EmptyStatement => "mission statement must not be empty",
            Self::EmptyConstraint => "mission constraint must not be empty",
            Self::EmptySuccessCriterion => "mission success criterion must not be empty",
            Self::InvalidForkSource => "fork source revision is invalid",
            Self::InvalidRequirementId => "mission requirement ID is invalid",
            Self::RequirementIdRemoved => "mission revision removed a requirement ID",
            Self::CanonicalHashMismatch => "mission revision canonical hash does not match content",
        })
    }
}

impl std::error::Error for MissionRevisionError {}
