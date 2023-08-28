use std::io::{Read, Write};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use super::message_error::DoIPMessageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            FurtherActionRequired::Reserved(value) => value,
            FurtherActionRequired::RoutingActivationRequiredToInitiateCentralSecurity => 0x10,
            FurtherActionRequired::VehicleManufacturerSpecific(value) => value,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VehicleIdentificationResponse {
    /// Vehicle Identification Number
    pub vin: [u8; 17],
    pub logical_address: u16,
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
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, DoIPMessageError> {
        let mut vin = [0x00; 17];
        reader.read_exact(&mut vin)?;

        let mut logical_address = reader.read_u16::<BigEndian>()?;

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

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        writer.write_all(&self.vin)?;
        writer.write_u16::<BigEndian>(self.logical_address)?;
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
