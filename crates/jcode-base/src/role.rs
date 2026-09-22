//! Role discovery.
//!
//! A *role* is a short Markdown file describing how to behave in a particular
//! kind of work: `devops.md`, `frontend.md`, `verify.md`. Roles are the
//! project-agnostic half of agent behavior. They pair with per-project facts
//! (`AGENTS.md`, an ops manifest) which stay in the repository.
//!
//! Discovery mirrors [`crate::skill`]: global roles live in `~/.jcode/roles/`
//! and a project may override or add roles in `./.jcode/roles/`. A project role
//! with the same name wins, so a repository can specialize `frontend` without
//! copying the global file.
//!
//! Roles are deliberately *lazy*. Only the name and one-line summary of each
//! role reaches the prompt; the body is read on demand with the normal file
//! tools. Loading every role body would reintroduce exactly the always-on
//! prompt tax this design avoids, and most turns need none of them.

use std::path::{Path, PathBuf};

/// A discovered role file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleInfo {
    /// File stem, e.g. `devops` for `devops.md`.
    pub name: String,
    /// First meaningful line of the body, used as the catalog summary.
    pub summary: String,
    /// Absolute path to the role file, so the agent can read it when relevant.
    pub path: PathBuf,
    /// True when a project role shadowed a global role of the same name.
    pub overrides_global: bool,
}

/// Longest summary retained per role. Roles are advertised, not inlined, so the
/// catalog stays small even with many roles installed.
const MAX_SUMMARY_CHARS: usize = 160;

/// Upper bound on role files read from one directory. A malformed or generated
/// directory should not be able to inflate the prompt without bound.
const MAX_ROLES_PER_DIR: usize = 64;

/// Extract a one-line summary: the first non-empty line that is not a heading,
/// front-matter fence, or blockquote. Falls back to the first heading text so a
/// role that is only a title still describes itself.
fn summarize(body: &str) -> String {
    let mut heading_fallback: Option<String> = None;
    let mut in_front_matter = false;

    for (idx, raw) in body.lines().enumerate() {
        let line = raw.trim();
        if line == "---" {
            // Only treat a leading `---` as front matter, not a horizontal rule
            // further down the file.
            if idx == 0 {
                in_front_matter = true;
                continue;
            }
            if in_front_matter {
                in_front_matter = false;
                continue;
            }
        }
        if in_front_matter || line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if heading_fallback.is_none() {
                let text = rest.trim_start_matches('#').trim();
                if !text.is_empty() {
                    heading_fallback = Some(text.to_string());
                }
            }
            continue;
        }
        if line.starts_with('>') || line.starts_with("```") {
            continue;
        }
        return truncate_summary(line);
    }

    heading_fallback
        .map(|h| truncate_summary(&h))
        .unwrap_or_default()
}

fn truncate_summary(text: &str) -> String {
    if text.chars().count() <= MAX_SUMMARY_CHARS {
        return text.to_string();
    }
    let clipped: String = text
        .chars()
        .take(MAX_SUMMARY_CHARS.saturating_sub(1))
        .collect();
    format!("{}…", clipped.trim_end())
}

/// Read every `*.md` role in one directory, sorted by name for stable prompts.
///
/// Ordering matters beyond tidiness: the role catalog sits in the cacheable
/// prompt prefix, so a nondeterministic directory order would change the prefix
/// between turns and cost a prompt-cache miss each time.
fn load_dir(dir: &Path) -> Vec<RoleInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut roles: Vec<RoleInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "md" {
                return None;
            }
            let name = path.file_stem()?.to_str()?.to_string();
            if name.is_empty() {
                return None;
            }
            let body = std::fs::read_to_string(&path).ok()?;
            Some(RoleInfo {
                name,
                summary: summarize(&body),
                path,
                overrides_global: false,
            })
        })
        .collect();

    roles.sort_by(|a, b| a.name.cmp(&b.name));
    roles.truncate(MAX_ROLES_PER_DIR);
    roles
}

/// Global role directory: `~/.jcode/roles/`.
pub fn global_roles_dir() -> Option<PathBuf> {
    crate::storage::jcode_dir()
        .ok()
        .map(|dir| dir.join("roles"))
}

/// Project role directory for `working_dir`: `<working_dir>/.jcode/roles/`.
pub fn project_roles_dir(working_dir: &Path) -> PathBuf {
    working_dir.join(".jcode").join("roles")
}

/// Discover roles for a session: global roles overlaid with project roles.
///
/// `working_dir` must be the session's project directory rather than the
/// process cwd; a shared daemon serves many projects from one process, so
/// resolving against the cwd would attribute one project's roles to another.
pub fn discover(working_dir: Option<&Path>) -> Vec<RoleInfo> {
    let mut roles = global_roles_dir()
        .map(|dir| load_dir(&dir))
        .unwrap_or_default();

    if let Some(working_dir) = working_dir {
        for project_role in load_dir(&project_roles_dir(working_dir)) {
            match roles.iter_mut().find(|role| role.name == project_role.name) {
                Some(existing) => {
                    *existing = RoleInfo {
                        overrides_global: true,
                        ..project_role
                    };
                }
                None => roles.push(project_role),
            }
        }
        roles.sort_by(|a, b| a.name.cmp(&b.name));
    }

    roles
}

/// Render the prompt catalog for discovered roles, or `None` when no roles
/// exist so a project without roles pays nothing.
///
/// Only names, summaries, and paths are emitted. The agent reads a body when a
/// task actually calls for that role.
pub fn build_roles_prompt(roles: &[RoleInfo]) -> Option<String> {
    let section = render_roles_prompt(roles)?;
    note_catalog_change(&section);
    Some(section)
}

/// Fingerprint of the role catalog most recently rendered into a prompt,
/// per project. Adding, removing, or editing the first prose line of a role
/// changes the static system prompt, which legitimately invalidates a warm KV
/// cache prefix.
///
/// Skills and config reloads already document their invalidations so the TUI
/// can attribute the resend instead of raising an unexplained "harness bust"
/// alarm. Roles are discovered from disk on every prompt build rather than
/// through an explicit reload, so there is no single call site to document.
/// Comparing the rendered catalog against the last one for the same project is
/// the equivalent signal.
///
/// Keyed by project because one daemon serves many working directories: a
/// global-only fingerprint would flip on every alternating session and report a
/// change that never happened.
static LAST_CATALOG: std::sync::Mutex<Option<std::collections::HashMap<u64, u64>>> =
    std::sync::Mutex::new(None);

fn note_catalog_change(section: &str) {
    // Cheap FNV-1a: this runs per prompt build, and the catalog is small.
    let hash = fnv1a(section.as_bytes());

    // The key must identify the *project* and survive every change we are
    // watching for. Keying on summaries misses a summary edit; keying on the
    // whole path set misses an added or removed role. Both would file the new
    // catalog under a fresh key and report nothing.
    //
    // The directory holding the roles is stable under both: adding
    // `~/.jcode/roles/lab.md` or editing a summary leaves `~/.jcode/roles` and
    // any project's `.jcode/roles` unchanged.
    let mut dirs: Vec<&str> = Vec::new();
    for line in section.lines() {
        if !line.starts_with("- `") {
            continue;
        }
        let Some(open) = line.rfind(" (") else {
            continue;
        };
        let rest = &line[open + 2..];
        let path = rest
            .split_once(", project override")
            .map(|(path, _)| path)
            .unwrap_or_else(|| rest.trim_end_matches(')'));
        if let Some(slash) = path.rfind('/')
            && !dirs.contains(&&path[..slash])
        {
            dirs.push(&path[..slash]);
        }
    }
    dirs.sort_unstable();
    let key = fnv1a(dirs.join("\n").as_bytes());

    let Ok(mut guard) = LAST_CATALOG.lock() else {
        return;
    };
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    match map.insert(key, hash) {
        Some(previous) if previous != hash => {
            crate::cache_invalidation::record(
                "role change",
                "the role catalog in the system prompt changed (a role file was added, \
                 removed, or its summary line edited)",
            );
        }
        _ => {}
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn render_roles_prompt(roles: &[RoleInfo]) -> Option<String> {
    if roles.is_empty() {
        return None;
    }

    let mut section = String::from(
        "# Available Roles\n\nRole files describe how to approach a kind of work. \
         When a task matches one, read the file first and follow it. \
         Project facts still come from the repository, not from these files.\n",
    );
    for role in roles {
        section.push('\n');
        section.push_str("- `");
        section.push_str(&role.name);
        section.push('`');
        if !role.summary.is_empty() {
            section.push_str(" - ");
            section.push_str(&role.summary);
        }
        section.push_str(" (");
        section.push_str(&role.path.to_string_lossy());
        if role.overrides_global {
            section.push_str(", project override");
        }
        section.push(')');
    }
    Some(section)
}

#[cfg(test)]
#[path = "role_tests.rs"]
mod role_tests;
