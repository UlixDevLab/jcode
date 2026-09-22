use crate::MissionArtifact;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionCoverageVerdict {
    Accepted,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementCoverageEntry {
    pub requirement_id: String,
    pub wave_id: Option<String>,
    pub evidence_record: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionCoverageReport {
    pub verdict: MissionCoverageVerdict,
    pub entries: Vec<RequirementCoverageEntry>,
}

impl MissionCoverageReport {
    pub fn entry(&self, requirement_id: &str) -> Option<&RequirementCoverageEntry> {
        self.entries
            .iter()
            .find(|entry| entry.requirement_id == requirement_id)
    }
}

impl MissionArtifact {
    /// The production verifier input used by `MissionStore` projections and by
    /// Sentinel's requirement coverage matrix. Required constraints and success
    /// criteria without an evidence-to-wave edge block acceptance. Forbidden and
    /// scope entries remain visible but do not independently change the verdict.
    pub fn verify_requirement_coverage(&self) -> MissionCoverageReport {
        let mut entries = Vec::new();
        for constraint in self
            .active()
            .into_iter()
            .flat_map(|revision| revision.constraints())
        {
            if !constraint.id.is_empty() {
                entries.push(self.coverage_entry(&constraint.id));
            }
        }
        for criterion in self
            .active()
            .into_iter()
            .flat_map(|revision| revision.success_criteria())
        {
            if !criterion.id.is_empty() {
                entries.push(self.coverage_entry(&criterion.id));
            }
        }
        let verdict = if entries.iter().any(|entry| {
            matches!(entry.requirement_id.chars().next(), Some('R' | 'C'))
                && entry.evidence_record.is_none()
        }) {
            MissionCoverageVerdict::Blocked
        } else {
            MissionCoverageVerdict::Accepted
        };
        MissionCoverageReport { verdict, entries }
    }

    fn coverage_entry(&self, requirement_id: &str) -> RequirementCoverageEntry {
        let evidence = self
            .evidence()
            .iter()
            .find(|item| item.requirement_id == requirement_id);
        RequirementCoverageEntry {
            requirement_id: requirement_id.to_owned(),
            wave_id: evidence.map(|item| item.wave_id.clone()),
            evidence_record: evidence.map(|item| item.acceptance_id.clone()),
        }
    }
}
