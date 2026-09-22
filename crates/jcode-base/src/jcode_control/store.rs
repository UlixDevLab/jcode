use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::storage;

use super::request::validate_pinned_action;
use super::{
    AuthenticatedPrincipal, ControlAction, ControlExecutionResult, ControlReceipt,
    ControlReceiptState, ControlRequest, PinnedCreateSessionResolution, ReceiptKey,
};

const LOCK_FILE: &str = ".jcode-control-receipts.lock";

/// Result of storing a received request without allowing execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Received(ControlReceipt),
    Replay(ControlReceipt),
}

/// Result of atomically claiming a request for a future execution layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    Claimed(ControlReceipt),
    Replay(ControlReceipt),
}

/// JSON-backed idempotency receipts rooted in one dedicated JCODE_HOME child.
#[derive(Debug, Clone)]
pub struct ControlReceiptStore {
    root: PathBuf,
}

impl ControlReceiptStore {
    /// Open the standard local receipt authority at `JCODE_HOME/jcode-control/v1`.
    pub fn from_jcode_home() -> Result<Self> {
        Self::at(storage::jcode_dir()?.join("jcode-control").join("v1"))
    }

    /// Open a test or alternate receipt root. The root is checked before every
    /// mutation so a later symlink substitution fails closed.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self { root: root.into() })
    }

    /// Persist a received request, or return its exact prior receipt. This
    /// method never claims execution and never calls into sessions or tools.
    pub fn receive(
        &self,
        principal: &AuthenticatedPrincipal,
        request: &ControlRequest,
    ) -> Result<ReceiveOutcome> {
        request.validate()?;
        let key = ReceiptKey::new(principal.clone(), request.idempotency_key.clone())?;
        let request_fingerprint = canonical_fingerprint(principal, request, None)?;
        let path = self.path_for(&key)?;
        self.with_lock(|| {
            if path.exists() {
                let receipt = self.load_locked(&path, &key)?;
                self.require_exact_replay(&receipt, request, &request_fingerprint)?;
                return Ok(ReceiveOutcome::Replay(receipt));
            }
            let receipt =
                ControlReceipt::received(principal.clone(), request.clone(), request_fingerprint)?;
            self.save_locked(&path, &receipt)?;
            Ok(ReceiveOutcome::Received(receipt))
        })
    }

    /// Atomically move an exact request from `received` to `claimed`. Exactly
    /// one concurrent caller receives `Claimed`; all exact replays receive the
    /// same durable record. No session is created and no message is accepted.
    pub fn claim(
        &self,
        principal: &AuthenticatedPrincipal,
        request: &ControlRequest,
        pinned_create_session: Option<PinnedCreateSessionResolution>,
    ) -> Result<ClaimOutcome> {
        request.validate()?;
        validate_pinned_action(&request.action, pinned_create_session.as_ref())?;
        let key = ReceiptKey::new(principal.clone(), request.idempotency_key.clone())?;
        let request_fingerprint = canonical_fingerprint(principal, request, None)?;
        let execution_fingerprint =
            canonical_fingerprint(principal, request, pinned_create_session.as_ref())?;
        let path = self.path_for(&key)?;
        self.with_lock(|| {
            let mut receipt = if path.exists() {
                let receipt = self.load_locked(&path, &key)?;
                self.require_exact_replay(&receipt, request, &request_fingerprint)?;
                receipt
            } else {
                ControlReceipt::received(
                    principal.clone(),
                    request.clone(),
                    request_fingerprint.clone(),
                )?
            };

            match &receipt.state {
                ControlReceiptState::Received => {
                    receipt.execution_fingerprint = Some(execution_fingerprint);
                    receipt.state = ControlReceiptState::Claimed {
                        pinned_create_session,
                    };
                    self.save_locked(&path, &receipt)?;
                    Ok(ClaimOutcome::Claimed(receipt))
                }
                ControlReceiptState::Claimed { .. }
                | ControlReceiptState::Completed { .. }
                | ControlReceiptState::Failed { .. } => {
                    if receipt.execution_fingerprint.as_deref() != Some(&execution_fingerprint) {
                        bail!("conflicting pinned resolution for idempotency replay")
                    }
                    Ok(ClaimOutcome::Replay(receipt))
                }
            }
        })
    }

    /// Persist a trusted higher-layer success result after a local claim. The
    /// stored terminal result is replayed only when exactly equal.
    pub fn complete(
        &self,
        key: &ReceiptKey,
        result: ControlExecutionResult,
    ) -> Result<ControlReceipt> {
        result.validate()?;
        self.finish(key, result, true)
    }

    /// Persist a trusted higher-layer failure result after a local claim. The
    /// stored terminal result is replayed only when exactly equal.
    pub fn fail(&self, key: &ReceiptKey, result: ControlExecutionResult) -> Result<ControlReceipt> {
        result.validate()?;
        self.finish(key, result, false)
    }

    /// Load one receipt using only the authenticated `(principal, idempotency_key)` key.
    pub fn load(&self, key: &ReceiptKey) -> Result<ControlReceipt> {
        let path = self.path_for(key)?;
        self.with_lock(|| self.load_locked(&path, key))
    }

    /// Exposed for focused filesystem integrity tests, not a transport API.
    pub fn path_for(&self, key: &ReceiptKey) -> Result<PathBuf> {
        Ok(self
            .root
            .join(key.principal().as_str())
            .join(format!("{}.json", key.idempotency_key())))
    }

    fn finish(
        &self,
        key: &ReceiptKey,
        result: ControlExecutionResult,
        completed: bool,
    ) -> Result<ControlReceipt> {
        let path = self.path_for(key)?;
        self.with_lock(|| {
            let mut receipt = self.load_locked(&path, key)?;
            let pinned_create_session = receipt.pinned_create_session().cloned();
            match &receipt.state {
                ControlReceiptState::Claimed { .. } => {
                    receipt.state = if completed {
                        ControlReceiptState::Completed {
                            pinned_create_session,
                            result,
                        }
                    } else {
                        ControlReceiptState::Failed {
                            pinned_create_session,
                            result,
                        }
                    };
                    self.save_locked(&path, &receipt)?;
                    Ok(receipt)
                }
                ControlReceiptState::Completed {
                    result: existing, ..
                } if completed && existing == &result => Ok(receipt),
                ControlReceiptState::Failed {
                    result: existing, ..
                } if !completed && existing == &result => Ok(receipt),
                ControlReceiptState::Received => {
                    bail!("control request must be claimed before recording a result")
                }
                ControlReceiptState::Completed { .. } | ControlReceiptState::Failed { .. } => {
                    bail!("conflicting terminal control result")
                }
            }
        })
    }

    fn require_exact_replay(
        &self,
        receipt: &ControlReceipt,
        request: &ControlRequest,
        request_fingerprint: &str,
    ) -> Result<()> {
        if receipt.request != *request || receipt.request_fingerprint != request_fingerprint {
            bail!("conflicting idempotency replay")
        }
        Ok(())
    }

    fn with_lock<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        reject_symlink_if_exists(&self.root)?;
        storage::ensure_dir(&self.root).context("create control receipt root")?;
        reject_symlink_if_exists(&self.root)?;
        let lock_path = self.root.join(LOCK_FILE);
        reject_symlink_if_exists(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&lock_path)
            .context("open control receipt lock")?;
        lock.lock().context("lock control receipts")?;
        let _lock = ControlReceiptStoreLock { _file: lock };
        action()
    }

    fn load_locked(&self, path: &Path, key: &ReceiptKey) -> Result<ControlReceipt> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("control receipt path has no parent"))?;
        reject_symlink_if_exists(&self.root)?;
        reject_symlink_if_exists(parent)?;
        reject_symlink_if_exists(path)?;
        let receipt: ControlReceipt =
            storage::read_json(path).context("read durable control receipt")?;
        receipt
            .validate()
            .context("validate durable control receipt")?;
        if receipt.principal != *key.principal() || receipt.idempotency_key != key.idempotency_key()
        {
            bail!("durable control receipt identity does not match its storage key")
        }
        let request_fingerprint =
            canonical_fingerprint(&receipt.principal, &receipt.request, None)?;
        if receipt.request_fingerprint != request_fingerprint {
            bail!("durable control receipt request fingerprint does not match record")
        }
        if let Some(execution_fingerprint) = &receipt.execution_fingerprint {
            let expected = canonical_fingerprint(
                &receipt.principal,
                &receipt.request,
                receipt.pinned_create_session(),
            )?;
            if execution_fingerprint != &expected {
                bail!("durable control receipt execution fingerprint does not match record")
            }
        }
        Ok(receipt)
    }

    fn save_locked(&self, path: &Path, receipt: &ControlReceipt) -> Result<()> {
        receipt.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("control receipt path has no parent"))?;
        reject_symlink_if_exists(&self.root)?;
        storage::ensure_dir(parent).context("create control receipt principal directory")?;
        reject_symlink_if_exists(parent)?;
        reject_symlink_if_exists(path)?;
        storage::write_json(path, receipt).context("atomically fsync durable control receipt")
    }
}

struct ControlReceiptStoreLock {
    _file: File,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct CanonicalFingerprint<'a> {
    schema: &'a str,
    principal: &'a str,
    idempotency_key: &'a str,
    action: &'a ControlAction,
    payload: &'a ControlRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pinned_create_session: Option<&'a PinnedCreateSessionResolution>,
}

fn canonical_fingerprint(
    principal: &AuthenticatedPrincipal,
    request: &ControlRequest,
    pinned_create_session: Option<&PinnedCreateSessionResolution>,
) -> Result<String> {
    let canonical = CanonicalFingerprint {
        schema: &request.schema,
        principal: principal.as_str(),
        idempotency_key: &request.idempotency_key,
        action: &request.action,
        payload: request,
        pinned_create_session,
    };
    let bytes =
        serde_json::to_vec(&canonical).context("serialize canonical control fingerprint")?;
    let hash = Sha256::digest(bytes);
    Ok(hex::encode(hash))
}

fn reject_symlink_if_exists(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!("refusing symlinked control receipt path")
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
