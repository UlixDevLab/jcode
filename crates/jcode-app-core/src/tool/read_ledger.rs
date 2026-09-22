//! Session-scoped ledger of what `read` has already returned.
//!
//! Why this exists: a usage audit over 475 recorded sessions
//! (`docs/audits/jcode-usage-analysis.md`) found that repeated inspection of a
//! file the agent had *already* read was the single largest category of
//! avoidable repeated work: 94 of 101 measured target repeats were `read`, and
//! one session read `adaptive_scalp_router.py` 31 times. An earlier audit of
//! the operator's other agent framework found the same failure shape
//! independently (511 duplicate calls across 18 of 20 sessions).
//!
//! Deliberate design limits, taken from what that evidence does and does not
//! support:
//!
//! * **Soft, not authoritative.** The file content is always returned. The
//!   ledger only appends a notice. P90 repeats are zero, so most sessions never
//!   see this, and a mandatory cache would be unsupported by the data.
//! * **Only a genuinely redundant read is flagged.** A repeat counts only when
//!   the same byte range of the same path is re-read *and* the file's mtime and
//!   length are unchanged. Paging through a large file, or re-reading a file
//!   after editing it, is normal work and stays silent.
//! * **Session-scoped.** Entries are keyed by session so one session's history
//!   can never leak advice into another, and are dropped when a session ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

/// Identity of a file version, used to tell "already read this" apart from
/// "read this again because it changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileVersion {
    pub len: u64,
    /// Modification time as nanoseconds since the epoch. `None` when the
    /// platform or filesystem does not report one, in which case a repeat is
    /// never claimed, because we cannot prove the file is unchanged.
    pub mtime_nanos: Option<u128>,
}

impl FileVersion {
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        let mtime_nanos = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|delta| delta.as_nanos());
        Self {
            len: metadata.len(),
            mtime_nanos,
        }
    }

    /// Whether two observations are provably the same file version.
    fn is_same_version_as(&self, other: &Self) -> bool {
        match (self.mtime_nanos, other.mtime_nanos) {
            (Some(a), Some(b)) => a == b && self.len == other.len,
            // Without a modification time we cannot prove the content is
            // unchanged, so we decline to call it a repeat.
            _ => false,
        }
    }
}

/// One previously returned read of a specific line range.
#[derive(Debug, Clone, Copy)]
struct ReadRecord {
    offset: usize,
    end: usize,
    version: FileVersion,
    /// How many times this exact range has been returned, including the first.
    count: usize,
}

type SessionLedger = HashMap<PathBuf, Vec<ReadRecord>>;

static LEDGER: LazyLock<Mutex<HashMap<String, SessionLedger>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cap per session so a very long session cannot grow the ledger without
/// bound. Chosen well above the observed P90 of zero repeats and above the
/// worst measured session, so it never truncates realistic work.
const MAX_TRACKED_PATHS_PER_SESSION: usize = 512;
/// Cap distinct ranges tracked per file, so paging through a huge file in many
/// small windows cannot grow without bound either.
const MAX_RANGES_PER_PATH: usize = 32;

/// Record that a read happened, and report how many times this same range of
/// this same file version has now been returned in this session.
///
/// Returns the repeat count: 1 for a first read, 2 for the first repeat, and
/// so on. Callers should only surface a notice when the count exceeds 1.
pub fn record_read(
    session_id: &str,
    path: &Path,
    offset: usize,
    end: usize,
    version: FileVersion,
) -> usize {
    let Ok(mut ledger) = LEDGER.lock() else {
        // A poisoned lock must never break a read. Degrade to "no advice".
        return 1;
    };
    let session = ledger.entry(session_id.to_string()).or_default();

    // Drop the ledger for this session rather than tracking unbounded paths.
    // Advice is a convenience; correctness of the read never depends on it.
    if session.len() >= MAX_TRACKED_PATHS_PER_SESSION && !session.contains_key(path) {
        session.clear();
    }

    let records = session.entry(path.to_path_buf()).or_default();

    // A changed file invalidates everything previously recorded for that path:
    // prior ranges describe content that no longer exists.
    if let Some(existing) = records.first() {
        if !existing.version.is_same_version_as(&version) {
            records.clear();
        }
    }

    if let Some(record) = records
        .iter_mut()
        .find(|record| record.offset == offset && record.end == end)
    {
        record.count += 1;
        return record.count;
    }

    if records.len() >= MAX_RANGES_PER_PATH {
        records.clear();
    }
    records.push(ReadRecord {
        offset,
        end,
        version,
        count: 1,
    });
    1
}

/// Forget everything recorded for a session. Called when a session ends so the
/// process-global map does not accumulate dead sessions.
pub fn forget_session(session_id: &str) {
    if let Ok(mut ledger) = LEDGER.lock() {
        ledger.remove(session_id);
    }
}

/// The notice appended to a redundant read, or `None` when the read was not
/// redundant.
///
/// The wording states the fact and leaves the decision to the caller: the
/// evidence supports making repetition visible, not forbidding it.
pub fn repeat_notice(repeat_count: usize, display_path: &str) -> Option<String> {
    if repeat_count <= 1 {
        return None;
    }
    Some(format!(
        "\n[note] You have already read this exact range of {display_path} \
         {repeat_count} times in this session, and the file has not changed \
         since. The earlier output is still in your context. Re-read only if \
         you need it again; otherwise use what you already have.\n"
    ))
}

#[cfg(test)]
mod tests;
