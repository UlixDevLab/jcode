use crate::{
    decomposition_materializer::{
        DecompositionPacket, MissionBinding, PacketStore, RequirementCoverageSummary,
        SpecScoutContradiction, SpecScoutResult, SpecScoutUnstatedDefault, WaveRequirementCoverage,
    },
    gates::{GateDecision, GateState, GateStore, GateSubject},
};

struct ApprovalGateEnvGuard {
    _guard: std::sync::MutexGuard<'static, ()>,
    previous_runtime: Option<std::ffi::OsString>,
    previous_home: Option<std::ffi::OsString>,
}

impl ApprovalGateEnvGuard {
    fn new() -> (Self, tempfile::TempDir) {
        let guard = crate::storage::lock_test_env();
        let temp = tempfile::TempDir::new().unwrap();
        let previous_runtime = std::env::var_os("JCODE_RUNTIME_DIR");
        let previous_home = std::env::var_os("JCODE_HOME");
        crate::env::set_var("JCODE_RUNTIME_DIR", temp.path());
        crate::env::set_var("JCODE_HOME", temp.path().join("home"));
        (
            Self {
                _guard: guard,
                previous_runtime,
                previous_home,
            },
            temp,
        )
    }
}

impl Drop for ApprovalGateEnvGuard {
    fn drop(&mut self) {
        match self.previous_runtime.take() {
            Some(value) => crate::env::set_var("JCODE_RUNTIME_DIR", value),
            None => crate::env::remove_var("JCODE_RUNTIME_DIR"),
        }
        match self.previous_home.take() {
            Some(value) => crate::env::set_var("JCODE_HOME", value),
            None => crate::env::remove_var("JCODE_HOME"),
        }
    }
}

fn materialized_packet(session_id: &str, nodes: Vec<TaskGraphNodeSpec>) -> DecompositionPacket {
    DecompositionPacket::new(
        "materialize an approval-gated DAG".to_string(),
        MissionBinding {
            mission_id: "mission-materialized".to_string(),
            revision: 1,
            artifact_hash: "sha256:mission-materialized".to_string(),
        },
        session_id.to_string(),
        1,
        "approval-gated decomposition".to_string(),
        Some("light".to_string()),
        nodes,
    )
    .unwrap()
    .with_coverage(RequirementCoverageSummary {
        snapshot_present: true,
        waves: vec![WaveRequirementCoverage {
            wave_id: "wave-fixture".to_string(),
            requirement_ids: vec!["R-fixture".to_string()],
        }],
        unowned_required_ids: vec!["C-fixture".to_string()],
    })
}

fn planted_scout_result() -> SpecScoutResult {
    SpecScoutResult {
        contradictions: vec![SpecScoutContradiction {
            a: "partner natural key is name".to_string(),
            b: "all conflicts use qid".to_string(),
            fact: "partner conflict key".to_string(),
        }],
        unstated_defaults: vec![SpecScoutUnstatedDefault {
            behavior: "deleted upstream rows".to_string(),
            question: "What happens to rows deleted upstream?".to_string(),
        }],
    }
}

fn durable_spec_scout_receipt_path(packet: &DecompositionPacket) -> std::path::PathBuf {
    crate::storage::durable_state_dir()
        .join("swarm-decomposition")
        .join(format!(
            "{}-{}.spec-scout.json",
            packet.session_id, packet.revision
        ))
}

#[tokio::test]
async fn e2e_applied_packet_creates_one_exact_transported_gate_without_starting_tasks() {
    let (_env, _runtime) = ApprovalGateEnvGuard::new();
    let mut fx = graph_fixture_named("swarm-materialized", "coord-materialized", "worker-m").await;
    let nodes = vec![
        node_spec("explore", "explore", &[]),
        node_spec("synthesize", "synthesize", &["explore"]),
    ];
    let packet = materialized_packet(&fx.coord, nodes.clone());
    PacketStore::durable().prepare(&packet).unwrap();
    PacketStore::durable()
        .record_spec_scout(&packet, &planted_scout_result())
        .unwrap();

    fx.seed("light", nodes.clone()).await;

    let gate_store = GateStore::from_jcode_home().unwrap();
    let first = gate_store.list_pending_transported_gates().unwrap();
    assert_eq!(first.len(), 1);
    let gate = &first[0];
    assert!(matches!(gate.state, GateState::Pending));
    let GateSubject::SessionDecision {
        packet_revision,
        packet_hash,
    } = &gate.binding.subject
    else {
        panic!("approval gate must bind a session decision");
    };
    assert_eq!(*packet_revision, packet.revision);
    assert_eq!(packet_hash, &packet.canonical_hash);
    assert!(gate.question.impact.contains(&packet.structural_digest));
    assert!(gate.question.recommendation.contains("wave-fixture"));
    assert!(gate.question.recommendation.contains("R-fixture"));
    assert!(gate.question.recommendation.contains("C-fixture"));
    assert!(gate.question.recommendation.contains("without an owner"));
    assert!(
        gate.question
            .options
            .iter()
            .any(|option| option.label == "Resolve contradiction: partner conflict key")
    );
    assert!(
        gate.question
            .options
            .iter()
            .any(|option| option.description == "What happens to rows deleted upstream?")
    );
    assert_eq!(
        gate.allowed_decisions,
        vec![
            GateDecision::Approve,
            GateDecision::RequestChanges,
            GateDecision::Reject
        ]
    );
    let first_bytes = serde_json::to_vec(gate).unwrap();
    assert!(
        fx.swarm_plans.read().await[&fx.swarm_id]
            .items
            .iter()
            .all(|item| item.status == "queued" && item.assigned_to.is_none())
    );

    fx.seed("light", nodes).await;
    let replay = gate_store.list_pending_transported_gates().unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(serde_json::to_vec(&replay[0]).unwrap(), first_bytes);
}

#[tokio::test]
async fn e2e_prepared_packet_without_a_valid_scout_result_creates_no_approval_gate() {
    let (_env, _runtime) = ApprovalGateEnvGuard::new();
    let mut fx = graph_fixture_named("swarm-no-scout", "coord-no-scout", "worker-s").await;
    let packet = materialized_packet(&fx.coord, vec![node_spec("expected", "explore", &[])]);
    PacketStore::durable().prepare(&packet).unwrap();

    fx.seed("light", vec![node_spec("expected", "explore", &[])])
        .await;

    assert!(
        GateStore::from_jcode_home()
            .unwrap()
            .list_pending_transported_gates()
            .unwrap()
            .is_empty(),
        "missing scout output must not silently create an approval gate"
    );
}

#[tokio::test]
async fn e2e_malformed_durable_scout_receipt_cannot_reach_applied_or_approval_gate() {
    let (_env, _runtime) = ApprovalGateEnvGuard::new();
    let mut fx = graph_fixture_named(
        "swarm-malformed-receipt",
        "coord-malformed-receipt",
        "worker-mr",
    )
    .await;
    let nodes = vec![node_spec("expected", "explore", &[])];
    let packet = materialized_packet(&fx.coord, nodes.clone());
    let store = PacketStore::durable();
    store.prepare(&packet).unwrap();
    std::fs::write(durable_spec_scout_receipt_path(&packet), b"{not valid json").unwrap();

    fx.seed("light", nodes).await;

    assert!(store.receipt(&packet.session_id, packet.revision).is_none());
    assert!(
        GateStore::from_jcode_home()
            .unwrap()
            .list_pending_transported_gates()
            .unwrap()
            .is_empty(),
        "a malformed durable scout receipt must not create an approval gate"
    );
    assert!(matches!(
        fx.client_rx.try_recv(),
        Ok(ServerEvent::Error { .. })
    ));
}

#[tokio::test]
async fn e2e_stale_durable_scout_receipt_cannot_reach_applied_or_approval_gate() {
    let (_env, _runtime) = ApprovalGateEnvGuard::new();
    let mut fx =
        graph_fixture_named("swarm-stale-receipt", "coord-stale-receipt", "worker-sr").await;
    let nodes = vec![node_spec("expected", "explore", &[])];
    let packet = materialized_packet(&fx.coord, nodes.clone());
    let store = PacketStore::durable();
    store.prepare(&packet).unwrap();
    store
        .record_spec_scout(&packet, &planted_scout_result())
        .unwrap();
    let receipt_path = durable_spec_scout_receipt_path(&packet);
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
    receipt["packet_hash"] = serde_json::Value::String("sha256:stale-receipt".to_string());
    std::fs::write(receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();

    fx.seed("light", nodes).await;

    assert!(store.receipt(&packet.session_id, packet.revision).is_none());
    assert!(
        GateStore::from_jcode_home()
            .unwrap()
            .list_pending_transported_gates()
            .unwrap()
            .is_empty(),
        "a stale durable scout receipt must not create an approval gate"
    );
    assert!(matches!(
        fx.client_rx.try_recv(),
        Ok(ServerEvent::Error { .. })
    ));
}

#[tokio::test]
async fn e2e_rejected_materialization_creates_no_approval_gate() {
    let (_env, _runtime) = ApprovalGateEnvGuard::new();
    let mut fx = graph_fixture_named("swarm-no-gate", "coord-no-gate", "worker-n").await;
    let packet = materialized_packet(&fx.coord, vec![node_spec("expected", "explore", &[])]);
    PacketStore::durable().prepare(&packet).unwrap();
    fx.seed("light", vec![node_spec("different", "explore", &[])])
        .await;

    assert!(
        GateStore::from_jcode_home()
            .unwrap()
            .list_pending_transported_gates()
            .unwrap()
            .is_empty()
    );
    assert!(fx.swarm_plans.read().await[&fx.swarm_id].items.is_empty());
}
