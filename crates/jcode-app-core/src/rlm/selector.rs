use crate::message::{ContentBlock, Message, Role};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterfactualSelection {
    pub included: Vec<SelectionEntry>,
    pub excluded: Vec<SelectionEntry>,
    pub token_budget: usize,
    pub token_estimate: usize,
    pub minimal_evidence_labels: Vec<String>,
    pub recall: RecallCounters,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity_error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CounterfactualPlan {
    pub selection: CounterfactualSelection,
    pub selected_indices: Vec<usize>,
    pub current_user_index: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionFailure {
    MissingCurrentUser,
    DuplicateToolUse,
    DuplicateToolResult,
    OrphanToolResult,
    MissingToolResult,
    ToolResultBeforeCall,
    RequiredContextOverBudget,
}
impl SelectionFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingCurrentUser => "missing-current-user",
            Self::DuplicateToolUse => "duplicate-tool-use-id",
            Self::DuplicateToolResult => "duplicate-tool-result-id",
            Self::OrphanToolResult => "orphan-tool-result",
            Self::MissingToolResult => "missing-tool-result",
            Self::ToolResultBeforeCall => "tool-result-before-call",
            Self::RequiredContextOverBudget => "required-context-over-budget",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionEntry {
    pub id: String,
    pub reason: String,
    pub content_hash: String,
    pub token_estimate: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallCounters {
    pub eligible: usize,
    pub selected: usize,
    pub excluded_for_budget: usize,
}

/// Selects an auditable counterfactual view only. The caller must keep using its
/// original message vector while observe mode is active.
pub fn select_counterfactual(messages: &[Message], token_budget: usize) -> CounterfactualSelection {
    match try_select_counterfactual(messages, token_budget) {
        Ok(plan) => plan.selection,
        Err(error) => CounterfactualSelection {
            token_budget,
            integrity_error: Some(error.as_str().to_string()),
            ..CounterfactualSelection::default()
        },
    }
}

pub(crate) fn try_select_counterfactual(
    messages: &[Message],
    token_budget: usize,
) -> Result<CounterfactualPlan, SelectionFailure> {
    let mut selection = CounterfactualSelection {
        token_budget,
        ..CounterfactualSelection::default()
    };
    let current_user = messages
        .iter()
        .rposition(is_user_text)
        .ok_or(SelectionFailure::MissingCurrentUser)?;
    let bundles = tool_bundles(messages)?;
    let mut assigned = BTreeSet::new();
    let mut required = Vec::new();

    required.push(Candidate::single(current_user, "required-current-user"));
    assigned.insert(current_user);
    for bundle in bundles.iter().filter(|bundle| bundle.call >= current_user) {
        required.push(Candidate::bundle(bundle, "required-active-tool-bundle"));
        assigned.extend(bundle.indices());
    }

    let required_tokens: usize = required
        .iter()
        .map(|candidate| candidate_token_estimate(messages, candidate))
        .sum();
    if required_tokens > token_budget {
        return Err(SelectionFailure::RequiredContextOverBudget);
    }
    for candidate in required {
        include(messages, &mut selection, candidate, "current-user");
    }

    let mut candidates = Vec::new();
    for bundle in &bundles {
        if bundle.overlaps(&assigned) {
            continue;
        }
        candidates.push(Candidate::bundle(bundle, "selected-tool-evidence"));
        assigned.extend(bundle.indices());
    }
    for (index, message) in messages.iter().enumerate() {
        if assigned.contains(&index) {
            continue;
        }
        if index < current_user && is_user_text(message) {
            candidates.push(Candidate::single(index, "selected-user-evidence"));
        } else {
            let reason = if has_tool_content(message) {
                "unpaired-tool-bundle"
            } else if message.role == Role::Assistant {
                "historical-model-prose"
            } else {
                "not-current-user-or-tool-bundle"
            };
            selection
                .excluded
                .push(entry(messages, Candidate::single(index, reason)));
        }
    }

    // Candidates have equal evidence rank within their class. Transcript index is
    // the deterministic tiebreaker, rather than hash-map or clock order.
    candidates.sort_by_key(|candidate| (candidate.reason, candidate.first()));
    selection.recall.eligible = candidates.len();
    for candidate in candidates {
        let estimate = candidate_token_estimate(messages, &candidate);
        if selection.token_estimate.saturating_add(estimate) <= token_budget {
            include(messages, &mut selection, candidate, "minimal-evidence");
        } else {
            selection.recall.excluded_for_budget += 1;
            let mut excluded = entry(messages, candidate);
            excluded.reason = "token-budget-exceeded".to_string();
            selection.excluded.push(excluded);
        }
    }
    selection
        .included
        .sort_by(|left, right| left.id.cmp(&right.id));
    selection
        .excluded
        .sort_by(|left, right| left.id.cmp(&right.id));
    selection.minimal_evidence_labels.sort();
    selection.minimal_evidence_labels.dedup();
    selection.recall.selected = selection.included.len();
    let selected_indices = assigned_selected_indices(messages, &selection, &bundles, current_user);
    Ok(CounterfactualPlan {
        selection,
        selected_indices,
        current_user_index: current_user,
    })
}

#[derive(Clone)]
struct Candidate {
    indices: Vec<usize>,
    reason: &'static str,
}

impl Candidate {
    fn single(index: usize, reason: &'static str) -> Self {
        Self {
            indices: vec![index],
            reason,
        }
    }

    fn bundle(bundle: &ToolBundle, reason: &'static str) -> Self {
        Self {
            indices: bundle.indices(),
            reason,
        }
    }

    fn first(&self) -> usize {
        self.indices[0]
    }
}

#[derive(Clone)]
struct ToolBundle {
    call: usize,
    results: Vec<usize>,
}

impl ToolBundle {
    fn indices(&self) -> Vec<usize> {
        let mut indices = vec![self.call];
        indices.extend(&self.results);
        indices
    }

    fn overlaps(&self, assigned: &BTreeSet<usize>) -> bool {
        self.indices().iter().any(|index| assigned.contains(index))
    }
}

fn tool_bundles(messages: &[Message]) -> Result<Vec<ToolBundle>, SelectionFailure> {
    let mut call_indices = BTreeMap::new();
    let mut result_indices = BTreeMap::new();
    for (index, message) in messages.iter().enumerate() {
        for block in &message.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    if call_indices.insert(id.as_str(), index).is_some() {
                        return Err(SelectionFailure::DuplicateToolUse);
                    }
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    if result_indices.insert(tool_use_id.as_str(), index).is_some() {
                        return Err(SelectionFailure::DuplicateToolResult);
                    }
                }
                _ => {}
            }
        }
    }
    if result_indices
        .keys()
        .any(|id| !call_indices.contains_key(id))
    {
        return Err(SelectionFailure::OrphanToolResult);
    }
    let mut bundles = Vec::new();
    for (call, message) in messages.iter().enumerate() {
        let ids: Vec<_> = message
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        if ids.is_empty() {
            continue;
        }
        let results = ids
            .iter()
            .map(|id| result_indices.get(id).copied())
            .collect::<Option<Vec<_>>>()
            .ok_or(SelectionFailure::MissingToolResult)?;
        if results.iter().any(|result| *result <= call) {
            return Err(SelectionFailure::ToolResultBeforeCall);
        }
        bundles.push(ToolBundle { call, results });
    }
    Ok(bundles)
}

fn assigned_selected_indices(
    messages: &[Message],
    selection: &CounterfactualSelection,
    bundles: &[ToolBundle],
    current_user: usize,
) -> Vec<usize> {
    let included_ids: BTreeSet<_> = selection
        .included
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    let mut indices = BTreeSet::from([current_user]);
    for bundle in bundles {
        let candidate = Candidate::bundle(bundle, "selected");
        if included_ids.contains(entry(messages, candidate).id.as_str()) {
            indices.extend(bundle.indices());
        }
    }
    for index in 0..current_user {
        if !is_user_text(&messages[index]) {
            continue;
        }
        let candidate = Candidate::single(index, "selected");
        if included_ids.contains(entry(messages, candidate).id.as_str()) {
            indices.insert(index);
        }
    }
    indices.into_iter().collect()
}

fn is_user_text(message: &Message) -> bool {
    message.role == Role::User
        && message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { .. }))
}

fn has_tool_content(message: &Message) -> bool {
    message.content.iter().any(|block| {
        matches!(
            block,
            ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
        )
    })
}

fn include(
    messages: &[Message],
    selection: &mut CounterfactualSelection,
    candidate: Candidate,
    label: &str,
) {
    selection.token_estimate += candidate_token_estimate(messages, &candidate);
    selection.included.push(entry(messages, candidate));
    selection.minimal_evidence_labels.push(label.to_string());
}

fn entry(messages: &[Message], candidate: Candidate) -> SelectionEntry {
    let bytes: Vec<u8> = candidate
        .indices
        .iter()
        .filter_map(|index| serde_json::to_vec(&messages[*index]).ok())
        .flatten()
        .collect();
    let first = candidate.first();
    let last = *candidate.indices.last().unwrap_or(&first);
    SelectionEntry {
        id: if first == last {
            format!("m{first:04}")
        } else {
            format!("m{first:04}-m{last:04}")
        },
        reason: candidate.reason.to_string(),
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
        token_estimate: bytes.len().div_ceil(4),
    }
}

fn candidate_token_estimate(messages: &[Message], candidate: &Candidate) -> usize {
    candidate
        .indices
        .iter()
        .filter_map(|index| serde_json::to_vec(&messages[*index]).ok())
        .map(|bytes| bytes.len().div_ceil(4))
        .sum()
}
