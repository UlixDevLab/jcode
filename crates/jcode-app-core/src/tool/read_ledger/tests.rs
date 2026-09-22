use super::*;
use std::time::Duration;

fn version(len: u64, mtime_nanos: u128) -> FileVersion {
    FileVersion {
        len,
        mtime_nanos: Some(mtime_nanos),
    }
}

fn unique_session(name: &str) -> String {
    // Keep sessions disjoint across tests, since the ledger is process-global
    // and tests run concurrently.
    format!("test-{name}-{:?}", std::time::Instant::now())
}

#[test]
fn first_read_is_not_a_repeat() {
    let session = unique_session("first");
    let count = record_read(&session, Path::new("/tmp/a.rs"), 0, 100, version(10, 1));
    assert_eq!(count, 1);
    assert!(repeat_notice(count, "/tmp/a.rs").is_none());
    forget_session(&session);
}

#[test]
fn identical_reread_of_unchanged_file_is_flagged() {
    let session = unique_session("repeat");
    let path = Path::new("/tmp/a.rs");
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 2);
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 3);

    let notice = repeat_notice(3, "/tmp/a.rs").expect("third read should be flagged");
    assert!(notice.contains("already read"));
    assert!(notice.contains('3'));
    forget_session(&session);
}

#[test]
fn different_range_of_same_file_is_not_a_repeat() {
    // Paging through a large file is normal work, not duplication.
    let session = unique_session("paging");
    let path = Path::new("/tmp/big.rs");
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 100, 200, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 200, 300, version(10, 1)), 1);
    forget_session(&session);
}

#[test]
fn reread_after_file_changes_is_not_a_repeat() {
    // Re-reading a file you just edited is the correct thing to do, and must
    // never be discouraged.
    let session = unique_session("changed");
    let path = Path::new("/tmp/a.rs");
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 0, 100, version(12, 2)), 1);
    // The stale range is forgotten, so the new version starts its own count.
    assert_eq!(record_read(&session, path, 0, 100, version(12, 2)), 2);
    forget_session(&session);
}

#[test]
fn same_mtime_but_different_length_is_not_a_repeat() {
    // A fast edit can preserve mtime granularity; length disagreement alone is
    // enough to prove the content differs.
    let session = unique_session("len");
    let path = Path::new("/tmp/a.rs");
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 0, 100, version(99, 1)), 1);
    forget_session(&session);
}

#[test]
fn missing_mtime_never_claims_a_repeat() {
    // Without a modification time we cannot prove the file is unchanged, so we
    // must not tell the agent its context is still valid.
    let session = unique_session("nomtime");
    let path = Path::new("/tmp/a.rs");
    let unknown = FileVersion {
        len: 10,
        mtime_nanos: None,
    };
    assert_eq!(record_read(&session, path, 0, 100, unknown), 1);
    assert_eq!(record_read(&session, path, 0, 100, unknown), 1);
    forget_session(&session);
}

#[test]
fn sessions_are_isolated() {
    let a = unique_session("iso-a");
    let b = unique_session("iso-b");
    let path = Path::new("/tmp/a.rs");
    assert_eq!(record_read(&a, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&b, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&a, path, 0, 100, version(10, 1)), 2);
    forget_session(&a);
    forget_session(&b);
}

#[test]
fn forget_session_clears_history() {
    let session = unique_session("forget");
    let path = Path::new("/tmp/a.rs");
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 2);
    forget_session(&session);
    assert_eq!(record_read(&session, path, 0, 100, version(10, 1)), 1);
    forget_session(&session);
}

#[test]
fn ranges_per_path_are_bounded() {
    let session = unique_session("bounded-ranges");
    let path = Path::new("/tmp/big.rs");
    for index in 0..(MAX_RANGES_PER_PATH * 3) {
        let offset = index * 100;
        assert_eq!(
            record_read(&session, path, offset, offset + 100, version(10, 1)),
            1
        );
    }
    let ledger = LEDGER.lock().expect("ledger lock");
    let ranges = ledger
        .get(&session)
        .and_then(|paths| paths.get(path))
        .map(|records| records.len())
        .unwrap_or_default();
    assert!(
        ranges <= MAX_RANGES_PER_PATH,
        "tracked {ranges} ranges, expected at most {MAX_RANGES_PER_PATH}"
    );
    drop(ledger);
    forget_session(&session);
}

#[test]
fn paths_per_session_are_bounded() {
    let session = unique_session("bounded-paths");
    for index in 0..(MAX_TRACKED_PATHS_PER_SESSION + 25) {
        let path = PathBuf::from(format!("/tmp/file-{index}.rs"));
        record_read(&session, &path, 0, 100, version(10, 1));
    }
    let ledger = LEDGER.lock().expect("ledger lock");
    let paths = ledger.get(&session).map(|paths| paths.len()).unwrap_or(0);
    assert!(
        paths <= MAX_TRACKED_PATHS_PER_SESSION,
        "tracked {paths} paths, expected at most {MAX_TRACKED_PATHS_PER_SESSION}"
    );
    drop(ledger);
    forget_session(&session);
}

#[test]
fn reproduces_the_audited_worst_case() {
    // Regression guard for the concrete failure this module exists to address:
    // session_fox_1786443843602 read adaptive_scalp_router.py 31 times. The
    // 31st read must be visibly the 31st, so the agent can stop.
    let session = unique_session("audited");
    let path = Path::new("/repo/llm_trader/autolab/adaptive_scalp_router.py");
    let mut last = 0;
    for _ in 0..31 {
        last = record_read(&session, path, 0, 2000, version(48_000, 7));
    }
    assert_eq!(last, 31);
    let notice = repeat_notice(last, "adaptive_scalp_router.py").expect("must be flagged");
    assert!(notice.contains("31"));
    forget_session(&session);
}

#[test]
fn real_file_version_detects_modification() {
    // Guards the metadata plumbing, not just the in-memory bookkeeping.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sample.txt");
    std::fs::write(&path, "first").expect("write");
    let before = FileVersion::from_metadata(&std::fs::metadata(&path).expect("stat"));

    // Ensure the filesystem reports a distinct modification time.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, "second content is longer").expect("rewrite");
    let after = FileVersion::from_metadata(&std::fs::metadata(&path).expect("stat"));

    assert!(!before.is_same_version_as(&after));
    assert!(before.is_same_version_as(&before));
}
