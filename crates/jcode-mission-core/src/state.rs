use crate::requirements::assign_requirement_ids;
use crate::revision::{
    MissionId, MissionRevision, MissionRevisionDraft, MissionRevisionError, MissionRevisionRef,
    RevisionReason, RevisionSource,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MissionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mission_id: Option<MissionId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    revisions: Vec<MissionRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_revision: Option<MissionRevisionRef>,
}

#[derive(Deserialize)]
struct MissionStateWire {
    #[serde(default)]
    mission_id: Option<MissionId>,
    #[serde(default)]
    revisions: Vec<MissionRevision>,
    #[serde(default)]
    active_revision: Option<MissionRevisionRef>,
}

impl<'de> Deserialize<'de> for MissionState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MissionStateWire::deserialize(deserializer)?;
        let state = Self {
            mission_id: wire.mission_id,
            revisions: wire.revisions,
            active_revision: wire.active_revision,
        };
        state.validate().map_err(serde::de::Error::custom)?;
        Ok(state)
    }
}

impl MissionState {
    pub fn is_empty(&self) -> bool {
        self.mission_id.is_none() && self.revisions.is_empty() && self.active_revision.is_none()
    }

    pub fn mission_id(&self) -> Option<&MissionId> {
        self.mission_id.as_ref()
    }

    pub fn revisions(&self) -> &[MissionRevision] {
        &self.revisions
    }

    pub fn active_revision(&self) -> Option<&MissionRevision> {
        let active = self.active_revision.as_ref()?;
        self.revisions.iter().find(|revision| {
            revision.mission_id() == &active.mission_id && revision.revision() == active.revision
        })
    }

    pub fn submit(
        &mut self,
        mut draft: MissionRevisionDraft,
    ) -> Result<MissionRevision, MissionStateError> {
        self.validate()?;
        assign_requirement_ids(
            &mut draft.constraints,
            &mut draft.success_criteria,
            self.active_revision(),
        )
        .map_err(MissionStateError::InvalidRevision)?;
        if let Some(existing) = self
            .revisions
            .iter()
            .find(|revision| revision.same_submission_content(&draft))
        {
            return Ok(existing.clone());
        }

        let mission_id = self.mission_id.clone().unwrap_or_else(MissionId::new);
        let revision = (self.revisions.len() as u64) + 1;
        let parent_revision = revision.checked_sub(1).filter(|parent| *parent > 0);
        let revision = MissionRevision::new(
            mission_id,
            revision,
            parent_revision,
            draft.statement,
            draft.constraints,
            draft.success_criteria,
            draft.reason,
            draft.source,
            draft.created_at,
        )
        .map_err(MissionStateError::InvalidRevision)?;
        self.append_persisted(revision.clone())?;
        Ok(revision)
    }

    pub fn append_persisted(&mut self, revision: MissionRevision) -> Result<(), MissionStateError> {
        self.validate()?;
        revision
            .verify_canonical_hash()
            .map_err(MissionStateError::InvalidRevision)?;

        let expected = (self.revisions.len() as u64) + 1;
        if revision.revision() != expected {
            return Err(MissionStateError::NonMonotonicRevision {
                expected,
                actual: revision.revision(),
            });
        }
        if let Some(mission_id) = &self.mission_id
            && revision.mission_id() != mission_id
        {
            return Err(MissionStateError::MissionMismatch);
        }
        match expected {
            1 if revision.parent_revision().is_some() => {
                return Err(MissionStateError::FirstRevisionHasParent);
            }
            1 => {}
            _ => {
                let parent =
                    revision
                        .parent_revision()
                        .ok_or(MissionStateError::MissingParent {
                            revision: expected,
                            parent: expected - 1,
                        })?;
                if parent == revision.revision() {
                    return Err(MissionStateError::Cycle {
                        revision: revision.revision(),
                        parent,
                    });
                }
                if parent != expected - 1 {
                    return Err(MissionStateError::MissingParent {
                        revision: revision.revision(),
                        parent,
                    });
                }
            }
        }

        let mission_id = revision.mission_id().clone();
        let active_revision = revision.reference();
        self.mission_id = Some(mission_id);
        self.revisions.push(revision);
        self.active_revision = Some(active_revision);
        Ok(())
    }

    pub fn fork_root_from(
        source: &Self,
        created_at: DateTime<Utc>,
        source_trace_id: Option<String>,
    ) -> Result<Self, MissionStateError> {
        source.validate()?;
        let Some(active) = source.active_revision() else {
            return Ok(Self::default());
        };
        let root = MissionRevision::new(
            MissionId::new(),
            1,
            None,
            active.statement(),
            active.constraints().to_vec(),
            active.success_criteria().to_vec(),
            RevisionReason::SessionFork,
            RevisionSource::SessionFork {
                source_revision: active.reference(),
                source_trace_id,
            },
            created_at,
        )
        .map_err(MissionStateError::InvalidRevision)?;
        let mut fork = Self::default();
        fork.append_persisted(root)?;
        Ok(fork)
    }

    pub fn validate(&self) -> Result<(), MissionStateError> {
        if self.revisions.is_empty() {
            return if self.mission_id.is_none() && self.active_revision.is_none() {
                Ok(())
            } else {
                Err(MissionStateError::InvalidEmptyState)
            };
        }
        let mission_id = self
            .mission_id
            .as_ref()
            .ok_or(MissionStateError::MissingMissionId)?;
        for (index, revision) in self.revisions.iter().enumerate() {
            revision
                .verify_canonical_hash()
                .map_err(MissionStateError::InvalidRevision)?;
            let expected = (index as u64) + 1;
            if revision.revision() != expected {
                return Err(MissionStateError::NonMonotonicRevision {
                    expected,
                    actual: revision.revision(),
                });
            }
            if revision.mission_id() != mission_id {
                return Err(MissionStateError::MissionMismatch);
            }
            let expected_parent = expected.checked_sub(1).filter(|parent| *parent > 0);
            if revision.parent_revision() != expected_parent {
                return match revision.parent_revision() {
                    Some(parent) if parent == expected => Err(MissionStateError::Cycle {
                        revision: expected,
                        parent,
                    }),
                    Some(parent) => Err(MissionStateError::MissingParent {
                        revision: expected,
                        parent,
                    }),
                    None if expected == 1 => Ok(()),
                    None => Err(MissionStateError::MissingParent {
                        revision: expected,
                        parent: expected - 1,
                    }),
                };
            }
        }
        let Some(last_revision) = self.revisions.last() else {
            return Err(MissionStateError::InvalidEmptyState);
        };
        let expected_active = last_revision.reference();
        if self.active_revision.as_ref() != Some(&expected_active) {
            return Err(MissionStateError::ActiveRevisionMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissionStateError {
    InvalidEmptyState,
    MissingMissionId,
    ActiveRevisionMismatch,
    FirstRevisionHasParent,
    MissingParent { revision: u64, parent: u64 },
    Cycle { revision: u64, parent: u64 },
    NonMonotonicRevision { expected: u64, actual: u64 },
    MissionMismatch,
    InvalidRevision(MissionRevisionError),
}

impl fmt::Display for MissionStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEmptyState => {
                formatter.write_str("empty mission state has residual state")
            }
            Self::MissingMissionId => formatter.write_str("mission state is missing a mission ID"),
            Self::ActiveRevisionMismatch => {
                formatter.write_str("mission active revision does not point to the last revision")
            }
            Self::FirstRevisionHasParent => {
                formatter.write_str("mission revision 1 must not have a parent")
            }
            Self::MissingParent { revision, parent } => {
                write!(
                    formatter,
                    "mission revision {revision} is missing parent {parent}"
                )
            }
            Self::Cycle { revision, parent } => {
                write!(
                    formatter,
                    "mission revision {revision} creates a cycle through {parent}"
                )
            }
            Self::NonMonotonicRevision { expected, actual } => write!(
                formatter,
                "mission revision is not monotonic: expected {expected}, got {actual}"
            ),
            Self::MissionMismatch => {
                formatter.write_str("mission revision has a different mission ID")
            }
            Self::InvalidRevision(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MissionStateError {}
