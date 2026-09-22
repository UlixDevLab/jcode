use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const GRANT_SCHEMA_VERSION: u32 = 1;
const MAX_ACTIONS: usize = 16;
const MAX_RESOURCES: usize = 128;
const MAX_USES: u32 = 10_000;

/// A scope produced only by a fixed, service-specific registration in code.
/// M2 deliberately has no production registrations. Future providers must
/// canonicalize their own account and resource identifiers before constructing
/// this value here, rather than forwarding model text into a durable grant.
#[derive(Clone, Debug)]
pub(crate) struct RegisteredGrantScope {
    provider: String,
    principal: String,
    actions: Vec<String>,
    resource_ids: Vec<String>,
    maximum_uses: u32,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl RegisteredGrantScope {
    fn new(
        provider: impl Into<String>,
        principal: impl Into<String>,
        actions: impl IntoIterator<Item = impl Into<String>>,
        resource_ids: impl IntoIterator<Item = impl Into<String>>,
        maximum_uses: u32,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        let scope = Self {
            provider: provider.into(),
            principal: principal.into(),
            actions: canonical_values(actions),
            resource_ids: canonical_values(resource_ids),
            maximum_uses,
            created_at,
            expires_at,
        };
        scope.validate()?;
        Ok(scope)
    }

    fn validate(&self) -> Result<()> {
        validate_identifier("provider", &self.provider)?;
        validate_identifier("principal", &self.principal)?;
        validate_values("actions", &self.actions, MAX_ACTIONS)?;
        validate_values("resource_ids", &self.resource_ids, MAX_RESOURCES)?;
        if self.maximum_uses == 0 || self.maximum_uses > MAX_USES {
            bail!("maximum_uses must be between 1 and {MAX_USES}");
        }
        if self.expires_at <= self.created_at {
            bail!("grant expiry must be later than creation");
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        provider: impl Into<String>,
        principal: impl Into<String>,
        actions: impl IntoIterator<Item = impl Into<String>>,
        resource_ids: impl IntoIterator<Item = impl Into<String>>,
        maximum_uses: u32,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self> {
        Self::new(
            provider,
            principal,
            actions,
            resource_ids,
            maximum_uses,
            created_at,
            expires_at,
        )
    }
}

/// Fixed code registrations only. This empty M2 registry intentionally makes
/// grants inert: neither Gmail, MCP, browser, shell, filesystem, nor desktop
/// operations can create or consume a durable grant yet.
pub(crate) fn registered_scope(_registration: &str) -> Option<RegisteredGrantScope> {
    None
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BatchGrantManifest {
    pub(crate) schema_version: u32,
    pub(crate) provider: String,
    pub(crate) principal: String,
    pub(crate) actions: Vec<String>,
    pub(crate) resource_ids: Vec<String>,
    pub(crate) resource_digest: String,
    pub(crate) maximum_uses: u32,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) digest: String,
}

impl BatchGrantManifest {
    pub(crate) fn from_registered_scope(scope: &RegisteredGrantScope) -> Result<Self> {
        scope.validate()?;
        let mut manifest = Self {
            schema_version: GRANT_SCHEMA_VERSION,
            provider: scope.provider.clone(),
            principal: scope.principal.clone(),
            actions: scope.actions.clone(),
            resource_ids: scope.resource_ids.clone(),
            resource_digest: resource_digest(&scope.resource_ids),
            maximum_uses: scope.maximum_uses,
            created_at: scope.created_at,
            expires_at: scope.expires_at,
            digest: String::new(),
        };
        manifest.digest = manifest.computed_digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != GRANT_SCHEMA_VERSION {
            bail!("unsupported grant schema version");
        }
        let scope = RegisteredGrantScope {
            provider: self.provider.clone(),
            principal: self.principal.clone(),
            actions: self.actions.clone(),
            resource_ids: self.resource_ids.clone(),
            maximum_uses: self.maximum_uses,
            created_at: self.created_at,
            expires_at: self.expires_at,
        };
        scope.validate()?;
        if self.resource_digest != resource_digest(&self.resource_ids) {
            bail!("grant resource digest does not match canonical resources");
        }
        if self.digest != self.computed_digest() {
            bail!("grant manifest digest does not match manifest");
        }
        Ok(())
    }

    pub(crate) fn prompt(&self) -> String {
        format!(
            "JCODE_BATCH_GRANT_V1\nprovider={}\nprincipal={}\nactions={}\nresources={}\nresource_count={}\nexpires_at={}\nmanifest_digest={}\nApprove this batch grant?",
            self.provider,
            self.principal,
            self.actions.join(","),
            self.resource_ids.join(","),
            self.resource_ids.len(),
            self.expires_at.to_rfc3339(),
            self.digest,
        )
    }

    fn computed_digest(&self) -> String {
        #[derive(Serialize)]
        struct Canonical<'a> {
            schema_version: u32,
            provider: &'a str,
            principal: &'a str,
            actions: &'a [String],
            resource_ids: &'a [String],
            resource_digest: &'a str,
            maximum_uses: u32,
            created_at: DateTime<Utc>,
            expires_at: DateTime<Utc>,
        }
        digest_json(&Canonical {
            schema_version: self.schema_version,
            provider: &self.provider,
            principal: &self.principal,
            actions: &self.actions,
            resource_ids: &self.resource_ids,
            resource_digest: &self.resource_digest,
            maximum_uses: self.maximum_uses,
            created_at: self.created_at,
            expires_at: self.expires_at,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GrantMatcher {
    provider: String,
    principal: String,
    actions: Vec<String>,
    resource_digest: String,
}

impl GrantMatcher {
    pub(crate) fn from_registered_scope(scope: &RegisteredGrantScope) -> Result<Self> {
        scope.validate()?;
        Ok(Self {
            provider: scope.provider.clone(),
            principal: scope.principal.clone(),
            actions: scope.actions.clone(),
            resource_digest: resource_digest(&scope.resource_ids),
        })
    }

    pub(crate) fn matches(&self, manifest: &BatchGrantManifest) -> bool {
        manifest.schema_version == GRANT_SCHEMA_VERSION
            && manifest.provider == self.provider
            && manifest.principal == self.principal
            && manifest.actions == self.actions
            && manifest.resource_digest == self.resource_digest
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct GrantSummary {
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) actions: Vec<String>,
    pub(crate) resource_count: usize,
    pub(crate) remaining_uses: u32,
    pub(crate) expires_at: DateTime<Utc>,
    pub(crate) digest: String,
    pub(crate) revoked: bool,
}

fn canonical_values(values: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    let mut values = values.into_iter().map(Into::into).collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn validate_values(label: &str, values: &[String], max: usize) -> Result<()> {
    if values.is_empty() || values.len() > max {
        bail!("{label} must contain between 1 and {max} canonical values");
    }
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        bail!("{label} must be sorted and unique");
    }
    for value in values {
        validate_identifier(label, value)?;
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/' | b'@' | b'+')
        })
    {
        bail!("{label} contains an invalid canonical identifier");
    }
    Ok(())
}

fn resource_digest(resources: &[String]) -> String {
    let mut digest = Sha256::new();
    for resource in resources {
        digest.update((resource.len() as u64).to_be_bytes());
        digest.update(resource.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn digest_json(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("serializable fixed grant manifest");
    format!("{:x}", Sha256::digest(bytes))
}
