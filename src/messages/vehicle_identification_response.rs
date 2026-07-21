use crate::logical_address::LogicalAddress;

use automotive_wire_codec::{read_array, read_u8, read_u16_be, write_all, write_u8, write_u16_be};

use super::message_error::MessageError;
use super::traits::{Decode, Encode};

/// Whether the tester must take further action before diagnostics can proceed
/// with this vehicle (ISO 13400-2 §7.1, `VIR`/vehicle announcement further-action
/// byte).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FurtherActionRequired {
    /// No further action needed; the tester may proceed directly to routing
    /// activation.
    NoFurtherActionRequired,
    /// A further-action value outside the range this crate models.
    Reserved(u8),
    /// The tester must send a routing activation request with a `CentralSecurity`
    /// activation type before diagnostics can proceed.
    RoutingActivationRequiredToInitiateCentralSecurity,
    /// Vehicle-manufacturer-specific further-action value.
    VehicleManufacturerSpecific(u8),
}

impl From<u8> for FurtherActionRequired {
    fn from(value: u8) -> Self {
        match value {
            0x00 => FurtherActionRequired::NoFurtherActionRequired,
            0x01..=0x0F => FurtherActionRequired::Reserved(value),
            0x10 => FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity,
            0x11..=0xFF => FurtherActionRequired::VehicleManufacturerSpecific(value),
        }
    }
}

impl From<FurtherActionRequired> for u8 {
    fn from(value: FurtherActionRequired) -> Self {
        match value {
            FurtherActionRequired::NoFurtherActionRequired => 0x00,
            FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity => 0x10,
            FurtherActionRequired::Reserved(value)
            | FurtherActionRequired::VehicleManufacturerSpecific(value) => value,
        }
    }
}

/// Whether the VIN and group ID (GID) are synchronized across all `DoIP` entities
/// in the vehicle (ISO 13400-2 §7.1). Relevant when a vehicle has multiple `DoIP`
/// gateways that must agree on identification data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VinGidSyncStatus {
    /// VIN and/or GID are synchronized
    Synchronized,
    /// A sync-status value outside the range this crate models.
    Reserved(u8),
    /// VIN and GID are NOT synchronized
    Incomplete,
}

impl From<u8> for VinGidSyncStatus {
    fn from(value: u8) -> Self {
        match value {
            0x00 => VinGidSyncStatus::Synchronized,
            0x10 => VinGidSyncStatus::Incomplete,
            // 0x01..=0x0F and 0x11..=0xFF
            _ => VinGidSyncStatus::Reserved(value),
        }
    }
}

impl From<VinGidSyncStatus> for u8 {
    fn from(value: VinGidSyncStatus) -> Self {
        match value {
            VinGidSyncStatus::Synchronized => 0x00,
            VinGidSyncStatus::Incomplete => 0x10,
            VinGidSyncStatus::Reserved(value) => value,
        }
    }
}

/// Vehicle identification response / Vehicle announcement
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VehicleIdentificationResponse {
    /// Vehicle Identification Number
    pub vin: [u8; 17],
    /// Logical address of the vehicle's `DoIP` gateway or the responding ECU.
    pub logical_address: LogicalAddress,
    /// Unique entity id, e.g. MAC address of network interface.
    pub entity_id: [u8; 6],
    //// Unique group identification of entities within a vehicle.
    /// None when value not set (as indicated by `0x00` or `0xFF`).
    pub group_id: Option<[u8; 6]>,
    /// Whether the tester must take further action (e.g. routing activation with
    /// central security) before diagnostics can proceed.
    pub further_action: FurtherActionRequired,
    /// Indicates whether all entities have synced information about VIN or GID.
    pub vin_gid_sync_status: VinGidSyncStatus,
}

impl<'a> Decode<'a> for VehicleIdentificationResponse {
    type Error = MessageError;

    /// Deserialize a vehicle identification response from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::Incomplete`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (vin, rest) = read_array::<17>(buf)?;

        let (logical_address, rest) = read_u16_be(rest)?;

        let (entity_id, rest) = read_array::<6>(rest)?;

        let (group_id, rest) = read_array::<6>(rest)?;

        // Table 1 - value not set
        let group_id = if group_id == [0x00; 6] || group_id == [0xFF; 6] {
            None
        } else {
            Some(group_id)
        };

        let (further_action, rest) = read_u8(rest)?;
        let further_action = FurtherActionRequired::from(further_action);

        let (vin_gid_sync_status, rest) = read_u8(rest)?;
        let vin_gid_sync_status = VinGidSyncStatus::from(vin_gid_sync_status);

        Ok((
            Self {
                vin,
                logical_address: LogicalAddress(logical_address),
                entity_id,
                group_id,
                further_action,
                vin_gid_sync_status,
            },
            rest,
        ))
    }
}

impl Encode for VehicleIdentificationResponse {
    type Error = MessageError;

    fn encoded_size(&self) -> Result<usize, MessageError> {
        Ok(33)
    }

    /// Serialize this vehicle identification response into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_all(writer, &self.vin)?;
        write_u16_be(writer, self.logical_address.into())?;
        write_all(writer, &self.entity_id)?;
        if let Some(group_id) = self.group_id {
            write_all(writer, &group_id)?;
        } else {
            write_all(writer, &[0x00; 6])?;
        }
        write_u8(writer, self.further_action.into())?;
        write_u8(writer, self.vin_gid_sync_status.into())?;
        Ok(33)
    }
}
