use jcode_desktop_core::{
    ApplicationMetadata, DeliveryState, DesktopEnvelope, DesktopPatch, DesktopState, MessageView,
};

#[test]
fn bootstrap_envelope_matches_the_checked_in_v1_contract() {
    let actual = serde_json::to_string_pretty(
        &DesktopState::bootstrap(ApplicationMetadata::demo()).snapshot_envelope(),
    )
    .expect("serialize v1 fixture");
    assert_eq!(actual + "\n", include_str!("fixtures/bootstrap_v1.json"));
}

#[test]
fn typed_patch_envelope_matches_the_checked_in_v1_contract() {
    let patch = DesktopEnvelope::patch(
        1,
        1,
        DesktopPatch::Message {
            message: MessageView::user(
                "message_demo_0001",
                "explain the harness API handshake",
                DeliveryState::Sending,
            ),
        },
    );
    let actual = serde_json::to_string_pretty(&patch).expect("serialize v1 patch fixture");
    assert_eq!(actual + "\n", include_str!("fixtures/send_patch_v1.json"));
}
