use chrono::{TimeZone, Utc};
use jcode_mission_core::{
    ConstraintKind, EvidenceRecord, MissionArtifact, MissionConstraint, MissionCoverageVerdict,
    MissionId, MissionPaths, MissionRevision, MissionRevisionDraft, MissionState, MissionStore,
    MissionWave, RevisionReason, RevisionSource, ScopeEntry, WaveStatus,
};
use std::fs;

fn draft(
    constraints: Vec<MissionConstraint>,
    success_criteria: Vec<String>,
) -> MissionRevisionDraft {
    MissionRevisionDraft::new(
        "Preserve requirement coverage.",
        constraints,
        success_criteria,
        RevisionReason::Initial,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        Utc.timestamp_opt(1_700_000_001, 1).single().unwrap(),
    )
}

#[test]
fn requirements_keep_ids_when_a_revision_adds_items() {
    let mut state = MissionState::default();
    let first = state
        .submit(draft(
            vec![
                MissionConstraint::new(ConstraintKind::Required, "native tests"),
                MissionConstraint::new(ConstraintKind::Forbidden, "no reload"),
            ],
            vec!["focused checks pass".to_owned()],
        ))
        .unwrap();
    assert_eq!(first.constraints()[0].id, "R1");
    assert_eq!(first.constraints()[1].id, "F1");
    assert_eq!(first.success_criteria()[0].id, "C1");

    let second = state
        .submit(draft(
            vec![
                MissionConstraint::new(ConstraintKind::Required, "updated native tests"),
                MissionConstraint::new(ConstraintKind::Forbidden, "no reload"),
                MissionConstraint::new(ConstraintKind::Required, "review evidence"),
            ],
            vec![
                "updated focused checks pass".to_owned(),
                "coverage matrix exists".to_owned(),
            ],
        ))
        .unwrap();

    assert_eq!(second.constraints()[0].id, "R1");
    assert_eq!(second.constraints()[1].id, "F1");
    assert_eq!(second.constraints()[2].id, "R2");
    assert_eq!(second.success_criteria()[0].id, "C1");
    assert_eq!(second.success_criteria()[1].id, "C2");
}

#[test]
fn verifier_blocks_a_fixture_mission_with_an_uncovered_required_id() {
    let project = tempfile::tempdir().unwrap();
    let paths = MissionPaths::resolve(project.path()).unwrap();
    let mut state = MissionState::default();
    state
        .submit(draft(
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "native tests",
            )],
            vec!["focused checks pass".to_owned()],
        ))
        .unwrap();
    let mut artifact = MissionArtifact::from_state(
        paths.project_root_id(),
        &state,
        vec![MissionWave {
            id: "W1".to_owned(),
            scope: vec![ScopeEntry {
                relative_path: "crates/mission.rs".to_owned(),
                entity_id: "mission-core".to_owned(),
                write_set: true,
            }],
            status: WaveStatus::Pending,
        }],
    )
    .unwrap();
    artifact
        .record_evidence(EvidenceRecord {
            acceptance_id: "A".to_owned(),
            requirement_id: "C1".to_owned(),
            wave_id: "W1".to_owned(),
            reference: "cargo test -p jcode-mission-core".to_owned(),
            recorded_at: Utc.timestamp_opt(1_700_000_002, 2).single().unwrap(),
        })
        .unwrap();

    let report = artifact.verify_requirement_coverage();
    assert_eq!(report.verdict, MissionCoverageVerdict::Blocked);
    assert_eq!(report.entry("R1").unwrap().wave_id, None);
    assert_eq!(report.entry("R1").unwrap().evidence_record, None);
    assert_eq!(report.entry("C1").unwrap().wave_id.as_deref(), Some("W1"));
    assert_eq!(
        report.entry("C1").unwrap().evidence_record.as_deref(),
        Some("A")
    );
    let store = MissionStore::open(project.path()).unwrap();
    store.create(&artifact).unwrap();
    let markdown = fs::read_to_string(paths.markdown()).unwrap();
    assert!(markdown.contains("- Verdict: `blocked`"));
    assert!(markdown.contains("- R1 -> unmapped -> unmapped"));
}

#[test]
fn legacy_string_criteria_and_constraints_without_ids_still_deserialize() {
    let legacy = MissionRevision::new(
        MissionId::from_persisted("legacy-mission"),
        1,
        None,
        "Keep historical sidecars readable.",
        vec![MissionConstraint::new(
            ConstraintKind::Required,
            "preserve bytes",
        )],
        vec!["legacy criterion".to_owned()],
        RevisionReason::Initial,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        Utc.timestamp_opt(1_700_000_003, 3).single().unwrap(),
    )
    .unwrap();
    let persisted = serde_json::to_value(&legacy).unwrap();
    assert!(persisted["constraints"][0].get("id").is_none());
    assert!(persisted["success_criteria"][0].is_string());

    let recovered: MissionRevision = serde_json::from_value(persisted).unwrap();
    assert_eq!(recovered.constraints()[0].id, "");
    assert_eq!(recovered.success_criteria()[0].id, "");
    assert!(recovered.verify_canonical_hash().is_ok());
}
