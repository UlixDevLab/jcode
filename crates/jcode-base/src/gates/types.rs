use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use super::binding::{GateBinding, GateSubject};
use super::transport::GateTransportBinding;

pub const GATE_SCHEMA_VERSION: u32 = 2;
const LEGACY_GATE_SCHEMA_VERSION: u32 = 1;

/// One user-authorized decision accepted by a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    Approve,
    RequestChanges,
    Reject,
}

/// A concise option a user can select while deciding a gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateOption {
    pub label: String,
    pub description: String,
}

/// The high-level decision context persisted for a user gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateQuestion {
    pub question: String,
    pub problem: String,
    pub impact: String,
    pub options: Vec<GateOption>,
    pub recommendation: String,
}

/// Bounded continuation descriptions. They intentionally contain no tool I/O.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateContinuation {
    pub on_approve: String,
    pub on_request_changes: String,
    pub on_reject: String,
    pub on_expiry: String,
}

/// Delivery state is observational only and cannot authorize a continuation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeliveryAcknowledgement {
    NotAttempted,
    Acknowledged {
        acknowledged_at: DateTime<Utc>,
    },
    DeliveryUnknown {
        observed_at: DateTime<Utc>,
        reason: String,
    },
}

/// A validated reply received from any transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerRecord {
    pub schema_version: u32,
    pub gate_id: String,
    #[serde(flatten)]
    pub binding: GateBinding,
    pub decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub answered_at: DateTime<Utc>,
    pub received_via: String,
}

/// One deterministic, transport-free instruction for a higher layer to resume
/// after an accepted answer. Its gate and session key are the containing record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateResumeDirective {
    pub decision: GateDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub continuation: String,
    pub state: GateResumeDirectiveState,
}

/// A directive can be claimed once by the eventual lifecycle consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GateResumeDirectiveState {
    Unconsumed,
    Consumed { consumed_at: DateTime<Utc> },
}

/// The gate lifecycle. `Applied` must be persisted before a higher layer wakes
/// or resumes a session. This packet intentionally performs no wake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GateState {
    Pending,
    Answered {
        answer: AnswerRecord,
    },
    Applied {
        answer: AnswerRecord,
        applied_at: DateTime<Utc>,
    },
    /// Reserved for malformed or unanswerable no-continuation cases. A valid
    /// user `reject` is always represented by `Answered` then `Applied`.
    #[serde(alias = "rejected")]
    Unanswerable {
        answer: AnswerRecord,
    },
    Expired {
        expired_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
        reason: String,
    },
}

/// One durable, high-level user gate. There are no credential or tool-output
/// fields in this data model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRequest {
    pub schema_version: u32,
    pub gate_id: String,
    #[serde(flatten)]
    pub binding: GateBinding,
    pub question: GateQuestion,
    pub allowed_decisions: Vec<GateDecision>,
    pub continuation: GateContinuation,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub delivery: DeliveryAcknowledgement,
    pub state: GateState,
    /// Optional Hermes envelope correlation. It is never authorization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport_binding: Option<GateTransportBinding>,
    /// Written in the same atomic record replacement as an accepted answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_directive: Option<GateResumeDirective>,
}

impl GateRequest {
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.schema_version,
            LEGACY_GATE_SCHEMA_VERSION | GATE_SCHEMA_VERSION
        ) {
            bail!("unsupported gate schema version")
        }
        validate_identifier("gate", &self.gate_id)?;
        self.binding.validate()?;
        if self.schema_version == LEGACY_GATE_SCHEMA_VERSION
            && !matches!(self.binding.subject, GateSubject::MissionPlan { .. })
        {
            bail!("session decision gates require schema version 2")
        }
        if let Some(transport) = &self.transport_binding {
            transport.validate()?;
            if transport.gate_id != self.gate_id || transport.binding != self.binding {
                bail!("gate transport binding does not match its local gate binding")
            }
        }
        self.question.validate()?;
        self.continuation.validate()?;
        if self.created_at >= self.expires_at {
            bail!("gate expiry must be after creation")
        }
        if self.allowed_decisions.is_empty() {
            bail!("gate must permit at least one decision")
        }
        for (index, decision) in self.allowed_decisions.iter().enumerate() {
            if self.allowed_decisions[..index].contains(decision) {
                bail!("gate contains duplicate allowed decision")
            }
        }
        self.delivery.validate()?;
        match &self.state {
            GateState::Pending | GateState::Expired { .. } => self.require_no_directive()?,
            GateState::Answered { answer } => {
                self.validate_answer(answer)?;
                self.validate_resume_directive(answer)?;
            }
            GateState::Applied { answer, .. } | GateState::Unanswerable { answer } => {
                self.validate_answer(answer)?;
                self.require_no_directive()?;
            }
            GateState::Cancelled { reason, .. } => {
                validate_text("cancellation reason", reason)?;
                self.require_no_directive()?;
            }
        }
        Ok(())
    }

    pub fn validate_answer(&self, answer: &AnswerRecord) -> Result<()> {
        answer.validate()?;
        if answer.gate_id != self.gate_id || answer.binding != self.binding {
            bail!("gate answer does not match its gate binding")
        }
        if !self.allowed_decisions.contains(&answer.decision) {
            bail!("gate answer uses a decision not allowed by this gate")
        }
        if matches!(answer.decision, GateDecision::RequestChanges)
            && answer.comment.as_deref().is_none_or(str::is_empty)
        {
            bail!("request_changes requires a comment")
        }
        Ok(())
    }

    pub fn resume_directive_for(&self, answer: &AnswerRecord) -> GateResumeDirective {
        GateResumeDirective {
            decision: answer.decision,
            reason: answer.comment.clone(),
            continuation: match answer.decision {
                GateDecision::Approve => self.continuation.on_approve.clone(),
                GateDecision::RequestChanges => self.continuation.on_request_changes.clone(),
                GateDecision::Reject => self.continuation.on_reject.clone(),
            },
            state: GateResumeDirectiveState::Unconsumed,
        }
    }

    fn require_no_directive(&self) -> Result<()> {
        if self.resume_directive.is_some() {
            bail!("only an answered gate may retain a resume directive")
        }
        Ok(())
    }

    fn validate_resume_directive(&self, answer: &AnswerRecord) -> Result<()> {
        let directive = self
            .resume_directive
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("answered gate is missing a resume directive"))?;
        directive.validate()?;
        let expected = self.resume_directive_for(answer);
        if directive.decision != expected.decision
            || directive.reason != expected.reason
            || directive.continuation != expected.continuation
        {
            bail!("resume directive does not match its accepted answer")
        }
        Ok(())
    }
}

impl GateQuestion {
    fn validate(&self) -> Result<()> {
        validate_text("gate question", &self.question)?;
        validate_text("gate problem", &self.problem)?;
        validate_text("gate impact", &self.impact)?;
        validate_text("gate recommendation", &self.recommendation)?;
        if self.options.is_empty() {
            bail!("gate must include at least one high-level option")
        }
        for option in &self.options {
            validate_text("gate option label", &option.label)?;
            validate_text("gate option description", &option.description)?;
        }
        Ok(())
    }
}

impl GateContinuation {
    fn validate(&self) -> Result<()> {
        validate_text("approve continuation", &self.on_approve)?;
        validate_text("request_changes continuation", &self.on_request_changes)?;
        validate_text("reject continuation", &self.on_reject)?;
        validate_text("expiry continuation", &self.on_expiry)
    }
}

impl DeliveryAcknowledgement {
    fn validate(&self) -> Result<()> {
        if let Self::DeliveryUnknown { reason, .. } = self {
            validate_text("delivery unknown reason", reason)?;
        }
        Ok(())
    }
}

impl AnswerRecord {
    pub fn validate(&self) -> Result<()> {
        if !matches!(
            self.schema_version,
            LEGACY_GATE_SCHEMA_VERSION | GATE_SCHEMA_VERSION
        ) {
            bail!("unsupported gate answer schema version")
        }
        validate_identifier("gate", &self.gate_id)?;
        self.binding.validate()?;
        if self.schema_version == LEGACY_GATE_SCHEMA_VERSION
            && !matches!(self.binding.subject, GateSubject::MissionPlan { .. })
        {
            bail!("session decision answers require schema version 2")
        }
        validate_text("answer transport", &self.received_via)?;
        if let Some(comment) = &self.comment {
            validate_text("answer comment", comment)?;
        }
        Ok(())
    }
}

impl GateResumeDirective {
    fn validate(&self) -> Result<()> {
        validate_text("resume continuation", &self.continuation)?;
        if let Some(reason) = &self.reason {
            validate_text("resume reason", reason)?;
        }
        Ok(())
    }
}

pub(super) fn validate_identifier(kind: &str, value: &str) -> Result<()> {
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

pub(super) fn validate_text(kind: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 12_000 || value.chars().any(char::is_control) {
        bail!("invalid {kind}")
    }
    Ok(())
}
