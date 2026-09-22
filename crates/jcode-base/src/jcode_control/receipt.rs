use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::request::{
    AuthenticatedPrincipal, CONTROL_RECEIPT_SCHEMA, ControlRequest, PinnedCreateSessionResolution,
    validate_identifier, validate_identifier_with_limit, validate_pinned_action, validate_text,
};

const MAX_RESULT_CODE_CHARS: usize = 128;
const MAX_RESULT_DETAIL_CHARS: usize = 12_000;

/// A caller-supplied, bounded durable result. This type makes no statement that
/// a session was created or a message accepted. Those statements belong only to
/// the later execution layer that chooses a result code.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlExecutionResult {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ControlExecutionResult {
    pub fn new(code: impl Into<String>, detail: Option<String>) -> Result<Self> {
        let result = Self {
            code: code.into(),
            detail,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<()> {
        validate_identifier_with_limit("result code", &self.code, MAX_RESULT_CODE_CHARS)?;
        if let Some(detail) = &self.detail {
            validate_text("result detail", detail, MAX_RESULT_DETAIL_CHARS)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ControlExecutionResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlExecutionResult")
            .field("code", &"REDACTED")
            .field("detail", &self.detail.as_ref().map(|_| "REDACTED"))
            .finish()
    }
}

/// The only durable receipt lifecycle. Claims and terminal results are local
/// authority records, not evidence that an action was actually performed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlReceiptState {
    Received,
    Claimed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned_create_session: Option<PinnedCreateSessionResolution>,
    },
    Completed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned_create_session: Option<PinnedCreateSessionResolution>,
        result: ControlExecutionResult,
    },
    Failed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned_create_session: Option<PinnedCreateSessionResolution>,
        result: ControlExecutionResult,
    },
}

impl fmt::Debug for ControlReceiptState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Received => "received",
            Self::Claimed { .. } => "claimed",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
        };
        formatter.write_str(label)
    }
}

/// One durable, replayable local control receipt.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlReceipt {
    pub schema: String,
    pub principal: AuthenticatedPrincipal,
    pub idempotency_key: String,
    pub request: ControlRequest,
    pub request_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_fingerprint: Option<String>,
    pub state: ControlReceiptState,
}

impl ControlReceipt {
    pub(crate) fn received(
        principal: AuthenticatedPrincipal,
        request: ControlRequest,
        request_fingerprint: String,
    ) -> Result<Self> {
        let receipt = Self {
            schema: CONTROL_RECEIPT_SCHEMA.to_string(),
            idempotency_key: request.idempotency_key.clone(),
            principal,
            request,
            request_fingerprint,
            execution_fingerprint: None,
            state: ControlReceiptState::Received,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != CONTROL_RECEIPT_SCHEMA {
            bail!("unsupported control receipt schema")
        }
        self.principal.validate()?;
        validate_identifier("idempotency key", &self.idempotency_key)?;
        self.request.validate()?;
        if self.request.idempotency_key != self.idempotency_key {
            bail!("receipt idempotency key does not match request")
        }
        validate_fingerprint("request", &self.request_fingerprint)?;
        let pinned = match &self.state {
            ControlReceiptState::Received => {
                if self.execution_fingerprint.is_some() {
                    bail!("received receipt must not have execution fingerprint")
                }
                return Ok(());
            }
            ControlReceiptState::Claimed {
                pinned_create_session,
            }
            | ControlReceiptState::Completed {
                pinned_create_session,
                ..
            }
            | ControlReceiptState::Failed {
                pinned_create_session,
                ..
            } => pinned_create_session,
        };
        validate_pinned_action(&self.request.action, pinned.as_ref())?;
        let execution_fingerprint = self
            .execution_fingerprint
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("claimed receipt is missing execution fingerprint"))?;
        validate_fingerprint("execution", execution_fingerprint)?;
        match &self.state {
            ControlReceiptState::Completed { result, .. }
            | ControlReceiptState::Failed { result, .. } => result.validate()?,
            ControlReceiptState::Received | ControlReceiptState::Claimed { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn pinned_create_session(&self) -> Option<&PinnedCreateSessionResolution> {
        match &self.state {
            ControlReceiptState::Received => None,
            ControlReceiptState::Claimed {
                pinned_create_session,
            }
            | ControlReceiptState::Completed {
                pinned_create_session,
                ..
            }
            | ControlReceiptState::Failed {
                pinned_create_session,
                ..
            } => pinned_create_session.as_ref(),
        }
    }
}

impl fmt::Debug for ControlReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ControlReceipt")
            .field("schema", &self.schema)
            .field("principal", &"REDACTED")
            .field("idempotency_key", &"REDACTED")
            .field("request", &"REDACTED")
            .field("request_fingerprint", &"REDACTED")
            .field(
                "execution_fingerprint",
                &self.execution_fingerprint.as_ref().map(|_| "REDACTED"),
            )
            .field("state", &self.state)
            .finish()
    }
}

/// The durable filesystem key, composed only from authenticated identity and
/// transport idempotency key. Its components are never read from a payload.
#[derive(Clone, PartialEq, Eq)]
pub struct ReceiptKey {
    principal: AuthenticatedPrincipal,
    idempotency_key: String,
}

impl ReceiptKey {
    pub fn new(
        principal: AuthenticatedPrincipal,
        idempotency_key: impl Into<String>,
    ) -> Result<Self> {
        let idempotency_key = idempotency_key.into();
        validate_identifier("idempotency key", &idempotency_key)?;
        Ok(Self {
            principal,
            idempotency_key,
        })
    }

    pub fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

impl fmt::Debug for ReceiptKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReceiptKey(REDACTED)")
    }
}
impl AuthenticatedPrincipal {
    fn validate(&self) -> Result<()> {
        validate_identifier("principal", self.as_str())
    }
}
fn validate_fingerprint(kind: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid {kind} fingerprint")
    }
    Ok(())
}
