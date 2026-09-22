use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::types::{validate_identifier, validate_text};

/// Immutable identity that binds a gate to one exact session and decision subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateBinding {
    pub session_id: String,
    pub subject: GateSubject,
}

/// The exact snapshot authorized by a gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GateSubject {
    SessionDecision {
        packet_revision: u64,
        packet_hash: String,
    },
    MissionPlan {
        mission_id: String,
        plan_revision: u64,
        plan_hash: String,
    },
}

impl Serialize for GateBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct V2<'a> {
            session_id: &'a str,
            subject: &'a GateSubject,
        }

        V2 {
            session_id: &self.session_id,
            subject: &self.subject,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GateBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct V2 {
            session_id: String,
            subject: GateSubject,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyMissionPlan {
            session_id: String,
            mission_id: String,
            plan_revision: u64,
            plan_hash: String,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Compatibility {
            V2(V2),
            LegacyMissionPlan(LegacyMissionPlan),
        }

        match Compatibility::deserialize(deserializer)? {
            Compatibility::V2(v2) => Ok(Self {
                session_id: v2.session_id,
                subject: v2.subject,
            }),
            Compatibility::LegacyMissionPlan(legacy) => Ok(Self::mission_plan(
                legacy.session_id,
                legacy.mission_id,
                legacy.plan_revision,
                legacy.plan_hash,
            )),
        }
    }
}

impl GateBinding {
    pub fn session_decision(
        session_id: impl Into<String>,
        packet_revision: u64,
        packet_hash: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            subject: GateSubject::SessionDecision {
                packet_revision,
                packet_hash: packet_hash.into(),
            },
        }
    }

    pub fn mission_plan(
        session_id: impl Into<String>,
        mission_id: impl Into<String>,
        plan_revision: u64,
        plan_hash: impl Into<String>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            subject: GateSubject::MissionPlan {
                mission_id: mission_id.into(),
                plan_revision,
                plan_hash: plan_hash.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_identifier("session", &self.session_id)?;
        match &self.subject {
            GateSubject::SessionDecision { packet_hash, .. } => {
                validate_text("packet hash", packet_hash)
            }
            GateSubject::MissionPlan {
                mission_id,
                plan_hash,
                ..
            } => {
                validate_identifier("mission", mission_id)?;
                validate_text("plan hash", plan_hash)
            }
        }
    }
}
