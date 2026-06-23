use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::logical_address::LogicalAddress;

use super::message_error::MessageError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FurtherActionRequired {
    NoFurtherActionRequired,
    Reserved(u8),
    RoutingActivationRequiredToInitiateCentralSecurity,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VinGidSyncStatus {
    /// VIN and/or GID are synchronized
    Synchronized,
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
    pub logical_address: LogicalAddress,
    /// Unique entity id, e.g. MAC address of network interface.
    pub entity_id: [u8; 6],
    //// Unique group identification of entities within a vehicle.
    /// None when value not set (as indicated by `0x00` or `0xFF`).
    pub group_id: Option<[u8; 6]>,
    pub further_action: FurtherActionRequired,
    /// Indicates whether all entities have synced information about VIN or GID.
    pub vin_gid_sync_status: VinGidSyncStatus,
}

impl VehicleIdentificationResponse {
    /// Deserialize a vehicle identification response from a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be read
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let mut vin = [0x00; 17];
        reader.read_exact(&mut vin)?;

        let logical_address = LogicalAddress(reader.read_u16::<BigEndian>()?);

        let mut entity_id = [0x00; 6];
        reader.read_exact(&mut entity_id)?;

        let mut group_id = [0x00; 6];
        reader.read_exact(&mut group_id)?;

        // Table 1 - value not set
        let group_id = if group_id == [0x00; 6] || group_id == [0xFF; 6] {
            None
        } else {
            Some(group_id)
        };

        let further_action_byte = reader.read_u8()?;
        let further_action = FurtherActionRequired::from(further_action_byte);

        let vin_gid_sync_status_byte = reader.read_u8()?;
        let vin_gid_sync_status = VinGidSyncStatus::from(vin_gid_sync_status_byte);

        Ok(Self {
            vin,
            logical_address,
            entity_id,
            group_id,
            further_action,
            vin_gid_sync_status,
        })
    }

    /// Serialize this vehicle identification response to a byte stream
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the byte stream cannot be written
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_all(&self.vin)?;
        writer.write_u16::<BigEndian>(self.logical_address.into())?;
        writer.write_all(&self.entity_id)?;
        if let Some(group_id) = self.group_id {
            writer.write_all(&group_id)?;
        } else {
            writer.write_all(&[0x00; 6])?;
        }
        writer.write_u8(self.further_action.into())?;
        writer.write_u8(self.vin_gid_sync_status.into())?;
        Ok(33)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogicalAddress;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_further_action_roundtrip(byte in any::<u8>()) {
            let code = FurtherActionRequired::from(byte);
            let back: u8 = code.into();
            prop_assert_eq!(byte, back);
        }

        #[test]
        fn prop_vin_gid_sync_roundtrip(byte in any::<u8>()) {
            let code = VinGidSyncStatus::from(byte);
            let back: u8 = code.into();
            prop_assert_eq!(byte, back);
        }

        #[test]
        fn prop_vehicle_identification_response_roundtrip(
            vin in any::<[u8; 17]>(),
            addr in any::<u16>(),
            entity_id in any::<[u8; 6]>(),
            group_id_bytes in any::<[u8; 6]>().prop_filter(
                "avoid all-zeros and all-0xFF which map to None",
                |g| *g != [0x00; 6] && *g != [0xFF; 6]
            ),
            further_byte in any::<u8>(),
            sync_byte in any::<u8>(),
        ) {
            let resp = VehicleIdentificationResponse {
                vin,
                logical_address: LogicalAddress(addr),
                entity_id,
                group_id: Some(group_id_bytes),
                further_action: FurtherActionRequired::from(further_byte),
                vin_gid_sync_status: VinGidSyncStatus::from(sync_byte),
            };
            let mut buf = Vec::new();
            resp.write(&mut buf).unwrap();

            let parsed = VehicleIdentificationResponse::read(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(resp, parsed);
        }

        #[test]
        fn prop_vehicle_identification_response_none_group_roundtrip(
            vin in any::<[u8; 17]>(),
            addr in any::<u16>(),
            entity_id in any::<[u8; 6]>(),
            further_byte in any::<u8>(),
            sync_byte in any::<u8>(),
        ) {
            let resp = VehicleIdentificationResponse {
                vin,
                logical_address: LogicalAddress(addr),
                entity_id,
                group_id: None,
                further_action: FurtherActionRequired::from(further_byte),
                vin_gid_sync_status: VinGidSyncStatus::from(sync_byte),
            };
            let mut buf = Vec::new();
            resp.write(&mut buf).unwrap();

            let parsed = VehicleIdentificationResponse::read(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(resp, parsed);
        }
    }
}
