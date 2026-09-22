use super::{SelectionFailure, select_counterfactual, try_select_counterfactual};
use crate::message::{ContentBlock, Message, Role};

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
fn deterministic_budgeting_keeps_current_user_and_atomic_tool_bundle() {
    let messages = vec![
        text(
            Role::User,
            "older context that cannot fit the selected budget",
        ),
        text(Role::Assistant, "old model prose must not be selected"),
        text(Role::User, "current request"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".into(),
                name: "read".into(),
                input: serde_json::json!({"path":"README.md"}),
                thought_signature: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        },
        Message::tool_result("tool-1", "important tool evidence", false),
    ];

    let first = select_counterfactual(&messages, 300);
    let second = select_counterfactual(&messages, 300);

    assert_eq!(first, second);
    assert!(first.token_estimate <= first.token_budget);
    assert!(
        first
            .included
            .iter()
            .any(|entry| entry.reason == "required-current-user")
    );
    assert!(
        first
            .included
            .iter()
            .any(|entry| entry.reason == "required-active-tool-bundle")
    );
    assert!(
        first
            .excluded
            .iter()
            .any(|entry| entry.reason == "historical-model-prose")
    );
    assert_eq!(first.recall.selected, first.included.len());
}

#[test]
fn stable_tie_breaking_uses_transcript_order() {
    let messages = vec![
        text(Role::User, "first equally-sized historical evidence"),
        text(Role::User, "second equally-sized historical evidence"),
        text(Role::User, "current request"),
    ];

    let selection = select_counterfactual(&messages, 100);
    let selected_ids: Vec<_> = selection
        .included
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert!(selected_ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn archived_tool_heavy_fixture_has_minimal_evidence_and_recall_counters() {
    let messages: Vec<Message> = serde_json::from_str(include_str!(
        "../../tests/fixtures/rlm/long_tool_heavy.json"
    ))
    .expect("fixture parses");
    let selection = select_counterfactual(&messages, 500);

    assert!(
        selection
            .minimal_evidence_labels
            .iter()
            .any(|label| label == "current-user")
    );
    assert!(
        selection
            .included
            .iter()
            .any(|entry| entry.reason == "required-active-tool-bundle")
    );
    assert!(selection.recall.eligible > 0);
    assert_eq!(selection.recall.selected, selection.included.len());
    assert!(selection.token_estimate <= selection.token_budget);
}

#[test]
fn strict_plan_materializes_transcript_order_without_model_prose() {
    let messages = vec![
        text(Role::User, "older evidence"),
        text(Role::Assistant, "historical model prose"),
        text(Role::User, "current request"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".into(),
                name: "read".into(),
                input: serde_json::json!({"path":"README.md"}),
                thought_signature: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        },
        Message::tool_result("tool-1", "evidence", false),
    ];

    let plan = try_select_counterfactual(&messages, 500).expect("strict plan");
    assert_eq!(plan.current_user_index, 2);
    assert_eq!(plan.selected_indices, vec![0, 2, 3, 4]);
}

#[test]
fn strict_plan_rejects_duplicate_or_orphan_tool_ids() {
    let duplicate = vec![
        text(Role::User, "request"),
        Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "dup".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                    thought_signature: None,
                },
                ContentBlock::ToolUse {
                    id: "dup".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                    thought_signature: None,
                },
            ],
            timestamp: None,
            tool_duration_ms: None,
        },
        Message::tool_result("dup", "result", false),
    ];
    assert_eq!(
        try_select_counterfactual(&duplicate, 500),
        Err(SelectionFailure::DuplicateToolUse)
    );

    let orphan = vec![
        text(Role::User, "request"),
        Message::tool_result("missing", "result", false),
    ];
    assert_eq!(
        try_select_counterfactual(&orphan, 500),
        Err(SelectionFailure::OrphanToolResult)
    );
}

#[test]
fn strict_plan_rejects_missing_results_and_required_over_budget() {
    let missing = vec![
        text(Role::User, "request"),
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".into(),
                name: "read".into(),
                input: serde_json::json!({}),
                thought_signature: None,
            }],
            timestamp: None,
            tool_duration_ms: None,
        },
    ];
    assert_eq!(
        try_select_counterfactual(&missing, 500),
        Err(SelectionFailure::MissingToolResult)
    );

    let too_large = vec![text(Role::User, &"x".repeat(2_000))];
    assert_eq!(
        try_select_counterfactual(&too_large, 10),
        Err(SelectionFailure::RequiredContextOverBudget)
    );
}
