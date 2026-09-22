//! Model-declared dispatch intent with no authority or approval material.
//!
//! This wire value is deliberately separate from `MissionDispatchBinding`.
//! It lets a producer request a target, role/profile, and file/entity scope,
//! but a server must derive all mission identity and authority independently.

use serde::{Deserialize, Serialize};

/// One requested repository-relative scope and its non-empty entity identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UntrustedDispatchScope {
    pub path: String,
    pub entity_id: String,
}

/// Model-declared dispatch preferences. This value can never authorize a spawn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UntrustedDispatchIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wave_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_read_scopes: Vec<UntrustedDispatchScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_write_scopes: Vec<UntrustedDispatchScope>,
}

impl UntrustedDispatchIntent {
    /// Reject malformed model preferences before they are put on a spawn wire
    /// request. This is lexical only: canonical project-root normalization and
    /// any approval comparison stay server-owned in the later admission packet.
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("wave_id", self.wave_id.as_deref()),
            ("task_id", self.task_id.as_deref()),
            ("role", self.role.as_deref()),
            ("profile", self.profile.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(format!("dispatch intent {name} must not be blank"));
            }
        }
        for scope in self
            .requested_read_scopes
            .iter()
            .chain(&self.requested_write_scopes)
        {
            validate_scope(scope)?;
        }
        Ok(())
    }
}

fn validate_scope(scope: &UntrustedDispatchScope) -> Result<(), String> {
    if scope.entity_id.trim().is_empty() {
        return Err("dispatch intent scope entity_id must not be blank".to_string());
    }
    let path = scope.path.as_str();
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("dispatch intent path is not normalized: {path}"));
    }
    Ok(())
}
