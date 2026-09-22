use super::{CommunicateInput, CommunicateTool};
use crate::protocol::Request;
use crate::tool::Tool;
use serde_json::json;

#[test]
fn explicit_spawn_and_assignment_fallback_build_equivalent_untrusted_intent() {
    let params: CommunicateInput = serde_json::from_value(json!({
        "action": "spawn",
        "dispatch_intent": {
            "wave_id": "wave-producer",
            "task_id": "task-producer",
            "role": "forge",
            "profile": "build",
            "requested_read_scopes": [{
                "path": "crates/jcode-protocol/src/lib.rs",
                "entity_id": "protocol-exports"
            }],
            "requested_write_scopes": [{
                "path": "crates/jcode-app-core/src/tool/communicate.rs",
                "entity_id": "producer-seam"
            }]
        }
    }))
    .expect("typed dispatch intent should deserialize");

    let explicit = params
        .build_spawn_request(
            "coord",
            params.spawn_initial_message(),
            None,
            Some("producer".into()),
        )
        .expect("explicit request should build");
    let fallback = params
        .build_spawn_request("coord", None, Some("nonce".into()), None)
        .expect("fallback request should build");

    let Request::CommSpawn {
        dispatch_intent: explicit_intent,
        ..
    } = explicit
    else {
        panic!("expected CommSpawn");
    };
    let Request::CommSpawn {
        dispatch_intent: fallback_intent,
        ..
    } = fallback
    else {
        panic!("expected CommSpawn");
    };
    assert_eq!(explicit_intent, fallback_intent);
    assert!(explicit_intent.is_some());
}

#[test]
fn legacy_spawn_omits_dispatch_intent_and_schema_excludes_trusted_fields() {
    let params: CommunicateInput = serde_json::from_value(json!({"action": "spawn"}))
        .expect("legacy shape should deserialize");
    let request = params
        .build_spawn_request("coord", None, None, None)
        .expect("legacy request should build");
    let json = serde_json::to_string(&request).expect("request should serialize");
    assert!(!json.contains("dispatch_intent"));

    let schema = CommunicateTool::new().parameters_schema();
    let intent_schema = &schema["properties"]["dispatch_intent"];
    assert_eq!(intent_schema["additionalProperties"], false);
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
        assert!(intent_schema["properties"][trusted_field].is_null());
    }
}

#[test]
fn producer_rejects_trusted_dispatch_intent_fields() {
    let parsed = serde_json::from_value::<CommunicateInput>(json!({
        "action": "spawn",
        "dispatch_intent": {"mission_id": "forged"}
    }));
    assert!(
        parsed.is_err(),
        "unknown trusted fields must be rejected before a spawn request is produced"
    );
}
