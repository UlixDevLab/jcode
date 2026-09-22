use crate::message::Message;

#[cfg(test)]
use crate::session::StoredMessage;

use super::{CounterfactualSelection, try_select_counterfactual};

#[cfg(test)]
use super::SelectionFailure;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PilotEligibility {
    reason: Option<&'static str>,
}

#[cfg(test)]
impl PilotEligibility {
    pub const fn eligible() -> Self {
        Self { reason: None }
    }

    pub const fn ineligible(reason: &'static str) -> Self {
        Self {
            reason: Some(reason),
        }
    }

    pub const fn reason(self) -> &'static str {
        match self.reason {
            Some(reason) => reason,
            None => "",
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) enum AssemblyOutcome {
    Baseline {
        messages: Vec<Message>,
        reason: &'static str,
    },
    Selected {
        messages: Vec<Message>,
    },
}

#[cfg(test)]
impl AssemblyOutcome {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Baseline { reason, .. } => reason,
            Self::Selected { .. } => "",
        }
    }

    pub fn messages(&self) -> &[Message] {
        match self {
            Self::Baseline { messages, .. } | Self::Selected { messages } => messages,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderView {
    pub messages: Vec<Message>,
    pub selected: bool,
    pub fallback_reason: Option<String>,
    pub selection: Option<CounterfactualSelection>,
}

impl ProviderView {
    pub fn baseline(messages: &[Message], reason: Option<impl Into<String>>) -> Self {
        Self {
            messages: messages.to_vec(),
            selected: false,
            fallback_reason: reason.map(Into::into),
            selection: None,
        }
    }
}

#[cfg(test)]
pub(crate) fn assemble_selected_context(
    baseline: &[Message],
    root_message: &StoredMessage,
    eligibility: PilotEligibility,
    token_budget: usize,
) -> AssemblyOutcome {
    if let Some(reason) = eligibility.reason {
        return AssemblyOutcome::Baseline {
            messages: baseline.to_vec(),
            reason,
        };
    }
    let plan = match try_select_counterfactual(baseline, token_budget) {
        Ok(plan) => plan,
        Err(error) => {
            return AssemblyOutcome::Baseline {
                messages: baseline.to_vec(),
                reason: selection_failure_reason(error),
            };
        }
    };
    if !baseline
        .get(plan.current_user_index)
        .is_some_and(|message| same_message(message, &root_message.to_message()))
    {
        return AssemblyOutcome::Baseline {
            messages: baseline.to_vec(),
            reason: "rlm-integrity-anchor-missing",
        };
    }
    let messages: Vec<_> = plan
        .selected_indices
        .iter()
        .filter_map(|index| baseline.get(*index).cloned())
        .collect();
    if messages.len() != plan.selected_indices.len() {
        return AssemblyOutcome::Baseline {
            messages: baseline.to_vec(),
            reason: "rlm-integrity-selected-index",
        };
    }
    AssemblyOutcome::Selected { messages }
}

pub(crate) fn assemble_pilot_view(
    baseline: &[Message],
    root_message: Option<&Message>,
    token_budget: usize,
) -> ProviderView {
    let Some(root_message) = root_message else {
        return ProviderView::baseline(baseline, Some("root-message-missing"));
    };
    let plan = match try_select_counterfactual(baseline, token_budget) {
        Ok(plan) => plan,
        Err(error) => return ProviderView::baseline(baseline, Some(error.as_str())),
    };
    if !baseline
        .get(plan.current_user_index)
        .is_some_and(|message| same_message(message, root_message))
    {
        return ProviderView::baseline(baseline, Some("root-message-mismatch"));
    }
    let messages: Vec<_> = plan
        .selected_indices
        .iter()
        .filter_map(|index| baseline.get(*index).cloned())
        .collect();
    if messages.len() != plan.selected_indices.len() {
        return ProviderView::baseline(baseline, Some("selected-index-out-of-range"));
    }
    ProviderView {
        messages,
        selected: true,
        fallback_reason: None,
        selection: Some(plan.selection),
    }
}

#[cfg(test)]
fn selection_failure_reason(error: SelectionFailure) -> &'static str {
    match error {
        SelectionFailure::MissingCurrentUser => "rlm-integrity-current-user-missing",
        SelectionFailure::DuplicateToolUse => "rlm-integrity-duplicate-tool-call",
        SelectionFailure::DuplicateToolResult => "rlm-integrity-duplicate-tool-result",
        SelectionFailure::OrphanToolResult => "rlm-integrity-orphan-tool-result",
        SelectionFailure::MissingToolResult => "rlm-integrity-missing-tool-result",
        SelectionFailure::ToolResultBeforeCall => "rlm-integrity-tool-result-order",
        SelectionFailure::RequiredContextOverBudget => "rlm-budget-required-context",
    }
}

fn same_message(left: &Message, right: &Message) -> bool {
    serde_json::to_vec(&(&left.role, &left.content)).ok()
        == serde_json::to_vec(&(&right.role, &right.content)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ContentBlock, Role};

    fn text(role: Role, value: &str) -> Message {
        Message {
            role,
            content: vec![ContentBlock::Text {
                text: value.into(),
                cache_control: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        }
    }

    #[test]
    fn pilot_selects_before_ephemeral_suffixes_are_added() {
        let baseline = vec![
            text(Role::User, "old evidence"),
            text(Role::Assistant, "old prose"),
            text(Role::User, "current request"),
        ];
        let view = assemble_pilot_view(&baseline, baseline.last(), 500);
        assert!(view.selected);
        assert_eq!(view.messages.len(), 2);
        assert!(same_message(
            view.messages.last().expect("selected current user"),
            baseline.last().expect("baseline current user")
        ));
    }

    #[test]
    fn root_mismatch_falls_back_to_exact_baseline() {
        let baseline = vec![text(Role::User, "current request")];
        let wrong_root = text(Role::User, "memory suffix");
        let view = assemble_pilot_view(&baseline, Some(&wrong_root), 500);
        assert!(!view.selected);
        assert_eq!(
            view.fallback_reason.as_deref(),
            Some("root-message-mismatch")
        );
        assert_eq!(
            serde_json::to_vec(&view.messages).expect("serialize view"),
            serde_json::to_vec(&baseline).expect("serialize baseline")
        );
    }
}
