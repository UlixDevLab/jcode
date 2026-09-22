//! RED gate tests for redacted action receipts.
//!
//! Receipts are audit output only. They must serialize only origin and
//! action metadata, never URL path / query, headers, request / response
//! bodies, credentials, or capability material.

use jcode_browser_broker::origin::{LocalOrigin, LoopbackAddress};
use jcode_browser_broker::receipt::{
    ActionReceipt, BoundaryTrip, LocalBrowserAction, Reason, RedactionError,
};
use jcode_browser_broker::refs::NavigationEpoch;
use std::net::Ipv4Addr;

fn sample_origin() -> LocalOrigin {
    LocalOrigin {
        address: LoopbackAddress::V4(Ipv4Addr::new(127, 0, 0, 1)),
        port: 8080,
    }
}

fn build(action: LocalBrowserAction) -> ActionReceipt {
    ActionReceipt::new(
        uuid::Uuid::new_v4(),
        action,
        sample_origin(),
        NavigationEpoch::new(),
        None,
    )
}

#[test]
fn receipt_serializes_redacted_projection() {
    let receipt = build(LocalBrowserAction::Click {
        target: "ref:abc".to_string(),
    });
    let value = serde_json::to_value(&receipt).expect("serialize");
    assert_eq!(value["action_id"], receipt.action_id.to_string());
    assert_eq!(value["action"], "click");
    assert_eq!(value["target"], "ref");
    assert!(value.get("boundary_trip").is_none());
}

#[test]
fn receipt_serializes_origin_only_no_path_query_fragment() {
    let receipt = build(LocalBrowserAction::Navigate {
        from: "/private-path".to_string(),
        to: "/also-private?secret=1#frag".to_string(),
    });
    // The redaction contract strips path/query/fragment from the
    // serialized form. The raw action fields are NOT serialized.
    let json = serde_json::to_string(&receipt).expect("serialize");
    assert!(!json.contains("/private-path"), "leaked path: {json}");
    assert!(!json.contains("/also-private"), "leaked path: {json}");
    assert!(!json.contains("secret=1"), "leaked query: {json}");
    assert!(!json.contains("#frag"), "leaked fragment: {json}");
}

#[test]
fn receipt_serializes_no_credentials_no_headers_no_body() {
    let receipt = build(LocalBrowserAction::Type {
        target: "ref:abc".to_string(),
        text: "super-secret-password".to_string(),
    });
    let json = serde_json::to_string(&receipt).expect("serialize");
    assert!(
        !json.contains("super-secret-password"),
        "leaked text: {json}"
    );
    assert!(
        !json.contains("Authorization"),
        "leaked header name: {json}"
    );
    assert!(!json.contains("Cookie"), "leaked header name: {json}");
}

#[test]
fn action_debug_redacts_sensitive_payloads_and_targets() {
    let action = LocalBrowserAction::Type {
        target: "ref:private-target".to_string(),
        text: "super-secret-password".to_string(),
    };
    let debug = format!("{action:?}");
    assert_eq!(debug, "LocalBrowserAction { class: \"type\" }");
    assert!(!debug.contains("private-target"));
    assert!(!debug.contains("super-secret-password"));
}

#[test]
fn receipt_serializes_no_capability_material() {
    let receipt = ActionReceipt::new(
        uuid::Uuid::new_v4(),
        LocalBrowserAction::Status,
        sample_origin(),
        NavigationEpoch::new(),
        Some(BoundaryTrip {
            reason: Reason::RedirectToPublicOrigin,
        }),
    );
    let json = serde_json::to_string(&receipt).expect("serialize");
    assert!(
        !json.contains("capability"),
        "leaked capability name: {json}"
    );
    assert!(!json.contains("secret"), "leaked secret name: {json}");
    assert!(!json.contains("socket"), "leaked socket path: {json}");
    assert!(!json.contains("Bearer"), "leaked bearer token: {json}");
}

#[test]
fn redactor_strips_path_query_and_fragment() {
    let sanitized = ActionReceipt::redact_url_path("/secret?q=1#x").expect("redact");
    assert_eq!(sanitized, "/*");
}

#[test]
fn redactor_rejects_unredactable_payload() {
    let err = ActionReceipt::redact_url_path("not-a-slash-prefixed-thing")
        .expect_err("must reject path that does not start with /");
    assert!(matches!(err, RedactionError::InvalidShape));
}

#[test]
fn boundary_trip_reason_serializes_as_snake_case_string() {
    let trip = BoundaryTrip {
        reason: Reason::CrossOriginSubresource,
    };
    let json = serde_json::to_string(&trip).expect("serialize");
    assert!(json.contains("\"cross_origin_subresource\""), "got {json}");
}
