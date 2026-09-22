//! Opaque element references and navigation epochs.
//!
//! `ElementRef` is a broker-issued handle bound to a single
//! `(instance_id, page_id, navigation_epoch)` tuple. Forged or stale
//! handles fail validation before any browser action runs.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

/// Identifies a single broker process.
///
/// Re-generated every time a broker is spawned, including after port
/// reuse invalidates a previous broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(Uuid);

impl InstanceId {
    /// Generate a fresh random instance id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Identifies a single Chromium page / target inside one broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageId(Uuid);

impl PageId {
    /// Generate a fresh random page id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Increments on every navigation so stale refs are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NavigationEpoch(Uuid);

impl NavigationEpoch {
    /// Generate a fresh random navigation epoch.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// The opaque per-element part of an [`ElementRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpaqueId(Uuid);

impl OpaqueId {
    /// Generate a fresh random opaque id.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Opaque broker-issued handle for a specific element on a specific page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ElementRef {
    /// Broker instance that issued this handle.
    pub instance_id: InstanceId,
    /// Page / target the handle is bound to.
    pub page_id: PageId,
    /// Navigation epoch under which the handle was issued.
    pub navigation_epoch: NavigationEpoch,
    /// Opaque per-element id.
    pub opaque_id: OpaqueId,
}

/// Validation errors for [`ElementRef`]s.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RefError {
    /// The `instance_id` does not match this broker.
    #[error("element ref instance id does not match this broker")]
    UnknownInstance,
    /// The `page_id` is not registered with this broker.
    #[error("element ref page id is not registered")]
    UnknownPage,
    /// The `opaque_id` was not issued by this broker.
    #[error("element ref opaque id was not issued by this broker")]
    UnknownOpaque,
    /// The `navigation_epoch` is older than the current epoch.
    #[error("element ref is stale (navigation epoch does not match the current broker epoch)")]
    StaleNavigationEpoch {
        /// Stale epoch observed on the ref.
        stale: NavigationEpoch,
        /// Current epoch on the broker.
        current: NavigationEpoch,
    },
}

/// Test-friendly reference registry.
///
/// In production (M2/M3) this lives inside the broker process and is
/// queried on every action. In M1 the registry is exposed so tests can
/// drive the validation contract directly without spawning a real
/// broker.
#[derive(Debug)]
pub struct RefRegistry {
    instance_id: InstanceId,
    page_id: PageId,
    navigation_epoch: NavigationEpoch,
    issued: HashSet<OpaqueId>,
}

impl RefRegistry {
    /// Build a registry bound to the supplied instance / page / epoch.
    pub fn new(
        instance_id: InstanceId,
        page_id: PageId,
        navigation_epoch: NavigationEpoch,
    ) -> Self {
        Self {
            instance_id,
            page_id,
            navigation_epoch,
            issued: HashSet::new(),
        }
    }

    /// Return the broker instance id.
    pub fn instance_id(&self) -> InstanceId {
        self.instance_id
    }

    /// Return the page id.
    pub fn page_id(&self) -> PageId {
        self.page_id
    }

    /// Return the current navigation epoch.
    pub fn navigation_epoch(&self) -> NavigationEpoch {
        self.navigation_epoch
    }

    /// Issue a fresh [`ElementRef`] and remember its `opaque_id`.
    pub fn issue_opaque(&mut self) -> ElementRef {
        let opaque_id = OpaqueId::new();
        self.issued.insert(opaque_id);
        ElementRef {
            instance_id: self.instance_id,
            page_id: self.page_id,
            navigation_epoch: self.navigation_epoch,
            opaque_id,
        }
    }

    /// Bump the navigation epoch; every previously issued ref is now
    /// stale.
    pub fn bump_navigation_epoch(&mut self) {
        self.navigation_epoch = NavigationEpoch::new();
        self.issued.clear();
    }

    /// Validate a ref against this registry.
    pub fn validate(&self, r: &ElementRef) -> Result<(), RefError> {
        if r.instance_id != self.instance_id {
            return Err(RefError::UnknownInstance);
        }
        if r.page_id != self.page_id {
            return Err(RefError::UnknownPage);
        }
        if r.navigation_epoch != self.navigation_epoch {
            return Err(RefError::StaleNavigationEpoch {
                stale: r.navigation_epoch,
                current: self.navigation_epoch,
            });
        }
        if !self.issued.contains(&r.opaque_id) {
            return Err(RefError::UnknownOpaque);
        }
        Ok(())
    }
}
