#[test]
fn test_history_activity_context_roundtrip_is_optional_and_structured() -> Result<()> {
    let json = r#"{
        "type":"history",
        "id":1,
        "session_id":"ses_test_123",
        "messages":[],
        "activity_context":{
            "repository":"private-project",
            "started_at":"2026-09-02T11:48:49+00:00",
            "activity_label":"Working"
        }
    }"#;

    let ServerEvent::History {
        activity_context: Some(activity_context),
        ..
    } = parse_event_json(json)?
    else {
        return Err(anyhow!("expected History event with activity context"));
    };

    assert_eq!(activity_context.repository, "private-project");
    assert_eq!(activity_context.started_at, "2026-09-02T11:48:49+00:00");
    assert_eq!(activity_context.activity_label, "Working");
    Ok(())
}
