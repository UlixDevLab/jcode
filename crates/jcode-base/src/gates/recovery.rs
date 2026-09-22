//! Safe, read-only enumeration of durable directives after a daemon restart.
//!
//! Recovery is deliberately limited to validated `Answered` records whose
//! directives remain unconsumed. It neither claims directives nor changes gate
//! records, leaving the eventual lifecycle consumer as the only continuation
//! authority.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use super::store::{LOCK_FILE, validate_path_identifier};
use super::{GateResumeDirective, GateResumeDirectiveState, GateState, GateStore};

/// One validated directive awaiting a future lifecycle consumer.
///
/// Results from [`GateStore::load_unconsumed_directives`] are sorted first by
/// `session_id`, then by `gate_id`, both in ascending bytewise string order.
/// The record exposes only durable identifiers and the already-bounded
/// directive, never the complete gate question or transport metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnconsumedGateDirective {
    pub gate_id: String,
    pub session_id: String,
    pub directive: GateResumeDirective,
}

impl GateStore {
    /// Enumerate validated, unconsumed directives without claiming or mutating
    /// any record. Corrupt, unknown-schema, unexpected, or symlinked entries
    /// fail the complete scan closed using the ledger's existing `Result` error
    /// convention.
    pub fn load_unconsumed_directives(&self) -> Result<Vec<UnconsumedGateDirective>> {
        self.with_lock(|| collect_unconsumed_directives(self, &self.root))
    }
}

fn collect_unconsumed_directives(
    store: &GateStore,
    root: &Path,
) -> Result<Vec<UnconsumedGateDirective>> {
    let mut directives = Vec::new();
    for session_path in read_entries(root)? {
        let session_name = checked_entry_name(&session_path, "session")?;
        if session_name == LOCK_FILE {
            continue;
        }
        reject_symlink(&session_path)?;
        if !fs::symlink_metadata(&session_path)?.file_type().is_dir() {
            bail!("unexpected entry in gate ledger root")
        }
        validate_path_identifier("session", &session_name)?;

        for record_path in read_entries(&session_path)? {
            let record_name = checked_entry_name(&record_path, "record")?;
            reject_symlink(&record_path)?;
            if !fs::symlink_metadata(&record_path)?.file_type().is_file() {
                bail!("unexpected entry in durable gate session")
            }
            // `storage::write_json` retains the previous primary inode as a
            // regular `<gate>.bak` file. It is non-authoritative recovery
            // history, never a second directive record. Require its primary
            // counterpart so an unexpected artifact cannot be hidden here.
            if let Some(backup_gate_id) = record_name.strip_suffix(".bak") {
                validate_path_identifier("gate", backup_gate_id)?;
                let primary = session_path.join(format!("{backup_gate_id}.json"));
                reject_symlink(&primary)?;
                if !fs::symlink_metadata(&primary)?.file_type().is_file() {
                    bail!("durable gate backup has no primary record")
                }
                continue;
            }
            let gate_id = record_name
                .strip_suffix(".json")
                .context("invalid durable gate record filename")?;
            validate_path_identifier("gate", gate_id)?;
            let gate = store.load_locked(&record_path, (gate_id, &session_name))?;
            if let (GateState::Answered { .. }, Some(directive)) =
                (&gate.state, gate.resume_directive)
                && matches!(directive.state, GateResumeDirectiveState::Unconsumed)
            {
                directives.push(UnconsumedGateDirective {
                    gate_id: gate.gate_id,
                    session_id: gate.binding.session_id,
                    directive,
                });
            }
        }
    }
    directives.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| left.gate_id.cmp(&right.gate_id))
    });
    Ok(directives)
}

pub(super) fn read_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read durable gate directory {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

pub(super) fn checked_entry_name(path: &Path, kind: &str) -> Result<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .with_context(|| format!("invalid {kind} entry in durable gate ledger"))
}

pub(super) fn reject_symlink(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("refusing symlinked gate ledger path")
    }
    Ok(())
}
