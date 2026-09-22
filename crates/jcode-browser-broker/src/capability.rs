//! Secret capability material for a single broker process.
//!
//! The bytes are intentionally unavailable outside this crate. Future IPC may
//! authenticate a fixed-size frame with [`BrokerCapability::matches`], but no
//! public formatter, serializer, accessor, or clone operation can export it.

use std::fmt;

use subtle::ConstantTimeEq as _;
use thiserror::Error;
use zeroize::Zeroize as _;

/// The in-memory capability required to authenticate a broker connection.
///
/// ```compile_fail
/// use jcode_browser_broker::capability::BrokerCapability;
/// fn require_clone<T: Clone>() {}
/// require_clone::<BrokerCapability>();
/// ```
///
/// ```compile_fail
/// use jcode_browser_broker::capability::BrokerCapability;
/// fn require_serialize<T: serde::Serialize>() {}
/// require_serialize::<BrokerCapability>();
/// ```
///
/// ```compile_fail
/// use jcode_browser_broker::capability::BrokerCapability;
/// fn require_deserialize<T: serde::de::DeserializeOwned>() {}
/// require_deserialize::<BrokerCapability>();
/// ```
///
/// ```compile_fail
/// use jcode_browser_broker::capability::BrokerCapability;
/// fn require_display<T: std::fmt::Display>() {}
/// require_display::<BrokerCapability>();
/// ```
pub struct BrokerCapability {
    bytes: [u8; Self::LENGTH],
}

impl BrokerCapability {
    /// The exact length of every generated capability.
    pub const LENGTH: usize = 32;

    /// Generates a capability from the operating system's random source.
    pub fn generate() -> Result<Self, CapabilityError> {
        let mut bytes = [0_u8; Self::LENGTH];
        getrandom::fill(&mut bytes).map_err(|_| CapabilityError::OsRandomness)?;
        Ok(Self { bytes })
    }

    /// Compares an IPC-sized candidate without exposing the capability.
    #[allow(dead_code)] // Reserved for the M2 IPC frame verifier.
    pub(crate) fn matches(&self, candidate: &[u8; Self::LENGTH]) -> bool {
        self.bytes.ct_eq(candidate).into()
    }

    fn erase(&mut self) {
        self.bytes.zeroize();
    }
}

impl Drop for BrokerCapability {
    fn drop(&mut self) {
        self.erase();
    }
}

impl fmt::Debug for BrokerCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted-capability>")
    }
}

/// Failure to obtain randomness without including sensitive generated data.
#[derive(Debug, Error)]
pub enum CapabilityError {
    /// The operating system could not provide cryptographically secure bytes.
    #[error("operating-system randomness is unavailable")]
    OsRandomness,
}

#[cfg(test)]
mod tests {
    use super::BrokerCapability;

    #[test]
    fn drop_overwrites_every_capability_byte() {
        let mut capability = std::mem::ManuallyDrop::new(BrokerCapability {
            bytes: [0xA5; BrokerCapability::LENGTH],
        });

        // SAFETY: `ManuallyDrop` suppresses the automatic destructor. The
        // capability contains only bytes, which remain valid to inspect after
        // its destructor has overwritten them.
        unsafe { std::ptr::drop_in_place(&mut *capability) };

        assert_eq!(capability.bytes, [0; BrokerCapability::LENGTH]);
    }

    #[test]
    fn matcher_accepts_only_identical_fixed_length_candidates() {
        let capability = BrokerCapability { bytes: [0xA5; 32] };

        assert!(capability.matches(&[0xA5; 32]));
        assert!(!capability.matches(&[0x5A; 32]));
    }
}
