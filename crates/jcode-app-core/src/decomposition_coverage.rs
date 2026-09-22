use jcode_mission_core::MissionArtifact;
use serde::{Deserialize, Serialize};

/// Immutable coverage information derived from the mission artifact at the
/// moment a root materialization is prepared. This lets the later approval gate
/// show what it is authorizing without re-reading mutable mission state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RequirementCoverageSummary {
    #[serde(default)]
    pub snapshot_present: bool,
    #[serde(default)]
    pub waves: Vec<WaveRequirementCoverage>,
    #[serde(default)]
    pub unowned_required_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WaveRequirementCoverage {
    pub wave_id: String,
    pub requirement_ids: Vec<String>,
}

impl RequirementCoverageSummary {
    pub(crate) fn from_artifact(artifact: &MissionArtifact) -> Self {
        let mut waves: Vec<WaveRequirementCoverage> = artifact
            .waves()
            .iter()
            .map(|wave| WaveRequirementCoverage {
                wave_id: wave.id.clone(),
                requirement_ids: Vec::new(),
            })
            .collect();
        let mut unowned_required_ids = Vec::new();
        for entry in artifact.verify_requirement_coverage().entries {
            if !matches!(entry.requirement_id.chars().next(), Some('R' | 'C')) {
                continue;
            }
            match entry
                .wave_id
                .as_deref()
                .and_then(|wave_id| waves.iter_mut().find(|wave| wave.wave_id == wave_id))
            {
                Some(wave) => wave.requirement_ids.push(entry.requirement_id),
                None => unowned_required_ids.push(entry.requirement_id),
            }
        }
        for wave in &mut waves {
            wave.requirement_ids.sort();
            wave.requirement_ids.dedup();
        }
        waves.sort_by(|left, right| left.wave_id.cmp(&right.wave_id));
        unowned_required_ids.sort();
        unowned_required_ids.dedup();
        Self {
            snapshot_present: true,
            waves,
            unowned_required_ids,
        }
    }
}
