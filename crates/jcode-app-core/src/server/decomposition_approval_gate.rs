//! Server-authored approval gates for already-materialized decomposition DAGs.
//!
//! This deliberately starts no work. It binds the durable user decision to one
//! exact APPLIED packet, then leaves normal ingress and resume code to process a
//! later answer.

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use jcode_base::gates::{
    DeliveryAcknowledgement, GateBinding, GateContinuation, GateDecision, GateQuestion,
    GateRequest, GateState, GateStore, GateTransportBinding,
};
use sha2::{Digest, Sha256};

use crate::decomposition_materializer::{
    AppliedReceipt, DecompositionPacket, MaterializerError, PacketStore, SpecScoutResult,
};

#[path = "decomposition_approval_coverage.rs"]
mod coverage;

const GATE_ID_DOMAIN: &[u8] = b"jcode/decomposition-approval-gate/id/v1\0";
const GATE_LIFETIME: Duration = Duration::days(30);

impl PacketStore {
    /// Read the exact durable packet and APPLIED receipt that authorize a
    /// post-materialization workflow step. Both stored records and the supplied
    /// identity must agree, so a caller can never use a nearby packet revision.
    pub(crate) fn read_applied_exact(
        &self,
        session_id: &str,
        revision: u64,
        canonical_hash: &str,
        structural_digest: &str,
    ) -> Result<DecompositionPacket, MaterializerError> {
        let packet: DecompositionPacket = crate::decomposition_materializer::storage::read_json(
            &self.prepared_path(session_id, revision),
        )?;
        let rebuilt = DecompositionPacket::new(
            packet.root_prompt.clone(),
            packet.mission.clone(),
            packet.session_id.clone(),
            packet.revision,
            packet.material_scope.clone(),
            Some(packet.mode.clone()),
            packet.requested_nodes(),
        )?
        .with_coverage(packet.coverage.clone());
        if rebuilt != packet
            || packet.session_id != session_id
            || packet.revision != revision
            || packet.canonical_hash != canonical_hash
            || packet.structural_digest != structural_digest
        {
            return Err(MaterializerError::PacketConflict { revision });
        }
        let receipt: AppliedReceipt = crate::decomposition_materializer::storage::read_json(
            &self.applied_path(session_id, revision),
        )?;
        let expected_receipt = AppliedReceipt {
            session_id: packet.session_id.clone(),
            revision: packet.revision,
            packet_hash: packet.canonical_hash.clone(),
            structural_digest: packet.structural_digest.clone(),
        };
        if receipt != expected_receipt {
            return Err(MaterializerError::PacketConflict { revision });
        }
        Ok(packet)
    }
}

/// Create or return the one approval gate for an exact durable APPLIED packet.
/// The packet is read back from the durable receipt rather than trusting the
/// request payload that originally caused materialization.
pub(super) fn create_packet_approval_gate(
    packet_store: &PacketStore,
    packet: &DecompositionPacket,
    spec_scout: &SpecScoutResult,
) -> Result<GateRequest> {
    let packet = packet_store
        .read_applied_exact(
            &packet.session_id,
            packet.revision,
            &packet.canonical_hash,
            &packet.structural_digest,
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let gate_store = GateStore::from_jcode_home()?;
    create_for_materialized_packet(&packet, spec_scout, &gate_store, Utc::now())
}

fn create_for_materialized_packet(
    packet: &DecompositionPacket,
    spec_scout: &SpecScoutResult,
    store: &GateStore,
    now: DateTime<Utc>,
) -> Result<GateRequest> {
    let gate_id = gate_id_for(packet);
    if store.path_for(&gate_id, &packet.session_id)?.exists() {
        let existing = store.load(&gate_id, &packet.session_id)?;
        validate_existing_gate(&existing, packet, spec_scout)?;
        return Ok(existing);
    }

    let mut gate = gate_for(packet, spec_scout, gate_id, now, now + GATE_LIFETIME);
    let transport = GateTransportBinding::for_gate(&gate.gate_id, gate.binding.clone())?;
    gate.transport_binding = Some(transport.clone());
    store.create_pending_transported(gate.clone(), transport)?;
    Ok(gate)
}

fn validate_existing_gate(
    existing: &GateRequest,
    packet: &DecompositionPacket,
    spec_scout: &SpecScoutResult,
) -> Result<()> {
    let expected = gate_for(
        packet,
        spec_scout,
        existing.gate_id.clone(),
        existing.created_at,
        existing.expires_at,
    );
    if existing.schema_version != expected.schema_version
        || existing.binding != expected.binding
        || existing.question != expected.question
        || existing.allowed_decisions != expected.allowed_decisions
        || existing.continuation != expected.continuation
        || existing.transport_binding
            != Some(GateTransportBinding::for_gate(
                &expected.gate_id,
                expected.binding.clone(),
            )?)
    {
        bail!("conflicting decomposition approval gate identity")
    }
    Ok(())
}

fn gate_for(
    packet: &DecompositionPacket,
    spec_scout: &SpecScoutResult,
    gate_id: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> GateRequest {
    let binding =
        GateBinding::session_decision(&packet.session_id, packet.revision, &packet.canonical_hash);
    GateRequest {
        schema_version: 2,
        gate_id,
        binding,
        question: GateQuestion {
            question: "Approve this already-materialized decomposition DAG?".to_string(),
            problem: format!(
                "Decomposition packet revision {} is materialized for session {}.",
                packet.revision, packet.session_id
            ),
            impact: format!(
                "Approval applies only to DAG packet {} with structural digest {}. \
                 This gate does not start tasks.",
                packet.canonical_hash, packet.structural_digest
            ),
            options: super::spec_scout::gate_options(spec_scout),
            recommendation: coverage::recommendation(packet),
        },
        allowed_decisions: vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject,
        ],
        continuation: GateContinuation {
            on_approve: format!(
                "Approval recorded for packet {} (structural digest {}); no task is started by this gate.",
                packet.canonical_hash, packet.structural_digest
            ),
            on_request_changes: format!(
                "Changes requested for packet {} (structural digest {}); no task is started by this gate.",
                packet.canonical_hash, packet.structural_digest
            ),
            on_reject: format!(
                "Packet {} (structural digest {}) was rejected; no task is started by this gate.",
                packet.canonical_hash, packet.structural_digest
            ),
            on_expiry: "No decision was received; the materialized DAG remains unapproved."
                .to_string(),
        },
        created_at,
        expires_at,
        delivery: DeliveryAcknowledgement::NotAttempted,
        state: GateState::Pending,
        transport_binding: None,
        resume_directive: None,
    }
}

fn gate_id_for(packet: &DecompositionPacket) -> String {
    let mut hasher = Sha256::new();
    hasher.update(GATE_ID_DOMAIN);
    hasher.update(packet.session_id.as_bytes());
    hasher.update([0]);
    hasher.update(packet.revision.to_be_bytes());
    hasher.update(packet.canonical_hash.as_bytes());
    format!("decomposition-approval-{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decomposition_materializer::{
        MissionBinding, RequirementCoverageSummary, SpecScoutResult, WaveRequirementCoverage,
        reconcile_packet,
    };
    use crate::plan::VersionedPlan;
    use crate::protocol::TaskGraphNodeSpec;

    fn packet() -> DecompositionPacket {
        DecompositionPacket::new(
            "root".to_string(),
            MissionBinding {
                mission_id: "mission".to_string(),
                revision: 1,
                artifact_hash: "mission-hash".to_string(),
            },
            "gate-session".to_string(),
            1,
            "scope".to_string(),
            Some("light".to_string()),
            vec![TaskGraphNodeSpec {
                id: "node".to_string(),
                content: "task".to_string(),
                kind: Some("explore".to_string()),
                depends_on: vec![],
                priority: 0,
            }],
        )
        .unwrap()
    }

    fn packet_with_fixture_coverage() -> DecompositionPacket {
        packet().with_coverage(RequirementCoverageSummary {
            snapshot_present: true,
            waves: vec![WaveRequirementCoverage {
                wave_id: "wave-fixture".to_string(),
                requirement_ids: vec!["R-fixture".to_string()],
            }],
            unowned_required_ids: vec!["C-fixture".to_string()],
        })
    }

    #[test]
    fn materialized_gate_renders_fixture_coverage_and_unowned_gap() {
        let temp = tempfile::tempdir().unwrap();
        let gate_store = GateStore::at(temp.path().join("gates"));
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let scout = SpecScoutResult {
            contradictions: vec![],
            unstated_defaults: vec![],
        };
        let gate = create_for_materialized_packet(
            &packet_with_fixture_coverage(),
            &scout,
            &gate_store,
            now,
        )
        .expect("create an actual materialized approval gate");

        assert!(gate.question.recommendation.contains("wave-fixture"));
        assert!(gate.question.recommendation.contains("R-fixture"));
        assert!(gate.question.recommendation.contains("C-fixture"));
        assert!(gate.question.recommendation.contains("without an owner"));
        assert_ne!(
            gate.question.recommendation,
            "Review the exact materialized DAG before approving it."
        );
    }

    #[test]
    fn exact_replay_returns_one_pending_transported_gate_and_all_decisions() {
        let temp = tempfile::tempdir().unwrap();
        let packet_store = PacketStore::at(temp.path().join("packets"));
        let packet = packet();
        packet_store.prepare(&packet).unwrap();
        let mut plan = VersionedPlan::new();
        reconcile_packet(&packet, &mut plan).unwrap();
        packet_store.record_applied(&packet, &plan).unwrap();
        let gate_store = GateStore::at(temp.path().join("gates"));
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();

        let scout = SpecScoutResult {
            contradictions: vec![],
            unstated_defaults: vec![],
        };
        let first = create_for_materialized_packet(&packet, &scout, &gate_store, now).unwrap();
        let replay =
            create_for_materialized_packet(&packet, &scout, &gate_store, now + Duration::days(1))
                .unwrap();

        assert_eq!(first, replay);
        assert!(matches!(first.state, GateState::Pending));
        assert_eq!(
            first.binding,
            GateBinding::session_decision("gate-session", 1, &packet.canonical_hash)
        );
        assert!(first.question.impact.contains(&packet.structural_digest));
        assert_eq!(
            first.allowed_decisions,
            vec![
                GateDecision::Approve,
                GateDecision::RequestChanges,
                GateDecision::Reject
            ]
        );
        assert_eq!(
            gate_store.list_pending_transported_gates().unwrap(),
            vec![first]
        );
    }

    #[test]
    fn changed_structural_digest_at_same_revision_and_hash_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let gate_store = GateStore::at(temp.path().join("gates"));
        let packet = packet();
        let now = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let scout = SpecScoutResult {
            contradictions: vec![],
            unstated_defaults: vec![],
        };
        create_for_materialized_packet(&packet, &scout, &gate_store, now).unwrap();
        let mut changed = packet.clone();
        changed.structural_digest = "changed-digest".to_string();

        assert!(create_for_materialized_packet(&changed, &scout, &gate_store, now).is_err());
    }

    mod spec_scout_tests {
        include!("decomposition_approval_gate_spec_scout_tests.rs");
    }
}
