use super::model::{BatchGrantManifest, GRANT_SCHEMA_VERSION, GrantMatcher, GrantSummary};
use crate::tool::consent::NativeBatchGrantApproval;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::Arc;

const GRANTS_FILE: &str = "grants-v1.json";
const AUDIT_FILE: &str = "audit-v1.jsonl";
const LOCK_FILE: &str = ".grants-v1.lock";

#[derive(Clone)]
pub(crate) struct BatchGrantStore {
    root: PathBuf,
    #[cfg(test)]
    _test_root: Option<Arc<tempfile::TempDir>>,
}

#[derive(Serialize, Deserialize)]
struct GrantDocument {
    schema_version: u32,
    grants: Vec<StoredGrant>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredGrant {
    id: String,
    session_id: String,
    manifest: BatchGrantManifest,
    remaining_uses: u32,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct AuditEvent<'a> {
    schema_version: u32,
    at: DateTime<Utc>,
    event: &'a str,
    grant_id: &'a str,
    manifest_digest: &'a str,
    remaining_uses: u32,
}

impl BatchGrantStore {
    #[cfg(test)]
    pub(crate) fn for_test() -> Result<Self> {
        let test_root = Arc::new(tempfile::TempDir::new()?);
        Ok(Self {
            root: test_root.path().join("consent"),
            _test_root: Some(test_root),
        })
    }

    pub(crate) fn local() -> Result<Self> {
        Ok(Self {
            root: crate::storage::jcode_dir()?.join("consent"),
            #[cfg(test)]
            _test_root: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn reopen(&self) -> Result<Self> {
        Ok(self.clone())
    }

    #[cfg(test)]
    pub(crate) fn audit_text(&self) -> Result<String> {
        Ok(std::fs::read_to_string(self.root.join(AUDIT_FILE)).unwrap_or_default())
    }

    #[cfg(test)]
    pub(crate) fn storage_paths(&self) -> (PathBuf, PathBuf) {
        (self.root.join(GRANTS_FILE), self.root.join(AUDIT_FILE))
    }

    pub(crate) fn create_after_native_approval(
        &self,
        _approval: NativeBatchGrantApproval,
        session_id: &str,
        manifest: BatchGrantManifest,
    ) -> Result<GrantSummary> {
        manifest.validate()?;
        if manifest.expires_at <= Utc::now() {
            bail!("cannot create an already expired batch grant");
        }
        validate_session_id(session_id)?;
        self.with_document(|document| {
            let grant = StoredGrant {
                id: crate::id::new_id("grant"),
                session_id: session_id.to_string(),
                remaining_uses: manifest.maximum_uses,
                manifest,
                revoked_at: None,
            };
            let summary = summary(&grant);
            document.grants.push(grant.clone());
            Ok((summary, Some(("created", grant))))
        })
    }

    pub(crate) fn list(&self, session_id: &str) -> Result<Vec<GrantSummary>> {
        validate_session_id(session_id)?;
        self.with_document(|document| {
            let mut grants = document
                .grants
                .iter()
                .filter(|grant| grant.session_id == session_id)
                .map(summary)
                .collect::<Vec<_>>();
            grants.sort_by(|left, right| left.id.cmp(&right.id));
            Ok((grants, None))
        })
    }

    pub(crate) fn revoke(
        &self,
        session_id: &str,
        grant_id: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        validate_session_id(session_id)?;
        self.with_document(|document| {
            let grant = document
                .grants
                .iter_mut()
                .find(|grant| grant.id == grant_id && grant.session_id == session_id)
                .ok_or_else(|| anyhow::anyhow!("grant is not available in this session"))?;
            if grant.revoked_at.is_some() {
                bail!("grant is already revoked");
            }
            grant.revoked_at = Some(now);
            Ok(((), Some(("revoked", grant.clone()))))
        })
    }

    pub(crate) fn consume(
        &self,
        session_id: &str,
        matcher: &GrantMatcher,
        now: DateTime<Utc>,
    ) -> Result<()> {
        validate_session_id(session_id)?;
        let outcome = self.with_document(|document| {
            let grant = document
                .grants
                .iter_mut()
                .find(|grant| {
                    grant.session_id == session_id
                        && grant.revoked_at.is_none()
                        && matcher.matches(&grant.manifest)
                })
                .ok_or_else(|| anyhow::anyhow!("no exact active grant is available"))?;
            if now >= grant.manifest.expires_at {
                return Ok((
                    ConsumeOutcome::Expired,
                    Some(("expired_denied", grant.clone())),
                ));
            }
            if grant.remaining_uses == 0 {
                return Ok((ConsumeOutcome::Exhausted, None));
            }
            grant.remaining_uses -= 1;
            Ok((ConsumeOutcome::Used, Some(("used", grant.clone()))))
        })?;
        match outcome {
            ConsumeOutcome::Used => Ok(()),
            ConsumeOutcome::Expired => bail!("grant is expired"),
            ConsumeOutcome::Exhausted => bail!("grant count is exhausted"),
        }
    }

    fn with_document<T>(
        &self,
        operation: impl FnOnce(&mut GrantDocument) -> Result<(T, Option<(&'static str, StoredGrant)>)>,
    ) -> Result<T> {
        self.ensure_private_root()?;
        let lock_path = self.root.join(LOCK_FILE);
        reject_symlink_if_exists(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(lock_path)?;
        set_owner_only(&self.root.join(LOCK_FILE), false)?;
        lock_exclusive(&lock)?;
        let mut document = self.load_document()?;
        let (value, audit) = operation(&mut document)?;
        if let Some((event, grant)) = audit {
            self.save_document(&document)?;
            self.append_audit(event, &grant)?;
        }
        Ok(value)
    }

    fn ensure_private_root(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        set_owner_only(&self.root, true)
    }

    fn load_document(&self) -> Result<GrantDocument> {
        let path = self.root.join(GRANTS_FILE);
        if !path.exists() {
            return Ok(GrantDocument {
                schema_version: GRANT_SCHEMA_VERSION,
                grants: Vec::new(),
            });
        }
        reject_symlink(&path)?;
        let document: GrantDocument = serde_json::from_slice(&std::fs::read(&path)?)
            .context("invalid persisted consent grant document")?;
        if document.schema_version != GRANT_SCHEMA_VERSION {
            bail!("unsupported persisted consent grant schema version");
        }
        for grant in &document.grants {
            validate_session_id(&grant.session_id)?;
            grant.manifest.validate()?;
            if grant.remaining_uses > grant.manifest.maximum_uses {
                bail!("persisted grant remaining count exceeds its cap");
            }
        }
        Ok(document)
    }

    fn save_document(&self, document: &GrantDocument) -> Result<()> {
        let path = self.root.join(GRANTS_FILE);
        let temp = self
            .root
            .join(format!(".{GRANTS_FILE}.{}.tmp", crate::id::new_id("write")));
        std::fs::write(&temp, serde_json::to_vec_pretty(document)?)?;
        set_owner_only(&temp, false)?;
        std::fs::rename(&temp, path)?;
        Ok(())
    }

    fn append_audit(&self, event: &'static str, grant: &StoredGrant) -> Result<()> {
        let path = self.root.join(AUDIT_FILE);
        reject_symlink_if_exists(&path)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        set_owner_only(&path, false)?;
        let event = AuditEvent {
            schema_version: GRANT_SCHEMA_VERSION,
            at: Utc::now(),
            event,
            grant_id: &grant.id,
            manifest_digest: &grant.manifest.digest,
            remaining_uses: grant.remaining_uses,
        };
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
        file.sync_data()?;
        Ok(())
    }
}

enum ConsumeOutcome {
    Used,
    Expired,
    Exhausted,
}

fn summary(grant: &StoredGrant) -> GrantSummary {
    GrantSummary {
        id: grant.id.clone(),
        provider: grant.manifest.provider.clone(),
        actions: grant.manifest.actions.clone(),
        resource_count: grant.manifest.resource_ids.len(),
        remaining_uses: grant.remaining_uses,
        expires_at: grant.manifest.expires_at,
        digest: grant.manifest.digest.clone(),
        revoked: grant.revoked_at.is_some(),
    }
}

fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("invalid grant session identifier");
    }
    Ok(())
}

fn lock_exclusive(file: &File) -> Result<()> {
    #[cfg(unix)]
    unsafe {
        use std::os::fd::AsRawFd;
        if libc::flock(file.as_raw_fd(), libc::LOCK_EX) != 0 {
            return Err(std::io::Error::last_os_error()).context("lock consent grants");
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("refusing symlinked consent grant storage");
    }
    Ok(())
}

fn reject_symlink_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        reject_symlink(path)?;
    }
    Ok(())
}

fn set_owner_only(path: &Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )?;
    }
    Ok(())
}
