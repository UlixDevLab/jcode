//! Warn when a worktree branch has also changed a file.
//!
//! The system prompt tells agents to keep one writer per file, and
//! `swarm-prompt.md` has said "keep one writer for overlapping files" since the
//! first commit. Neither helped, because an agent has no way to *see* that
//! another branch is editing the same file: sessions are isolated by design and
//! a worktree is invisible from inside another worktree.
//!
//! A rule nobody can check is not followed. On 2026-08-18..20 seven branches
//! edited one file (`public/js/i18n.js`) in 48 hours, producing
//! fix-A-breaks-B oscillation as each merge silently undid the last.
//!
//! So this makes branch overlap observable at the only moment it matters: the
//! edit itself. When a tool writes a file that another worktree branch has also
//! modified relative to the integration base, the tool result carries a notice
//! naming the competing branches. This is a clue to check real ownership and
//! integration, not proof of simultaneous editing.
//!
//! Design constraints:
//!
//! * **Never fail an edit.** Every git failure degrades to "no notice". A
//!   missing repo, detached HEAD, or unusual layout must not block work.
//! * **Cheap.** Single-worktree repositories are the overwhelming majority, so
//!   the first check is `git worktree list` and it exits immediately when only
//!   one worktree exists. Results are cached per repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a computed claim map stays usable.
///
/// Branch contents change on the timescale of commits, not keystrokes, so a
/// short cache removes the per-edit git cost while staying fresh enough that a
/// newly created worktree is noticed within the same working session.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Upper bound on paths recorded per worktree branch.
///
/// A long-lived branch that has drifted far from the base can touch thousands
/// of files. Those are stale-branch noise rather than live contention, and
/// letting one branch dominate the map would both slow the check and bury the
/// real conflicts.
const MAX_PATHS_PER_BRANCH: usize = 4000;

/// Competing branches for one repository, keyed by repo-relative path.
type ClaimMap = HashMap<String, Vec<String>>;

struct CacheEntry {
    computed_at: Instant,
    claims: ClaimMap,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run a git command in `dir`, returning stdout when git exits successfully.
///
/// A non-zero exit is a normal answer here (no such ref, not a repository), not
/// an error worth surfacing, so both failure modes collapse to `None`.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The integration base to diff branches against.
///
/// Prefers the *remote* head. A stale local `main` makes every branch appear to
/// change files that merely landed upstream: in ULIX on 2026-08-20 local `main`
/// was 42 commits behind, which inflated two branches' true 9 shared files into
/// 122 and would have made this notice pure noise.
fn integration_base(dir: &Path) -> Option<String> {
    let head = git(
        dir,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty());
    let candidates: Vec<String> = head
        .into_iter()
        .chain(
            ["origin/main", "origin/master", "main", "master"]
                .iter()
                .map(|s| s.to_string()),
        )
        .collect();
    candidates
        .into_iter()
        .find(|c| git(dir, &["rev-parse", "--verify", "--quiet", c]).is_some())
}

/// Parse `git worktree list --porcelain` into (branch, is_current) pairs.
///
/// The porcelain format emits a blank-line-separated record per worktree.
/// Detached worktrees have no `branch` line and are skipped: they have no
/// branch to name in a warning.
fn worktree_branches(dir: &Path) -> Vec<String> {
    let Some(text) = git(dir, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| l.strip_prefix("branch "))
        .map(|r| r.trim().trim_start_matches("refs/heads/").to_string())
        .collect()
}

/// Build the path -> branches map for a repository.
fn compute_claims(repo_root: &Path) -> ClaimMap {
    let branches = worktree_branches(repo_root);
    // One worktree cannot contend with itself. This is the common case and it
    // costs a single git invocation.
    if branches.len() < 2 {
        return ClaimMap::new();
    }
    let Some(base) = integration_base(repo_root) else {
        return ClaimMap::new();
    };

    let mut claims: ClaimMap = HashMap::new();
    for branch in branches {
        if branch == base || base.ends_with(&format!("/{branch}")) {
            continue;
        }
        let range = format!("{base}...{branch}");
        let Some(files) = git(repo_root, &["diff", "--name-only", &range]) else {
            continue;
        };
        for path in files.lines().take(MAX_PATHS_PER_BRANCH) {
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            claims
                .entry(path.to_string())
                .or_default()
                .push(branch.clone());
        }
    }
    // Deliberately *not* filtered to paths claimed by two or more branches.
    //
    // The base branch is usually checked out in the primary worktree, so a file
    // that one feature branch modified appears exactly once in this map. That
    // branch overlap is still useful to surface: it prompts the current editor
    // to check actual ownership and integration before treating it as conflict.
    // The self-branch is removed later in `competing_branches`, which keeps
    // ordinary solo work quiet.
    claims
}

/// Locate the repository root containing `path`.
fn repo_root(path: &Path) -> Option<PathBuf> {
    let dir = if path.is_dir() { path } else { path.parent()? };
    let out = git(dir, &["rev-parse", "--show-toplevel"])?;
    let root = out.trim();
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root))
}

/// Branches other than the current one that are also modifying `path`.
fn competing_branches(path: &Path) -> Option<Vec<String>> {
    let path = std::fs::canonicalize(path).ok()?;
    let root = repo_root(&path)?;
    let relative = path
        .strip_prefix(&root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");

    let mut guard = cache().lock().ok()?;
    let fresh = guard
        .get(&root)
        .is_some_and(|e| e.computed_at.elapsed() < CACHE_TTL);
    if !fresh {
        let claims = compute_claims(&root);
        guard.insert(
            root.clone(),
            CacheEntry {
                computed_at: Instant::now(),
                claims,
            },
        );
    }
    let claims = &guard.get(&root)?.claims;
    let all = claims.get(&relative)?;

    // Exclude the branch this worktree is on: it is the writer doing the edit,
    // and naming it would read as a conflict with itself.
    let current = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let others: Vec<String> = all.iter().filter(|b| **b != current).cloned().collect();
    if others.is_empty() {
        None
    } else {
        Some(others)
    }
}

/// Notice when another worktree branch also changed a file.
///
/// Returns `None` in every ordinary case, so single-worktree repositories and
/// uncontested files see no change at all.
pub fn contested_file_notice(path: &Path) -> Option<String> {
    let others = competing_branches(path)?;
    let list = others
        .iter()
        .map(|b| format!("`{b}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "\n\nWARNING: another worktree branch also changed this file relative to the \
         integration base: {list}. This is a branch-divergence clue, not proof \
         that another agent is actively editing it. Check ownership and integration \
         before continuing. If same-file work is active, prefer one writer per file; \
         coordinate with its owner or defer the affected file. An isolated worktree \
         does not make concurrent same-file edits safe."
    ))
}

/// Append [`contested_file_notice`] to a tool result body.
pub fn append_contested_file_notice(body: &mut String, path: &Path) {
    if let Some(notice) = contested_file_notice(path) {
        body.push_str(&notice);
    }
}

#[cfg(test)]
#[path = "contested_file_notice_tests.rs"]
mod tests;
