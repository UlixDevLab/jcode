use super::*;
fn mission() -> MissionBinding {
    MissionBinding {
        mission_id: "mission-1".to_string(),
        revision: 1,
        artifact_hash: "sha256:v1:mission".to_string(),
    }
}

fn nodes() -> Vec<TaskGraphNodeSpec> {
    vec![
        TaskGraphNodeSpec {
            id: "explore".to_string(),
            content: "inspect conventions".to_string(),
            kind: Some("explore".to_string()),
            depends_on: vec![],
            priority: 0,
        },
        TaskGraphNodeSpec {
            id: "synthesize".to_string(),
            content: "synthesize findings".to_string(),
            kind: Some("synthesize".to_string()),
            depends_on: vec!["explore".to_string()],
            priority: 1,
        },
    ]
}

fn packet(revision: u64) -> DecompositionPacket {
    DecompositionPacket::new(
        "add health check".to_string(),
        mission(),
        "root-session".to_string(),
        revision,
        "CLI health check".to_string(),
        Some("light".to_string()),
        nodes(),
    )
    .expect("valid packet")
}

fn fixture_coverage(unowned_required_ids: Vec<&str>) -> RequirementCoverageSummary {
    RequirementCoverageSummary {
        snapshot_present: true,
        waves: vec![WaveRequirementCoverage {
            wave_id: "wave-fixture".to_string(),
            requirement_ids: vec!["R-fixture".to_string()],
        }],
        unowned_required_ids: unowned_required_ids
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

#[test]
fn coverage_snapshot_is_bound_to_the_durable_packet() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = PacketStore::at(temp.path().join("packets"));
    let covered_packet = packet(1).with_coverage(fixture_coverage(vec!["C-fixture"]));
    assert_eq!(
        store.prepare(&covered_packet).unwrap(),
        PrepareOutcome::Prepared
    );
    let mut plan = VersionedPlan::new();
    reconcile_packet(&covered_packet, &mut plan).unwrap();
    store.record_applied(&covered_packet, &plan).unwrap();

    assert_eq!(
        store
            .read_applied_exact(
                "root-session",
                1,
                &covered_packet.canonical_hash,
                &covered_packet.structural_digest,
            )
            .unwrap(),
        covered_packet
    );
    let changed = packet(1).with_coverage(fixture_coverage(vec!["R-fixture-2"]));
    assert_ne!(covered_packet.canonical_hash, changed.canonical_hash);
}

#[test]
fn prepared_packet_reconciles_two_node_dag_and_receipts_after_readback() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = PacketStore::at(temp.path().join("packets"));
    let packet = packet(1);
    assert_eq!(store.prepare(&packet).unwrap(), PrepareOutcome::Prepared);
    let mut plan = VersionedPlan::new();
    assert_eq!(
        reconcile_packet(&packet, &mut plan).unwrap(),
        ReconcileOutcome::Inserted
    );
    assert_eq!(plan.items.len(), 2);
    assert!(plan.items.iter().all(|item| item.status == "queued"));
    store.record_applied(&packet, &plan).unwrap();
    assert_eq!(
        store.receipt("root-session", 1).unwrap().structural_digest,
        packet.structural_digest
    );
    assert_eq!(
        store
            .read_applied_exact(
                "root-session",
                1,
                &packet.canonical_hash,
                &packet.structural_digest,
            )
            .unwrap(),
        packet
    );
    assert!(matches!(
        store.read_applied_exact("root-session", 1, &packet.canonical_hash, "wrong-digest"),
        Err(MaterializerError::PacketConflict { .. })
    ));
}

#[test]
fn replay_and_restart_reconcile_without_resetting_execution_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = PacketStore::at(temp.path().join("packets"));
    let packet = packet(1);
    store.prepare(&packet).unwrap();
    let mut plan = VersionedPlan::new();
    reconcile_packet(&packet, &mut plan).unwrap();
    plan.items[0].status = "completed".to_string();
    plan.items[0].assigned_to = Some("worker".to_string());
    // Simulates a crash after durable graph write and before the APPLIED receipt.
    let restarted = PacketStore::at(temp.path().join("packets"));
    let loaded = restarted
        .active_prepared_for("root-session")
        .unwrap()
        .unwrap();
    assert_eq!(
        reconcile_packet(&loaded, &mut plan).unwrap(),
        ReconcileOutcome::Replay
    );
    assert_eq!(plan.items[0].status, "completed");
    assert_eq!(plan.items[0].assigned_to.as_deref(), Some("worker"));
    restarted.record_applied(&loaded, &plan).unwrap();
}

#[test]
fn changed_same_revision_conflicts_but_revision_two_is_separate_and_inactive() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = PacketStore::at(temp.path().join("packets"));
    let first = packet(1);
    assert_eq!(store.prepare(&first).unwrap(), PrepareOutcome::Prepared);
    let mut changed_nodes = nodes();
    changed_nodes[0].content = "different".to_string();
    let changed = DecompositionPacket::new(
        "add health check".to_string(),
        mission(),
        "root-session".to_string(),
        1,
        "CLI health check".to_string(),
        Some("light".to_string()),
        changed_nodes,
    )
    .unwrap();
    assert!(matches!(
        store.prepare(&changed),
        Err(MaterializerError::PacketConflict { .. })
    ));
    let second = packet(2);
    assert_eq!(
        store.prepare(&second).unwrap(),
        PrepareOutcome::InactiveRevision
    );
    assert!(store.active_prepared_for("root-session").unwrap().is_some());
}

#[test]
fn existing_matching_nodes_replay_and_conflicting_nodes_are_denied() {
    let packet = packet(1);
    let mut matching = VersionedPlan::new();
    reconcile_packet(&packet, &mut matching).unwrap();
    assert_eq!(
        reconcile_packet(&packet, &mut matching).unwrap(),
        ReconcileOutcome::Replay
    );
    let mut conflicting = VersionedPlan::new();
    let mut conflict = nodes();
    conflict[0].content = "different".to_string();
    let conflict_packet = DecompositionPacket::new(
        "other".to_string(),
        mission(),
        "other-session".to_string(),
        1,
        "scope".to_string(),
        Some("light".to_string()),
        conflict,
    )
    .unwrap();
    reconcile_packet(&conflict_packet, &mut conflicting).unwrap();
    assert!(matches!(
        reconcile_packet(&packet, &mut conflicting),
        Err(MaterializerError::Graph(_))
    ));
}
