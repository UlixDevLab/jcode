//! Contract tests for the broker's in-memory authentication capability.

use jcode_browser_broker::capability::BrokerCapability;

#[test]
fn capability_debug_is_a_fixed_redaction_marker() {
    let capability = BrokerCapability::generate().expect("OS randomness is available");

    assert_eq!(format!("{capability:?}"), "<redacted-capability>");
}
