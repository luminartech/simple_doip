use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationType {
    /// ISO 14229
    Default,
    /// Diagnostic communication required by regulation (e.g. ISO 27145, the ISO 20730 series, etc.)
    RegulationRequired,
    /// ISO/SAE reserved
    Reserved(u8),
    /// Vehicle manufacturer specific authentication,
    CentralSecurity,
    /// Available for additional VM-specific use
    VehicleManufacturerSpecific(u8),
}

impl From<u8> for ActivationType {
    fn from(value: u8) -> Self {
        match value {
            0x00 => ActivationType::Default,
            0x01 => ActivationType::RegulationRequired,
            0x02..=0xDF => ActivationType::Reserved(value),
            0xE0 => ActivationType::CentralSecurity,
            0xE1..=0xFF => ActivationType::VehicleManufacturerSpecific(value),
        }
    }
}

impl From<ActivationType> for u8 {
    fn from(value: ActivationType) -> Self {
        match value {
            ActivationType::Default => 0x00,
            ActivationType::RegulationRequired => 0x01,
            ActivationType::Reserved(value) => value,
            ActivationType::CentralSecurity => 0xE0,
            ActivationType::VehicleManufacturerSpecific(value) => value,
        }
    }
}

impl ActivationType {
    pub fn read<T: Read>(reader: &mut T) -> ActivationType {
        reader.read_u8().unwrap().into()
    }
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_u8((*self).into()).unwrap();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingActivationRequest {
    /// Address of DoIP entity that requests routing activation.
    pub source_address: [u8; 2],
    pub activation_type: ActivationType,
    pub reserved: [u8; 4],
    pub reserved_vehicle_manufacturer: Option<[u8; 4]>,
}

impl RoutingActivationRequest {
    pub fn write<T: Write>(&self, writer: &mut T) {
        writer.write_all(&self.source_address).unwrap();
        writer.write_u8(self.activation_type.into()).unwrap();
        writer.write_all(&self.reserved).unwrap();
        if let Some(reserved_vehicle_manufacturer) = self.reserved_vehicle_manufacturer {
            writer.write_all(&reserved_vehicle_manufacturer).unwrap();
        }
    }

    pub fn read<T: Read>(reader: &mut T) -> Self {
        let mut source_address = [0x00; 2];
        reader.read_exact(&mut source_address).unwrap();

        let activation_type_raw: u8 = reader.read_u8().unwrap();
        let activation_type = ActivationType::try_from(activation_type_raw).unwrap();

        let mut reserved = [0x00; 4];
        reader.read_exact(&mut reserved).unwrap();

        let reserved_vehicle_manufacturer = None; // TODO

        Self {
            source_address,
            activation_type,
            reserved,
            reserved_vehicle_manufacturer,
        }
    }
}
