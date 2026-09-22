use chrono::{TimeZone, Utc};
use jcode_mission_core::{
    ConstraintKind, MissionArtifact, MissionCas, MissionPathError, MissionPaths,
    MissionRevisionDraft, MissionState, MissionStatus, MissionStore, MissionStoreError,
    MissionTransition, MissionTransitionError, MissionWave, RevisionReason, RevisionSource,
    ScopeEntry, WaveStatus, render_projection,
};
use std::fs::{self, OpenOptions};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

fn artifact(root: &TempDir, path: &str) -> MissionArtifact {
    let mut state = MissionState::default();
    state
        .submit(jcode_mission_core::MissionRevisionDraft::new(
            "Ship mission sidecar",
            vec![jcode_mission_core::MissionConstraint::new(
                ConstraintKind::Required,
                "test it",
            )],
            vec!["sidecar works".to_owned()],
            RevisionReason::Initial,
            RevisionSource::DirectUserPrompt {
                source_trace_id: Some("trace-1".to_owned()),
            },
            Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        ))
        .unwrap();
    let paths = MissionPaths::resolve(root.path()).unwrap();
    MissionArtifact::from_state(
        paths.project_root_id(),
        &state,
        vec![MissionWave {
            id: "wave-1".to_owned(),
            scope: vec![ScopeEntry {
                relative_path: path.to_owned(),
                entity_id: "entity-1".to_owned(),
                write_set: true,
            }],
            status: WaveStatus::Pending,
        }],
    )
    .unwrap()
}

fn approved(root: &TempDir) -> MissionArtifact {
    let mut value = artifact(root, "src/lib.rs");
    value
        .approve(
            "operator",
            vec!["wave-1".to_owned()],
            Utc.timestamp_opt(1_700_000_001, 0).single().unwrap(),
        )
        .unwrap();
    value
}

#[test]
fn sidecar_create_load_restart_and_deterministic_projection_round_trip() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "// scope\n").unwrap();
    let value = approved(&root);
    let store = MissionStore::open(root.path()).unwrap();
    store.create(&value).unwrap();
    assert!(store.paths().sidecar().is_file());
    let loaded = store.load().unwrap();
    assert_eq!(loaded, value);
    assert_eq!(render_projection(&value), render_projection(&loaded));
    assert_eq!(
        fs::read(store.paths().markdown()).unwrap(),
        render_projection(&value).markdown
    );
    assert_eq!(
        MissionStore::open(root.path()).unwrap().load().unwrap(),
        value
    );
}

#[test]
fn tampered_or_unknown_hash_sidecar_fails_closed() {
    let root = TempDir::new().unwrap();
    let store = MissionStore::open(root.path()).unwrap();
    store.create(&artifact(&root, "future.rs")).unwrap();
    let sidecar = store.paths().sidecar();
    let original = fs::read_to_string(&sidecar).unwrap();
    fs::write(
        &sidecar,
        original.replacen("Ship mission sidecar", "Hack mission sidecar", 1),
    )
    .unwrap();
    assert!(matches!(
        store.load(),
        Err(MissionStoreError::InvalidSidecar(_))
    ));
    fs::write(&sidecar, original.replacen("sha256:v1:", "sha256:v9:", 1)).unwrap();
    assert!(matches!(
        store.load(),
        Err(MissionStoreError::InvalidSidecar(_))
    ));
}

#[test]
fn stale_cas_is_rejected_without_mutating_sidecar_bytes() {
    let root = TempDir::new().unwrap();
    let store = MissionStore::open(root.path()).unwrap();
    let value = approved(&root);
    let cas = MissionCas {
        mission_id: value.mission_id().clone(),
        expected_revision: 1,
        expected_state_hash: value.state_hash().clone(),
    };
    store.create(&value).unwrap();
    store
        .compare_and_swap(
            &cas,
            MissionTransition::StartWave {
                wave_id: "wave-1".to_owned(),
            },
        )
        .unwrap();
    let after = fs::read(store.paths().sidecar()).unwrap();
    assert!(matches!(
        store.compare_and_swap(
            &cas,
            MissionTransition::CompleteWave {
                wave_id: "wave-1".to_owned()
            }
        ),
        Err(MissionStoreError::Transition(
            MissionTransitionError::StaleStateHash
        ))
    ));
    assert_eq!(fs::read(store.paths().sidecar()).unwrap(), after);
}

#[test]
fn revision_transition_persists_history_invalidates_approval_and_rejects_stale_writes() {
    let root = TempDir::new().unwrap();
    let store = MissionStore::open(root.path()).unwrap();
    let value = approved(&root);
    let cas = MissionCas {
        mission_id: value.mission_id().clone(),
        expected_revision: value.active_revision().revision,
        expected_state_hash: value.state_hash().clone(),
    };
    store.create(&value).unwrap();

    let revised = store
        .compare_and_swap(
            &cas,
            MissionTransition::Revise {
                draft: MissionRevisionDraft::new(
                    "Ship revised mission sidecar",
                    vec![jcode_mission_core::MissionConstraint::new(
                        ConstraintKind::Required,
                        "review it",
                    )],
                    vec!["revised sidecar works".to_owned()],
                    RevisionReason::UserRevision,
                    RevisionSource::DirectUserPrompt {
                        source_trace_id: Some("trace-revision".to_owned()),
                    },
                    Utc.timestamp_opt(1_700_000_002, 0).single().unwrap(),
                ),
            },
        )
        .unwrap();
    assert_eq!(revised.revisions().len(), 2);
    assert_eq!(revised.active_revision().revision, 2);
    assert_eq!(revised.revisions()[1].parent_revision(), Some(1));
    assert_eq!(
        revised.revisions()[1].reason(),
        RevisionReason::UserRevision
    );
    assert!(revised.approval().is_none());
    assert_eq!(revised.status(), MissionStatus::Declared);
    assert_eq!(store.load().unwrap(), revised);

    let after_revision = fs::read(store.paths().sidecar()).unwrap();
    assert!(matches!(
        store.compare_and_swap(
            &cas,
            MissionTransition::Revise {
                draft: MissionRevisionDraft::new(
                    "Stale revision",
                    Vec::new(),
                    Vec::<String>::new(),
                    RevisionReason::UserRevision,
                    RevisionSource::DirectUserPrompt {
                        source_trace_id: None
                    },
                    Utc.timestamp_opt(1_700_000_003, 0).single().unwrap(),
                ),
            },
        ),
        Err(MissionStoreError::Transition(
            MissionTransitionError::StaleRevision { .. } | MissionTransitionError::StaleStateHash
        ))
    ));
    assert_eq!(fs::read(store.paths().sidecar()).unwrap(), after_revision);
}

#[test]
fn revision_after_execution_starts_is_rejected_without_mutating_the_sidecar() {
    let root = TempDir::new().unwrap();
    let store = MissionStore::open(root.path()).unwrap();
    let value = approved(&root);
    let cas = MissionCas {
        mission_id: value.mission_id().clone(),
        expected_revision: value.active_revision().revision,
        expected_state_hash: value.state_hash().clone(),
    };
    store.create(&value).unwrap();
    let executing = store
        .compare_and_swap(
            &cas,
            MissionTransition::StartWave {
                wave_id: "wave-1".to_owned(),
            },
        )
        .unwrap();
    let before_rejection = fs::read(store.paths().sidecar()).unwrap();
    let executing_cas = MissionCas {
        mission_id: executing.mission_id().clone(),
        expected_revision: executing.active_revision().revision,
        expected_state_hash: executing.state_hash().clone(),
    };

    assert!(matches!(
        store.compare_and_swap(
            &executing_cas,
            MissionTransition::Revise {
                draft: MissionRevisionDraft::new(
                    "Revision after execution",
                    Vec::new(),
                    Vec::<String>::new(),
                    RevisionReason::UserRevision,
                    RevisionSource::DirectUserPrompt {
                        source_trace_id: None
                    },
                    Utc.timestamp_opt(1_700_000_004, 0).single().unwrap(),
                ),
            },
        ),
        Err(MissionStoreError::Transition(
            MissionTransitionError::InvalidTransition(
                jcode_mission_core::MissionArtifactError::RevisionAfterExecutionStarted
            )
        ))
    ));
    assert_eq!(fs::read(store.paths().sidecar()).unwrap(), before_rejection);
}

#[test]
fn competing_writers_have_one_winner_and_readers_never_observe_partial_yaml() {
    let root = TempDir::new().unwrap();
    let value = approved(&root);
    let store = Arc::new(MissionStore::open(root.path()).unwrap());
    store.create(&value).unwrap();
    let cas = MissionCas {
        mission_id: value.mission_id().clone(),
        expected_revision: 1,
        expected_state_hash: value.state_hash().clone(),
    };
    let barrier = Arc::new(Barrier::new(3));
    let mut writers = Vec::new();
    for _ in 0..2 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        let cas = cas.clone();
        writers.push(thread::spawn(move || {
            barrier.wait();
            store.compare_and_swap(
                &cas,
                MissionTransition::Revise {
                    draft: MissionRevisionDraft::new(
                        "Concurrent mission revision",
                        vec![jcode_mission_core::MissionConstraint::new(
                            ConstraintKind::Required,
                            "one writer wins",
                        )],
                        vec!["sidecar remains complete".to_owned()],
                        RevisionReason::UserRevision,
                        RevisionSource::DirectUserPrompt {
                            source_trace_id: Some("trace-concurrent-revision".to_owned()),
                        },
                        Utc.timestamp_opt(1_700_000_005, 0).single().unwrap(),
                    ),
                },
            )
        }));
    }
    barrier.wait();
    for _ in 0..100 {
        let bytes = fs::read(store.paths().sidecar()).unwrap();
        serde_yaml::from_slice::<MissionArtifact>(&bytes).unwrap();
    }
    let successes = writers
        .into_iter()
        .map(|writer| writer.join().unwrap())
        .filter(Result::is_ok)
        .count();
    assert_eq!(successes, 1);
}

#[test]
fn lock_contention_waits_and_known_crash_temp_is_cleaned() {
    let root = TempDir::new().unwrap();
    let store = Arc::new(MissionStore::open(root.path()).unwrap());
    store.create(&artifact(&root, "future.rs")).unwrap();
    fs::write(store.paths().temp(), "partial").unwrap();
    assert!(store.load().is_ok());
    assert!(!store.paths().temp().exists());
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.paths().lock())
        .unwrap();
    file.lock().unwrap();
    let (sent, received) = mpsc::channel();
    let pending = Arc::clone(&store);
    let waiter = thread::spawn(move || {
        let result = pending.load();
        sent.send(result.is_ok()).unwrap();
    });
    assert!(received.recv_timeout(Duration::from_millis(40)).is_err());
    file.unlock().unwrap();
    assert!(received.recv_timeout(Duration::from_secs(2)).unwrap());
    waiter.join().unwrap();
}

#[test]
fn absolute_traversal_and_symlink_escapes_are_rejected() {
    let root = TempDir::new().unwrap();
    let paths = MissionPaths::resolve(root.path()).unwrap();
    assert!(matches!(
        paths.normalize_relative("/etc/passwd"),
        Err(MissionPathError::AbsolutePath)
    ));
    assert!(matches!(
        paths.normalize_relative("../outside"),
        Err(MissionPathError::Traversal)
    ));
    #[cfg(unix)]
    {
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        assert!(matches!(
            paths.normalize_relative("escape/file"),
            Err(MissionPathError::SymlinkEscape)
        ));
        let store = MissionStore::open(root.path()).unwrap();
        assert!(matches!(
            store.create(&artifact(&root, "escape/file")),
            Err(MissionStoreError::Path(MissionPathError::SymlinkEscape))
        ));
    }
}

#[test]
fn approval_is_exact_and_cannot_cross_mission_ids() {
    let first_root = TempDir::new().unwrap();
    let second_root = TempDir::new().unwrap();
    let first = approved(&first_root);
    let second = approved(&second_root);
    let first_store = MissionStore::open(first_root.path()).unwrap();
    let second_store = MissionStore::open(second_root.path()).unwrap();
    first_store.create(&first).unwrap();
    second_store.create(&second).unwrap();
    let foreign = MissionCas {
        mission_id: first.mission_id().clone(),
        expected_revision: 1,
        expected_state_hash: first.state_hash().clone(),
    };
    assert!(matches!(
        second_store.compare_and_swap(
            &foreign,
            MissionTransition::StartWave {
                wave_id: "wave-1".to_owned()
            }
        ),
        Err(MissionStoreError::Transition(
            MissionTransitionError::MissionMismatch
        ))
    ));
}
