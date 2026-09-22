//! Broker handle stub (M1 placeholder for the M2 runtime handle).
//!
//! `BrokerHandle` is the M2-owned handle to a single broker process. M1
//! only needs the type shape and the non-`Serialize` guarantee so the
//! crate compiles and the redaction contract is testable. The M2
//! implementation will replace [`BrokerHandle::stub_for_tests`] with a
//! `spawn` constructor that:
//!
//!   1. creates a private `0700` runtime directory,
//!   2. generates a per-process random capability,
//!   3. passes the capability through an inherited pipe,
//!   4. spawns the broker child with `--remote-debugging-pipe`,
//!   5. remembers the pinned [`LocalOrigin`] and [`InstanceId`].
//!
//! `BrokerHandle` deliberately does **not** implement `Serialize`:
//! the per-process capability is sensitive and must never leave the
//! parent process via JSON, log line, error report, or audit pipeline.

use uuid::Uuid;

use crate::origin::LocalOrigin;

/// Opaque, non-serializable per-process capability string.
///
/// The actual byte material lands here in M2; in M1 the field is
/// initialized to a deterministic placeholder so the struct has
/// well-defined behavior under `Debug` and the redaction contract is
/// observable.
pub struct BrokerCapability {
    #[allow(dead_code)]
    bytes: Vec<u8>,
}

impl std::fmt::Debug for BrokerCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted-capability>")
    }
}

impl BrokerCapability {
    fn placeholder() -> Self {
        Self { bytes: Vec::new() }
    }
}

/// Handle to one broker process.
///
/// The struct intentionally has no public fields: every accessor
/// returns a borrow or copy so the capability string cannot be smuggled
/// out through a public API. The M2 release will populate the
/// [`BrokerHandle::stub_for_tests`] slot with a real `spawn`
/// constructor.
///
/// ```compile_fail
/// use jcode_browser_broker::handle::BrokerHandle;
/// fn require_serialize<T: serde::Serialize>() {}
/// require_serialize::<BrokerHandle>();
/// ```
#[derive(Debug)]
pub struct BrokerHandle {
    origin: LocalOrigin,
    instance_id: Uuid,
    #[allow(dead_code)]
    capability: BrokerCapability,
}

impl BrokerHandle {
    /// Build an M1 stub handle for tests / type-driven callers.
    ///
    /// This will be replaced by `Broker::spawn(LocalOrigin) -> BrokerHandle`
    /// in M2.
    pub fn stub_for_tests(origin: LocalOrigin) -> Self {
        Self {
            origin,
            instance_id: Uuid::new_v4(),
            capability: BrokerCapability::placeholder(),
        }
    }

    /// Pinned loopback origin for this broker.
    pub fn origin(&self) -> &LocalOrigin {
        &self.origin
    }

    /// Unique broker instance id.
    pub fn instance_id(&self) -> Uuid {
        self.instance_id
    }
}
