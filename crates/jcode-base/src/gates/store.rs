use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::storage;

use super::types::{
    AnswerRecord, DeliveryAcknowledgement, GateRequest, GateResumeDirective,
    GateResumeDirectiveState, GateState,
};

pub(super) const LOCK_FILE: &str = ".gate-ledger.lock";

/// Durable result of attempting to ingest an externally delivered answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerIngestOutcome {
    Answered,
    Applied,
    Idempotent,
    Expired,
}

/// Durable result of accepting an answer and creating its resume directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordAnswerOutcome {
    Recorded { directive: GateResumeDirective },
    Idempotent { directive: GateResumeDirective },
    Expired,
}

/// Result of an explicit expiry sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpireOutcome {
    Expired,
    Unchanged,
}

/// JSON-backed gate ledger rooted at a dedicated `JCODE_HOME/gates` directory.
#[derive(Debug, Clone)]
pub struct GateStore {
    pub(super) root: PathBuf,
}

impl GateStore {
    /// Open the standard transport-neutral gate ledger under `JCODE_HOME`.
    pub fn from_jcode_home() -> Result<Self> {
        Ok(Self::at(storage::jcode_dir()?.join("gates")))
    }

    /// Open a ledger rooted at `root`. Tests should use a fresh temporary root.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Create one pending gate. Existing gate identities are never overwritten.
    pub fn create(&self, gate: GateRequest) -> Result<()> {
        gate.validate()?;
        if !matches!(gate.state, GateState::Pending) {
            bail!("new gate must begin pending")
        }
        let path = self.path_for(&gate.gate_id, &gate.binding.session_id)?;
        self.with_lock(|| {
            reject_symlink_if_exists(&path)?;
            if path.exists() {
                bail!("gate already exists")
            }
            self.save_locked(&path, &gate)
        })
    }

    /// Load the exact gate identified by both its gate and session IDs.
    pub fn load(&self, gate_id: &str, session_id: &str) -> Result<GateRequest> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| self.load_locked(&path, (gate_id, session_id)))
    }

    /// Atomically accept an exact answer and its continuation directive.
    pub fn record_answer(
        &self,
        answer: AnswerRecord,
        now: DateTime<Utc>,
    ) -> Result<RecordAnswerOutcome> {
        answer.validate()?;
        let path = self.path_for(&answer.gate_id, &answer.binding.session_id)?;
        self.with_lock(|| {
            let mut gate =
                self.load_locked(&path, (&answer.gate_id, &answer.binding.session_id))?;
            gate.validate_answer(&answer)?;

            if matches!(gate.state, GateState::Pending) && now >= gate.expires_at {
                gate.state = GateState::Expired { expired_at: now };
                self.save_locked(&path, &gate)?;
                return Ok(RecordAnswerOutcome::Expired);
            }

            match &gate.state {
                GateState::Pending => {
                    let directive = gate.resume_directive_for(&answer);
                    gate.state = GateState::Answered { answer };
                    gate.resume_directive = Some(directive.clone());
                    self.save_locked(&path, &gate)?;
                    Ok(RecordAnswerOutcome::Recorded { directive })
                }
                GateState::Answered { answer: current } if current == &answer => {
                    let directive = gate
                        .resume_directive
                        .clone()
                        .context("answered gate is missing a resume directive")?;
                    Ok(RecordAnswerOutcome::Idempotent { directive })
                }
                GateState::Applied {
                    answer: current, ..
                } if current == &answer => Ok(RecordAnswerOutcome::Idempotent {
                    directive: gate.resume_directive_for(current),
                }),
                GateState::Answered { .. }
                | GateState::Applied { .. }
                | GateState::Unanswerable { .. } => {
                    bail!("conflicting duplicate gate answer")
                }
                GateState::Expired { .. } => Ok(RecordAnswerOutcome::Expired),
                GateState::Cancelled { .. } => bail!("gate is cancelled"),
            }
        })
    }

    /// Compatibility result for answer-ingest callers that do not need a directive.
    pub fn ingest_answer(
        &self,
        answer: AnswerRecord,
        now: DateTime<Utc>,
    ) -> Result<AnswerIngestOutcome> {
        match self.record_answer(answer, now)? {
            RecordAnswerOutcome::Recorded { .. } => Ok(AnswerIngestOutcome::Answered),
            RecordAnswerOutcome::Idempotent { .. } => Ok(AnswerIngestOutcome::Idempotent),
            RecordAnswerOutcome::Expired => Ok(AnswerIngestOutcome::Expired),
        }
    }

    /// Return the unconsumed directive for this exact gate and session, if any.
    pub fn load_pending_directive(
        &self,
        gate_id: &str,
        session_id: &str,
    ) -> Result<Option<GateResumeDirective>> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let gate = self.load_locked(&path, (gate_id, session_id))?;
            Ok(match (&gate.state, gate.resume_directive) {
                (GateState::Answered { .. }, Some(directive))
                    if matches!(directive.state, GateResumeDirectiveState::Unconsumed) =>
                {
                    Some(directive)
                }
                _ => None,
            })
        })
    }

    /// Compare and swap an exact unconsumed directive to consumed. `true` means
    /// this caller made the only durable claim; `false` means it was not pending.
    pub fn consume_pending_directive(
        &self,
        gate_id: &str,
        session_id: &str,
        expected: &GateResumeDirective,
        consumed_at: DateTime<Utc>,
    ) -> Result<bool> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let mut gate = self.load_locked(&path, (gate_id, session_id))?;
            let Some(directive) = gate.resume_directive.as_mut() else {
                return Ok(false);
            };
            if !matches!(gate.state, GateState::Answered { .. })
                || directive != expected
                || !matches!(directive.state, GateResumeDirectiveState::Unconsumed)
            {
                return Ok(false);
            }
            directive.state = GateResumeDirectiveState::Consumed { consumed_at };
            self.save_locked(&path, &gate)?;
            Ok(true)
        })
    }

    /// Persist the application marker before a higher layer wakes a session.
    pub fn mark_applied(
        &self,
        gate_id: &str,
        session_id: &str,
        applied_at: DateTime<Utc>,
    ) -> Result<AnswerIngestOutcome> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let mut gate = self.load_locked(&path, (gate_id, session_id))?;
            match gate.state.clone() {
                GateState::Answered { answer } => {
                    gate.state = GateState::Applied { answer, applied_at };
                    gate.resume_directive = None;
                    self.save_locked(&path, &gate)?;
                    Ok(AnswerIngestOutcome::Applied)
                }
                GateState::Applied { .. } => Ok(AnswerIngestOutcome::Idempotent),
                _ => bail!("only an answered gate may be applied"),
            }
        })
    }

    /// Persist a delivery observation. Delivery does not affect authorization.
    pub fn record_delivery(
        &self,
        gate_id: &str,
        session_id: &str,
        delivery: DeliveryAcknowledgement,
    ) -> Result<()> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let mut gate = self.load_locked(&path, (gate_id, session_id))?;
            gate.delivery = delivery;
            gate.validate()?;
            self.save_locked(&path, &gate)
        })
    }

    /// Explicitly expire a pending gate. Non-pending states are preserved.
    pub fn expire(
        &self,
        gate_id: &str,
        session_id: &str,
        now: DateTime<Utc>,
    ) -> Result<ExpireOutcome> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let mut gate = self.load_locked(&path, (gate_id, session_id))?;
            if matches!(gate.state, GateState::Pending) && now >= gate.expires_at {
                gate.state = GateState::Expired { expired_at: now };
                self.save_locked(&path, &gate)?;
                Ok(ExpireOutcome::Expired)
            } else {
                Ok(ExpireOutcome::Unchanged)
            }
        })
    }

    /// Cancel a pending gate before an answer is accepted.
    pub fn cancel(
        &self,
        gate_id: &str,
        session_id: &str,
        reason: &str,
        cancelled_at: DateTime<Utc>,
    ) -> Result<()> {
        validate_reason(reason)?;
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            let mut gate = self.load_locked(&path, (gate_id, session_id))?;
            if !matches!(gate.state, GateState::Pending) {
                bail!("only a pending gate may be cancelled")
            }
            gate.state = GateState::Cancelled {
                cancelled_at,
                reason: reason.to_string(),
            };
            self.save_locked(&path, &gate)
        })
    }

    /// Return the durable path for a validated key. Exposed for filesystem
    /// integrity tests, not as a transport integration API.
    pub fn path_for(&self, gate_id: &str, session_id: &str) -> Result<PathBuf> {
        validate_path_identifier("gate", gate_id)?;
        validate_path_identifier("session", session_id)?;
        Ok(self.root.join(session_id).join(format!("{gate_id}.json")))
    }

    pub(super) fn with_lock<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        reject_symlink_if_exists(&self.root)?;
        storage::ensure_dir(&self.root)?;
        let lock_path = self.root.join(LOCK_FILE);
        reject_symlink_if_exists(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&lock_path)
            .context("open gate ledger lock")?;
        lock.lock().context("lock gate ledger")?;
        let _lock = GateStoreLock { _file: lock };
        action()
    }

    pub(super) fn load_locked(&self, path: &Path, key: (&str, &str)) -> Result<GateRequest> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("gate path has no parent"))?;
        reject_symlink_if_exists(parent)?;
        reject_symlink_if_exists(path)?;
        let gate: GateRequest = storage::read_json(path).context("read durable gate")?;
        gate.validate().context("validate durable gate")?;
        if gate.gate_id != key.0 || gate.binding.session_id != key.1 {
            bail!("durable gate identity does not match its storage key")
        }
        Ok(gate)
    }

    pub(super) fn save_locked(&self, path: &Path, gate: &GateRequest) -> Result<()> {
        gate.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("gate path has no parent"))?;
        reject_symlink_if_exists(parent)?;
        reject_symlink_if_exists(path)?;
        storage::write_json(path, gate).context("atomically persist durable gate")
    }
}

struct GateStoreLock {
    _file: File,
}

pub(super) fn validate_path_identifier(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("invalid {kind} identifier")
    }
    Ok(())
}

fn validate_reason(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 12_000 || value.chars().any(char::is_control) {
        bail!("invalid cancellation reason")
    }
    Ok(())
}

fn reject_symlink_if_exists(path: &Path) -> Result<()> {
    if path.exists() && std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("refusing symlinked gate ledger path")
    }
    Ok(())
}
