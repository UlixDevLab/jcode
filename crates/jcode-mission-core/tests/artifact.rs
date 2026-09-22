use chrono::{TimeZone, Utc};
use jcode_mission_core::{
    ConstraintKind, MissionConstraint, MissionId, MissionRevision, MissionRevisionDraft,
    MissionState, MissionStateError, RevisionReason, RevisionSource,
};

fn draft(statement: &str, constraints: Vec<MissionConstraint>) -> MissionRevisionDraft {
    MissionRevisionDraft::new(
        statement,
        constraints,
        vec!["preserve history".to_owned()],
        RevisionReason::Initial,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        Utc.timestamp_opt(1_700_000_001, 1).single().unwrap(),
    )
}

#[test]
fn equivalent_submission_reuses_the_existing_revision_and_changed_content_appends() {
    let mut state = MissionState::default();
    let first = state
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "native tests",
            )],
        ))
        .unwrap();
    let repeated = state
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "native tests",
            )],
        ))
        .unwrap();
    assert_eq!(repeated, first);
    assert_eq!(state.revisions().len(), 1);

    let changed = state
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "fixed vectors",
            )],
        ))
        .unwrap();
    assert_eq!(changed.revision(), 2);
    assert_eq!(changed.parent_revision(), Some(1));
    assert_eq!(changed.mission_id(), first.mission_id());
    assert_ne!(changed.canonical_hash(), first.canonical_hash());
    assert_eq!(state.revisions().len(), 2);
    assert_eq!(state.revisions()[0], first);
    assert_eq!(state.active_revision(), Some(&changed));
}

#[test]
fn invalid_ancestry_is_rejected_atomically_with_stable_errors() {
    let mut state = MissionState::default();
    let first = state
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "native tests",
            )],
        ))
        .unwrap();
    let before = serde_json::to_vec(&state).unwrap();
    let timestamp = Utc.timestamp_opt(1_700_000_002, 2).single().unwrap();

    let missing_parent = MissionRevision::new(
        first.mission_id().clone(),
        2,
        Some(99),
        "Ship R1",
        first.constraints().to_vec(),
        first.success_criteria().to_vec(),
        RevisionReason::UserRevision,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        timestamp,
    )
    .unwrap();
    assert_eq!(
        state.append_persisted(missing_parent),
        Err(MissionStateError::MissingParent {
            revision: 2,
            parent: 99
        })
    );
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);

    let cycle = MissionRevision::new(
        first.mission_id().clone(),
        2,
        Some(2),
        "Ship R1",
        first.constraints().to_vec(),
        first.success_criteria().to_vec(),
        RevisionReason::UserRevision,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        timestamp,
    )
    .unwrap();
    assert_eq!(
        state.append_persisted(cycle),
        Err(MissionStateError::Cycle {
            revision: 2,
            parent: 2
        })
    );
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);

    let duplicate = MissionRevision::new(
        first.mission_id().clone(),
        1,
        None,
        "Ship R1",
        first.constraints().to_vec(),
        first.success_criteria().to_vec(),
        RevisionReason::Initial,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        timestamp,
    )
    .unwrap();
    assert_eq!(
        state.append_persisted(duplicate),
        Err(MissionStateError::NonMonotonicRevision {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);

    let mismatched = MissionRevision::new(
        MissionId::from_persisted("other-mission"),
        2,
        Some(1),
        "Ship R1",
        first.constraints().to_vec(),
        first.success_criteria().to_vec(),
        RevisionReason::UserRevision,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        timestamp,
    )
    .unwrap();
    assert_eq!(
        state.append_persisted(mismatched),
        Err(MissionStateError::MissionMismatch)
    );
    assert_eq!(serde_json::to_vec(&state).unwrap(), before);

    let mut tampered: serde_json::Value = serde_json::from_slice(&before).unwrap();
    tampered["revisions"][0]["canonical_hash"] = serde_json::Value::String(
        "sha256:v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
    );
    assert!(serde_json::from_value::<MissionState>(tampered).is_err());
}

#[test]
fn domain_fork_creates_only_a_new_revision_one_root() {
    let mut source = MissionState::default();
    let source_first = source
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "native tests",
            )],
        ))
        .unwrap();
    let source_active = source
        .submit(draft(
            "Ship R1",
            vec![MissionConstraint::new(
                ConstraintKind::Required,
                "fixed vectors",
            )],
        ))
        .unwrap();
    assert_eq!(source_first.revision(), 1);
    assert_eq!(source_active.revision(), 2);

    let fork = MissionState::fork_root_from(
        &source,
        Utc.timestamp_opt(1_700_000_003, 3).single().unwrap(),
        None,
    )
    .unwrap();
    let root = fork.active_revision().unwrap();
    assert_ne!(fork.mission_id(), source.mission_id());
    assert_eq!(fork.revisions().len(), 1);
    assert_eq!(root.revision(), 1);
    assert_eq!(root.parent_revision(), None);
    assert_eq!(root.statement(), source_active.statement());
    assert_eq!(root.constraints(), source_active.constraints());
    assert_eq!(root.success_criteria(), source_active.success_criteria());
    assert_eq!(root.reason(), RevisionReason::SessionFork);
    assert_eq!(
        root.source(),
        &RevisionSource::SessionFork {
            source_revision: source_active.reference(),
            source_trace_id: None,
        }
    );
    assert!(
        MissionState::fork_root_from(&MissionState::default(), Utc::now(), None)
            .unwrap()
            .is_empty()
    );
}
