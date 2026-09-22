use crate::{MissionRevision, MissionRevisionError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ConstraintKind {
    Required = 1,
    Forbidden = 2,
    Scope = 3,
}

impl ConstraintKind {
    pub const fn requirement_prefix(self) -> char {
        match self {
            Self::Required => 'R',
            Self::Forbidden => 'F',
            Self::Scope => 'S',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionConstraint {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub kind: ConstraintKind,
    pub text: String,
}

impl MissionConstraint {
    pub fn new(kind: ConstraintKind, text: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            kind,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionSuccessCriterion {
    pub id: String,
    pub text: String,
}

impl MissionSuccessCriterion {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            text: text.into(),
        }
    }
}

impl From<String> for MissionSuccessCriterion {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<&str> for MissionSuccessCriterion {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl Serialize for MissionSuccessCriterion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.id.is_empty() {
            serializer.serialize_str(&self.text)
        } else {
            #[derive(Serialize)]
            struct Current<'a> {
                id: &'a str,
                text: &'a str,
            }
            Current {
                id: &self.id,
                text: &self.text,
            }
            .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for MissionSuccessCriterion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Legacy(String),
            Current {
                #[serde(default)]
                id: String,
                text: String,
            },
        }
        match Wire::deserialize(deserializer)? {
            Wire::Legacy(text) => Ok(Self::new(text)),
            Wire::Current { id, text } => Ok(Self { id, text }),
        }
    }
}

pub(crate) fn assign_requirement_ids(
    constraints: &mut [MissionConstraint],
    success_criteria: &mut [MissionSuccessCriterion],
    parent: Option<&MissionRevision>,
) -> Result<(), MissionRevisionError> {
    for kind in [
        ConstraintKind::Required,
        ConstraintKind::Forbidden,
        ConstraintKind::Scope,
    ] {
        let parent_ids: Vec<&str> = parent
            .map(|revision| {
                revision
                    .constraints()
                    .iter()
                    .filter(|constraint| constraint.kind == kind && !constraint.id.is_empty())
                    .map(|constraint| constraint.id.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let mut next = next_requirement_number(
            kind.requirement_prefix(),
            parent_ids.iter().copied().chain(
                constraints
                    .iter()
                    .filter(|constraint| constraint.kind == kind)
                    .map(|constraint| constraint.id.as_str()),
            ),
        );
        for (index, constraint) in constraints
            .iter_mut()
            .filter(|constraint| constraint.kind == kind)
            .enumerate()
        {
            if constraint.id.is_empty() {
                constraint.id = parent_ids
                    .get(index)
                    .map(|id| (*id).to_owned())
                    .unwrap_or_else(|| {
                        allocate_requirement_id(kind.requirement_prefix(), &mut next)
                    });
            }
        }
        let current: BTreeSet<&str> = constraints
            .iter()
            .filter(|constraint| constraint.kind == kind)
            .map(|constraint| constraint.id.as_str())
            .collect();
        if !parent_ids.iter().all(|id| current.contains(id)) {
            return Err(MissionRevisionError::RequirementIdRemoved);
        }
    }

    let parent_ids: Vec<&str> = parent
        .map(|revision| {
            revision
                .success_criteria()
                .iter()
                .filter(|criterion| !criterion.id.is_empty())
                .map(|criterion| criterion.id.as_str())
                .collect()
        })
        .unwrap_or_default();
    let mut next = next_requirement_number(
        'C',
        parent_ids.iter().copied().chain(
            success_criteria
                .iter()
                .map(|criterion| criterion.id.as_str()),
        ),
    );
    for (index, criterion) in success_criteria.iter_mut().enumerate() {
        if criterion.id.is_empty() {
            criterion.id = parent_ids
                .get(index)
                .map(|id| (*id).to_owned())
                .unwrap_or_else(|| allocate_requirement_id('C', &mut next));
        }
    }
    let current: BTreeSet<&str> = success_criteria
        .iter()
        .map(|criterion| criterion.id.as_str())
        .collect();
    if !parent_ids.iter().all(|id| current.contains(id)) {
        return Err(MissionRevisionError::RequirementIdRemoved);
    }

    let mut all_ids = BTreeSet::new();
    if constraints
        .iter()
        .map(|constraint| constraint.id.as_str())
        .chain(
            success_criteria
                .iter()
                .map(|criterion| criterion.id.as_str()),
        )
        .any(|id| !all_ids.insert(id))
    {
        return Err(MissionRevisionError::InvalidRequirementId);
    }
    Ok(())
}

pub(crate) fn valid_requirement_id(value: &str, prefix: char) -> bool {
    let Some(number) = value.strip_prefix(prefix) else {
        return false;
    };
    !number.is_empty() && number.parse::<u64>().is_ok_and(|number| number > 0)
}

fn allocate_requirement_id(prefix: char, next: &mut u64) -> String {
    let id = format!("{prefix}{next}");
    *next += 1;
    id
}

fn next_requirement_number<'a>(prefix: char, ids: impl Iterator<Item = &'a str>) -> u64 {
    ids.filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}
