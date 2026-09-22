use super::*;
use std::process::Command;

/// Run a git command, asserting success. Test setup must be unambiguous.
fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be available");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with one commit on `main` and deterministic identity, so
/// results do not depend on the developer's global git config.
fn init_repo(dir: &Path) {
    run(dir, &["init", "--initial-branch=main"]);
    run(dir, &["config", "user.email", "t@example.com"]);
    run(dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("shared.txt"), "base\n").unwrap();
    std::fs::write(dir.join("solo.txt"), "base\n").unwrap();
    run(dir, &["add", "."]);
    run(dir, &["commit", "-m", "base"]);
}

#[test]
fn single_worktree_repository_reports_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    // The common case: no contention is possible, so no notice and no cost.
    assert!(contested_file_notice(&tmp.path().join("shared.txt")).is_none());
}

#[test]
fn file_touched_by_a_second_worktree_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    std::fs::create_dir(&main).unwrap();
    init_repo(&main);

    // A second worktree edits shared.txt, exactly the ULIX i18n.js shape.
    let other = tmp.path().join("other");
    run(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "feature/other",
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("shared.txt"), "changed by other\n").unwrap();
    run(&other, &["add", "."]);
    run(&other, &["commit", "-m", "other edits shared"]);

    // The first worktree also changes it, making it genuinely contested.
    std::fs::write(main.join("shared.txt"), "changed by main\n").unwrap();
    run(&main, &["add", "."]);
    run(&main, &["commit", "-m", "main edits shared"]);

    // Editing from a third branch must name the competing branches.
    run(&main, &["checkout", "-b", "feature/third"]);
    let notice = contested_file_notice(&main.join("shared.txt"))
        .expect("a file changed on two branches is contested");
    assert!(notice.contains("feature/other"), "notice: {notice}");
    assert!(notice.contains("one writer per file"), "notice: {notice}");
}

#[test]
fn uncontested_file_in_a_multi_worktree_repository_is_quiet() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    std::fs::create_dir(&main).unwrap();
    init_repo(&main);

    let other = tmp.path().join("other");
    run(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "feature/other",
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("shared.txt"), "changed\n").unwrap();
    run(&other, &["add", "."]);
    run(&other, &["commit", "-m", "other edits shared"]);

    // solo.txt is touched by nobody: parallel work must stay silent on files
    // that are not actually shared, or the warning becomes noise.
    assert!(contested_file_notice(&main.join("solo.txt")).is_none());
}

#[test]
fn the_editing_branch_is_not_reported_against_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    std::fs::create_dir(&main).unwrap();
    init_repo(&main);

    let other = tmp.path().join("other");
    run(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "feature/other",
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("shared.txt"), "changed\n").unwrap();
    run(&other, &["add", "."]);
    run(&other, &["commit", "-m", "other edits shared"]);

    // Only one branch has touched the file, and it is the one editing now.
    // Warning here would be a false positive on ordinary solo work.
    assert!(contested_file_notice(&other.join("shared.txt")).is_none());
}

#[test]
fn a_path_outside_any_repository_is_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let loose = tmp.path().join("loose.txt");
    std::fs::write(&loose, "x").unwrap();
    // Editing a file outside a repo must never error or warn.
    assert!(contested_file_notice(&loose).is_none());
}

#[test]
fn append_leaves_an_uncontested_body_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    let mut body = "Edited file".to_string();
    append_contested_file_notice(&mut body, &tmp.path().join("solo.txt"));
    assert_eq!(body, "Edited file");
}
