//! RED gate tests for the `BrokerHandle` stub.
//!
//! `BrokerHandle` is the M1 placeholder for the M2 runtime handle. It must
//! never be `Serialize` (the per-process capability is sensitive), it must
//! carry the exact pinned loopback origin, and its debug output must not
//! leak capability material.

use jcode_browser_broker::handle::BrokerHandle;
use jcode_browser_broker::origin::{LocalOrigin, LoopbackAddress};
use std::net::Ipv4Addr;

fn origin() -> LocalOrigin {
    LocalOrigin {
        address: LoopbackAddress::V4(Ipv4Addr::new(127, 0, 0, 1)),
        port: 8080,
    }
}

#[test]
fn handle_exposes_pinned_origin_and_instance() {
    let handle = BrokerHandle::stub_for_tests(origin());
    assert_eq!(handle.origin(), &origin());
    assert_eq!(handle.origin().port, 8080);
}

#[test]
fn handle_debug_redacts_capability_value() {
    let handle = BrokerHandle::stub_for_tests(origin());
    let dbg = format!("{handle:?}");
    // The capability value is redacted; the marker is required.
    assert!(
        dbg.contains("<redacted-capability>"),
        "expected redacted marker, got {dbg}"
    );
    assert!(!dbg.contains("Bearer"), "leaked bearer token: {dbg}");
    assert!(!dbg.contains("token="), "leaked token: {dbg}");
}
