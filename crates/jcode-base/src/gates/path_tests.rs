use super::*;

#[cfg(unix)]
#[test]
fn root_symlink_is_rejected_without_mutating_outside_storage() {
    let temp = TempDir::new().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    let root = temp.path().join("gates");
    symlink(&outside, &root).unwrap();
    let error = store(&temp).create(request()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing symlinked gate ledger path")
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"untouched");
}

#[cfg(unix)]
#[test]
fn session_parent_symlink_is_rejected_without_mutating_outside_storage() {
    let temp = TempDir::new().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    let root = temp.path().join("gates");
    std::fs::create_dir(&root).unwrap();
    symlink(&outside, root.join(request().binding.session_id)).unwrap();
    let error = store(&temp).create(request()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing symlinked gate ledger path")
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"untouched");
}

#[cfg(unix)]
#[test]
fn record_symlink_is_rejected_without_mutating_outside_storage() {
    let temp = TempDir::new().unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let sentinel = outside.join("sentinel");
    std::fs::write(&sentinel, b"untouched").unwrap();
    let root = temp.path().join("gates");
    let session = root.join(request().binding.session_id);
    std::fs::create_dir_all(&session).unwrap();
    symlink(&sentinel, session.join("gate-123.json")).unwrap();
    let error = store(&temp).create(request()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing symlinked gate ledger path")
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"untouched");
}
