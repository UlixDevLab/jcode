//! RED gate tests for opaque element references and navigation epochs.
//!
//! These tests assert that `ElementRef` is an opaque broker-issued handle
//! bound to an instance id, page id, and navigation epoch. Forged or stale
//! refs must fail before any browser action runs.

use jcode_browser_broker::refs::{
    ElementRef, InstanceId, NavigationEpoch, OpaqueId, PageId, RefError, RefRegistry,
};

#[test]
fn registry_issues_unique_opaque_ids() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let a = reg.issue_opaque();
    let b = reg.issue_opaque();
    assert_ne!(a.opaque_id, b.opaque_id);
}

#[test]
fn validate_accepts_fresh_ref() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let r = reg.issue_opaque();
    assert_eq!(reg.validate(&r), Ok(()));
}

#[test]
fn validate_rejects_forged_instance_id() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let mut r = reg.issue_opaque();
    r.instance_id = InstanceId::new();
    assert_eq!(reg.validate(&r), Err(RefError::UnknownInstance));
}

#[test]
fn validate_rejects_forged_page_id() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let mut r = reg.issue_opaque();
    r.page_id = PageId::new();
    assert_eq!(reg.validate(&r), Err(RefError::UnknownPage));
}

#[test]
fn validate_rejects_forged_opaque_id() {
    let reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let forged = ElementRef {
        instance_id: reg.instance_id(),
        page_id: reg.page_id(),
        navigation_epoch: reg.navigation_epoch(),
        opaque_id: OpaqueId::new(),
    };
    assert_eq!(reg.validate(&forged), Err(RefError::UnknownOpaque));
}

#[test]
fn validate_rejects_stale_navigation_epoch() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let r = reg.issue_opaque();
    let stale = r.navigation_epoch;
    reg.bump_navigation_epoch();
    let current = reg.navigation_epoch();
    assert_eq!(
        reg.validate(&r),
        Err(RefError::StaleNavigationEpoch { stale, current })
    );
}

#[test]
fn bump_navigation_epoch_invalidates_every_previous_ref() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    let stale: Vec<ElementRef> = (0..5).map(|_| reg.issue_opaque()).collect();
    reg.bump_navigation_epoch();
    let current = reg.navigation_epoch();
    for r in stale {
        let expected = RefError::StaleNavigationEpoch {
            stale: r.navigation_epoch,
            current,
        };
        assert_eq!(reg.validate(&r), Err(expected));
    }
}

#[test]
fn refs_after_bump_can_be_validated() {
    let mut reg = RefRegistry::new(InstanceId::new(), PageId::new(), NavigationEpoch::new());
    reg.bump_navigation_epoch();
    let r = reg.issue_opaque();
    assert_eq!(reg.validate(&r), Ok(()));
}

#[test]
fn instance_page_epoch_accessors_round_trip() {
    let i = InstanceId::new();
    let p = PageId::new();
    let e = NavigationEpoch::new();
    let reg = RefRegistry::new(i, p, e);
    assert_eq!(reg.instance_id(), i);
    assert_eq!(reg.page_id(), p);
    assert_eq!(reg.navigation_epoch(), e);
}
