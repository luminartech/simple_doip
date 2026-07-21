//! `DoIP` logical addressing ([`LogicalAddress`]), the identifier space used to
//! address testers, ECUs, and gateways on a `DoIP` network (ISO 13400-2 §7.8).

use core::fmt::{Debug, Display, LowerHex, UpperHex};
use tracing::info;

#[derive(Clone, Copy, Eq)]
/// Logical addressing is used to identify the ECU
///
/// A physical logical address uniquely represents a diagnostic application
/// layer entity within any `DoIP` entity or on any server of the in-vehicle networks
/// connected via `DoIP` gateways. See ISO 13400-2 section 7.8
pub struct LogicalAddress(
    /// The 16-bit address value, as transmitted on the wire.
    pub u16,
);

impl LogicalAddress {
    /// Lower bound of the logical address range reserved for external test equipment
    /// (testers), per ISO 13400-2 Table 5. Addresses below this range are reserved
    /// for other entity classes (e.g. `DoIP` gateways, ECUs).
    pub const MIN_CLIENT_ADDRESS: LogicalAddress = LogicalAddress(0x0E00);
    /// Upper bound of the logical address range reserved for external test equipment
    /// (testers), per ISO 13400-2 Table 5.
    pub const MAX_CLIENT_ADDRESS: LogicalAddress = LogicalAddress(0x0FFF);

    /// Sub-range of client addresses reserved for internal on-board diagnostics (OBD)
    /// tooling rather than general external testers, per ISO 13400-2 Table 5
    /// (0x0F00-0x0F7F). A client address in this range is still valid, but
    /// [`is_valid_client_address`](Self::is_valid_client_address) logs a warning
    /// since this crate's use cases are external testers, not OBD tooling.
    pub const OBD_ADDRESS_RANGE: (LogicalAddress, LogicalAddress) =
        (LogicalAddress(0x0F00), LogicalAddress(0x0F7F));

    /// Verify if the logical address is within the valid range for a client address
    /// of 0x0E00 - 0x0FFF
    pub fn is_valid_client_address(&self) -> bool {
        if *self >= Self::MIN_CLIENT_ADDRESS && *self <= Self::MAX_CLIENT_ADDRESS {
            // Check if the logical address is in the OBD range
            // For now we just log info to the user since this is a valid address,
            // but it is not recommended to use this range for client addresses
            // and is not in the use case of the crate at this time
            if *self >= Self::OBD_ADDRESS_RANGE.0 && *self <= Self::OBD_ADDRESS_RANGE.1 {
                info!(
                    "Logical addresses in the 0xF000-0xF7F range are intended for internal \
                data collection/on-board diagnotics only. Ensure that this is the intended use case."
                );
            }
            true
        } else {
            false
        }
    }
}

impl Display for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#06X}", self.0)
    }
}
impl Debug for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:#06X}", self.0)
    }
}
impl UpperHex for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        UpperHex::fmt(&self.0, f)
    }
}
impl LowerHex for LogicalAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        LowerHex::fmt(&self.0, f)
    }
}
impl From<u16> for LogicalAddress {
    fn from(addr: u16) -> Self {
        LogicalAddress(addr)
    }
}
impl From<LogicalAddress> for u16 {
    fn from(addr: LogicalAddress) -> Self {
        addr.0
    }
}
impl PartialOrd<u16> for LogicalAddress {
    fn partial_cmp(&self, other: &u16) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}
impl PartialOrd<LogicalAddress> for LogicalAddress {
    fn partial_cmp(&self, other: &LogicalAddress) -> Option<core::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}
impl PartialEq<u16> for LogicalAddress {
    fn eq(&self, other: &u16) -> bool {
        self.0 == *other
    }
}
impl PartialEq<LogicalAddress> for LogicalAddress {
    fn eq(&self, other: &LogicalAddress) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logical_address() {
        let addr = LogicalAddress(0x0E00);
        assert!(addr.is_valid_client_address());
        assert_eq!(addr.0, 0x0E00);
        assert_eq!(addr, LogicalAddress(0x0E00));
        assert_eq!(addr, 0x0E00);
    }
}
