use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

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

impl FurtherActionRequired {
    pub fn read<T: Read>(reader: &mut T) -> FurtherActionRequired {
        reader.read_u8().unwrap().into()
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_u8((*self).into()).unwrap();
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

impl VinGidSyncStatus {
    pub fn read<T: Read>(reader: &mut T) -> VinGidSyncStatus {
        reader.read_u8().unwrap().into()
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_u8((*self).into()).unwrap();
    }
}

/// Vehicle identification response / Vehicle announcement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleIdentificationResponse {
    /// Vehicle Identification Number
    pub vin: [u8; 17],

    pub logical_address: [u8; 2],
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
    pub fn read<T: Read>(reader: &mut T) -> Self {
        let mut vin = [0x00; 17];
        reader.read_exact(&mut vin).unwrap();

        let mut logical_address = [0x00; 2];
        reader.read_exact(&mut logical_address).unwrap();

        let mut entity_id = [0x00; 6];
        reader.read_exact(&mut entity_id).unwrap();

        let mut group_id = [0x00; 6];
        reader.read_exact(&mut group_id).unwrap();

        // Table 1 - value not set
        let group_id = if group_id == [0x00; 6] || group_id == [0xFF; 6] {
            None
        } else {
            Some(group_id)
        };

        let further_action_byte = reader.read_u8().unwrap();
        let further_action = FurtherActionRequired::from(further_action_byte);

        let vin_gid_sync_status_byte = reader.read_u8().unwrap();
        let vin_gid_sync_status = VinGidSyncStatus::from(vin_gid_sync_status_byte);

        Self {
            vin,
            logical_address,
            entity_id,
            group_id,
            further_action,
            vin_gid_sync_status,
        }
    }

    pub fn write<T: std::io::Write>(&self, writer: &mut T) {
        writer.write_all(&self.vin).unwrap();
        writer.write_all(&self.logical_address).unwrap();
        writer.write_all(&self.entity_id).unwrap();
        if let Some(group_id) = self.group_id {
            writer.write_all(&group_id).unwrap();
        } else {
            writer.write_all(&[0x00; 6]).unwrap();
        }
        self.further_action.write(writer);
        self.vin_gid_sync_status.write(writer);
    }
}
