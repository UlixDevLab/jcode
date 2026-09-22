#[test]
fn untrusted_dispatch_intent_roundtrips_without_authority_or_digests() -> Result<()> {
    let intent = UntrustedDispatchIntent {
        wave_id: Some("wave-producer".to_string()),
        task_id: Some("task-producer".to_string()),
        role: Some("forge".to_string()),
        profile: Some("build".to_string()),
        requested_read_scopes: vec![UntrustedDispatchScope {
            path: "crates/jcode-protocol/src/lib.rs".to_string(),
            entity_id: "protocol-exports".to_string(),
        }],
        requested_write_scopes: vec![UntrustedDispatchScope {
            path: "crates/jcode-protocol/src/wire.rs".to_string(),
            entity_id: "wire-request".to_string(),
        }],
    };

    let json = serde_json::to_string(&intent)?;
    for trusted_field in [
        "mission_id",
        "revision",
        "artifact_hash",
        "plan_hash",
        "scope_digest",
        "approval",
        "authority",
        "identity",
    ] {
        assert!(
            !json.contains(&format!("\"{trusted_field}\"")),
            "untrusted intent must not contain trusted field {trusted_field}"
        );
    }

    let decoded: UntrustedDispatchIntent = serde_json::from_str(&json)?;
    assert_eq!(decoded, intent);
    decoded.validate().map_err(anyhow::Error::msg)?;
    Ok(())
}

#[test]
fn untrusted_dispatch_intent_rejects_traversal_empty_entity_and_trusted_fields() {
    for json in [
        r#"{"requested_write_scopes":[{"path":"../secret.rs","entity_id":"safe"}]}"#,
        r#"{"requested_write_scopes":[{"path":"src/lib.rs","entity_id":""}]}"#,
    ] {
        let intent: UntrustedDispatchIntent =
            serde_json::from_str(json).expect("shape should deserialize before validation");
        assert!(intent.validate().is_err(), "intent {json} must be rejected");
    }

    assert!(
        serde_json::from_str::<UntrustedDispatchIntent>(
            r#"{"mission_id":"model-must-not-claim-this"}"#
        )
        .is_err(),
        "trusted fields must be rejected by the typed untrusted schema"
    );
}
