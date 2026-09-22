use super::{AssemblyOutcome, PilotEligibility, assemble_selected_context};
use crate::{
    message::{ContentBlock, Message, Role},
    session::StoredMessage,
};

fn user(text: &str) -> Message {
    Message::user(text)
}

fn stored_user(text: &str) -> StoredMessage {
    let mut session =
        crate::session::Session::create_with_id("assembler-anchor".into(), None, None);
    session.add_message(Role::User, user(text).content);
    session.messages.pop().expect("stored user")
}

fn encoded(messages: &[Message]) -> Vec<u8> {
    serde_json::to_vec(messages).expect("serialize messages")
}

fn tool_call(id: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: id.into(),
            name: "read".into(),
            input: serde_json::json!({"path": "README.md"}),
            thought_signature: None,
        }],
        timestamp: None,
        tool_duration_ms: None,
    }
}

fn eligible() -> PilotEligibility {
    PilotEligibility::eligible()
}

#[test]
fn ordinary_and_observe_views_are_returned_unchanged() {
    let baseline = vec![user("previous"), user("current")];
    let anchor = stored_user("current");

    for eligibility in [
        PilotEligibility::ineligible("rlm-ineligible-not-rlm"),
        PilotEligibility::ineligible("rlm-ineligible-mode-observe"),
    ] {
        let outcome = assemble_selected_context(&baseline, &anchor, eligibility, 1);
        assert_eq!(outcome.reason(), eligibility.reason());
        assert_eq!(encoded(outcome.messages()), encoded(&baseline));
    }
}

#[test]
fn selected_pilot_dispatch_anchors_the_turn_entry_user_before_suffixes() {
    let baseline = vec![user("historical evidence"), user("turn-entry user")];
    let anchor = stored_user("turn-entry user");

    let AssemblyOutcome::Selected { messages } =
        assemble_selected_context(&baseline, &anchor, eligible(), 1_000)
    else {
        panic!("eligible pilot must select context");
    };

    assert_eq!(encoded(&messages), encoded(&baseline));
}

#[test]
fn selected_tool_pairs_are_atomic_and_preserve_transcript_order() {
    let baseline = vec![
        user("turn-entry user"),
        tool_call("read-1"),
        Message::tool_result("read-1", "important evidence", false),
    ];
    let anchor = stored_user("turn-entry user");

    let AssemblyOutcome::Selected { messages } =
        assemble_selected_context(&baseline, &anchor, eligible(), 1_000)
    else {
        panic!("eligible pilot must select context");
    };

    assert_eq!(encoded(&messages), encoded(&baseline));
}

#[test]
fn duplicate_or_orphan_tool_pairs_fail_closed_with_stable_reasons() {
    let anchor = stored_user("turn-entry user");
    let duplicate = vec![
        user("turn-entry user"),
        tool_call("same"),
        tool_call("same"),
    ];
    let orphan = vec![
        user("turn-entry user"),
        Message::tool_result("missing", "orphan", false),
    ];

    assert_eq!(
        assemble_selected_context(&duplicate, &anchor, eligible(), 1_000).reason(),
        "rlm-integrity-duplicate-tool-call"
    );
    assert_eq!(
        assemble_selected_context(&orphan, &anchor, eligible(), 1_000).reason(),
        "rlm-integrity-orphan-tool-result"
    );
}

#[test]
fn missing_anchor_fails_closed_with_a_stable_reason() {
    let baseline = vec![user("other user")];
    let anchor = stored_user("turn-entry user");

    assert_eq!(
        assemble_selected_context(&baseline, &anchor, eligible(), 1_000).reason(),
        "rlm-integrity-anchor-missing"
    );
}

#[test]
fn selected_views_are_identical_for_streaming_and_non_streaming_inputs() {
    let baseline = vec![user("historical"), user("turn-entry user")];
    let anchor = stored_user("turn-entry user");
    let streaming = assemble_selected_context(&baseline, &anchor, eligible(), 1_000);
    let non_streaming = assemble_selected_context(&baseline, &anchor, eligible(), 1_000);

    assert_eq!(streaming.reason(), non_streaming.reason());
    assert_eq!(
        encoded(streaming.messages()),
        encoded(non_streaming.messages())
    );
    assert_eq!(encoded(streaming.messages()), encoded(&baseline));
}
