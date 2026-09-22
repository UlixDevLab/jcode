#[test]
fn test_comm_spawn_roundtrip_with_mission_binding() -> Result<()> {
    let mission_binding = MissionDispatchBinding {
        mission_id: "mission-alpha".to_string(),
        revision: 1,
        artifact_hash: "sha256:v1:abc123".to_string(),
        plan_hash: "sha256:v1:def456".to_string(),
        scope_digest: "sha256:v1:ghi789".to_string(),
        wave_id: "wave-1".to_string(),
        task_id: "task-1".to_string(),
        write_set: vec!["crates/jcode-protocol/src/mission.rs".to_string()],
        read_set: Vec::new(),
        entity_ids: vec!["protocol-binding".to_string()],
        role: "forge".to_string(),
        spawn_mode: "headless".to_string(),
    };
    let request = Request::CommSpawn {
        id: 60,
        session_id: "sess_coord".to_string(),
        working_dir: None,
        initial_message: None,
        request_nonce: None,
        spawn_mode: Some("headless".to_string()),
        model: None,
        effort: None,
        label: None,
        mission_binding: Some(mission_binding.clone()),
        dispatch_intent: None,
    };

    let json = serde_json::to_string(&request)?;
    assert!(json.contains("\"mission_binding\""));
    let Request::CommSpawn {
        mission_binding: decoded,
        ..
    } = parse_request_json(&json)?
    else {
        return Err(anyhow!("expected CommSpawn"));
    };
    assert_eq!(decoded, Some(mission_binding));
    Ok(())
}

#[test]
fn test_comm_spawn_decodes_without_a_mission_binding_for_older_clients() -> Result<()> {
    // Older clients omit all optional spawn fields, including mission_binding.
    let json = r#"{"type":"comm_spawn","id":60,"session_id":"sess_coord"}"#;
    let decoded = parse_request_json(json)?;
    let Request::CommSpawn {
        model,
        effort,
        label,
        mission_binding,
        dispatch_intent,
        ..
    } = decoded
    else {
        return Err(anyhow!("expected CommSpawn"));
    };
    assert_eq!(model, None);
    assert_eq!(effort, None);
    assert_eq!(label, None);
    assert_eq!(mission_binding, None);
    assert_eq!(dispatch_intent, None);
    Ok(())
}

#[test]
fn test_legacy_mission_binding_defaults_new_admission_fields() -> Result<()> {
    let json = r#"{"type":"comm_spawn","id":60,"session_id":"sess_coord","mission_binding":{"mission_id":"mission-alpha","revision":1,"artifact_hash":"sha256:v1:abc123","wave_id":"wave-1","task_id":"task-1","write_set":["src/lib.rs"],"entity_ids":["entity-1"],"role":"forge","spawn_mode":"headless"}}"#;
    let Request::CommSpawn {
        mission_binding: Some(binding),
        ..
    } = parse_request_json(json)?
    else {
        return Err(anyhow!("expected CommSpawn with mission binding"));
    };
    assert!(binding.plan_hash.is_empty());
    assert!(binding.scope_digest.is_empty());
    assert!(binding.read_set.is_empty());
    Ok(())
}
