use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

pub const CONTROL_SCHEMA: &str = "jcode_control.v1";
pub const CONTROL_RECEIPT_SCHEMA: &str = "jcode_control.receipt.v1";
pub const MAX_MESSAGE_CHARS: usize = 12_000;
pub const MAX_INITIAL_PROMPT_CHARS: usize = 12_000;
const MAX_IDENTIFIER_CHARS: usize = 128;

/// Identity supplied by the future authenticated transport adapter, never by the
/// untrusted control payload.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthenticatedPrincipal(String);

impl AuthenticatedPrincipal {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_identifier("principal", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthenticatedPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticatedPrincipal(REDACTED)")
    }
}

/// The sole payload accepted from a later trusted control adapter.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    #[serde(deserialize_with = "deserialize_control_schema")]
    pub schema: String,
    #[serde(deserialize_with = "deserialize_idempotency_key")]
    pub idempotency_key: String,
    pub action: ControlAction,
}

impl ControlRequest {
    pub fn validate(&self) -> Result<()> {
        if self.schema != CONTROL_SCHEMA {
            bail!("unsupported control schema")
        }
        validate_identifier("idempotency key", &self.idempotency_key)?;
        self.action.validate()
    }
}

impl fmt::Debug for ControlRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlRequest")
            .field("schema", &self.schema)
            .field("idempotency_key", &"REDACTED")
            .field("action", &self.action)
            .finish()
    }
}

/// The intentionally tiny action set. It accepts neither paths, credentials,
/// transport identifiers, shell/protocol operations, gates, nor broad resume.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlAction {
    MessageExisting {
        #[serde(deserialize_with = "deserialize_session_id")]
        jcode_session_id: String,
        #[serde(deserialize_with = "deserialize_message")]
        message: String,
        #[serde(default)]
        wake: bool,
    },
    CreateSession {
        #[serde(deserialize_with = "deserialize_root_id")]
        root_id: String,
        #[serde(deserialize_with = "deserialize_route_alias")]
        route_alias: String,
        #[serde(default, deserialize_with = "deserialize_initial_prompt")]
        initial_prompt: Option<String>,
    },
}

impl ControlAction {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::MessageExisting {
                jcode_session_id,
                message,
                ..
            } => {
                validate_identifier("jcode session", jcode_session_id)?;
                validate_text("message", message, MAX_MESSAGE_CHARS)
            }
            Self::CreateSession {
                root_id,
                route_alias,
                initial_prompt,
            } => {
                validate_identifier("root", root_id)?;
                validate_identifier("route alias", route_alias)?;
                if let Some(initial_prompt) = initial_prompt {
                    validate_text("initial prompt", initial_prompt, MAX_INITIAL_PROMPT_CHARS)?;
                }
                Ok(())
            }
        }
    }

    pub(crate) fn pinned_create_session(&self) -> Option<PinnedCreateSessionResolution> {
        match self {
            Self::MessageExisting { .. } => None,
            Self::CreateSession {
                root_id,
                route_alias,
                ..
            } => Some(PinnedCreateSessionResolution {
                root_id: root_id.clone(),
                route_alias: route_alias.clone(),
            }),
        }
    }
}

impl fmt::Debug for ControlAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MessageExisting { wake, .. } => formatter
                .debug_struct("MessageExisting")
                .field("jcode_session_id", &"REDACTED")
                .field("message", &"REDACTED")
                .field("wake", wake)
                .finish(),
            Self::CreateSession { .. } => formatter
                .debug_struct("CreateSession")
                .field("root_id", &"REDACTED")
                .field("route_alias", &"REDACTED")
                .field("initial_prompt", &"REDACTED")
                .finish(),
        }
    }
}

/// The resolved root/route identity frozen atomically when a session creation is
/// claimed. It must exactly match the validated request and cannot redirect it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedCreateSessionResolution {
    pub root_id: String,
    pub route_alias: String,
}

impl PinnedCreateSessionResolution {
    pub fn new(root_id: impl Into<String>, route_alias: impl Into<String>) -> Result<Self> {
        let resolution = Self {
            root_id: root_id.into(),
            route_alias: route_alias.into(),
        };
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn validate(&self) -> Result<()> {
        validate_identifier("root", &self.root_id)?;
        validate_identifier("route alias", &self.route_alias)
    }
}
pub(crate) fn validate_pinned_action(
    action: &ControlAction,
    pinned: Option<&PinnedCreateSessionResolution>,
) -> Result<()> {
    match (action.pinned_create_session(), pinned) {
        (None, None) => Ok(()),
        (Some(expected), Some(actual)) if expected == *actual => Ok(()),
        (Some(_), None) => bail!("create session claim requires pinned root and route resolution"),
        (None, Some(_)) => bail!("message existing claim cannot include root or route resolution"),
        (Some(_), Some(_)) => bail!("conflicting pinned resolution for control request"),
    }
}

pub(crate) fn validate_identifier(kind: &str, value: &str) -> Result<()> {
    validate_identifier_with_limit(kind, value, MAX_IDENTIFIER_CHARS)
}

pub(super) fn validate_identifier_with_limit(
    kind: &str,
    value: &str,
    maximum: usize,
) -> Result<()> {
    if value.is_empty()
        || value.len() > maximum
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("invalid {kind}")
    }
    Ok(())
}

pub(super) fn validate_text(kind: &str, value: &str, maximum: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.chars().count() > maximum
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
    {
        bail!("invalid {kind}")
    }
    Ok(())
}
fn deserialize_control_schema<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    (value == CONTROL_SCHEMA)
        .then_some(value)
        .ok_or_else(|| serde::de::Error::custom("unsupported control schema"))
}

fn deserialize_idempotency_key<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_checked_identifier(deserializer, "idempotency key")
}

fn deserialize_session_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_checked_identifier(deserializer, "jcode session")
}

fn deserialize_root_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_checked_identifier(deserializer, "root")
}

fn deserialize_route_alias<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_checked_identifier(deserializer, "route alias")
}

fn deserialize_checked_identifier<'de, D>(deserializer: D, kind: &str) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    validate_identifier(kind, &value).map_err(serde::de::Error::custom)?;
    Ok(value)
}

fn deserialize_message<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    validate_text("message", &value, MAX_MESSAGE_CHARS).map_err(serde::de::Error::custom)?;
    Ok(value)
}

fn deserialize_initial_prompt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    if let Some(value) = &value {
        validate_text("initial prompt", value, MAX_INITIAL_PROMPT_CHARS)
            .map_err(serde::de::Error::custom)?;
    }
    Ok(value)
}
