use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use crate::error::DoIPError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationTypeCode {
    /// ISO 14229
    Default,
    /// Diagnostic communication required by regulation
    /// (e.g. ISO 27145, the ISO 20730 series, etc.)
    RegulationRequired,
    /// ISO/SAE reserved
    Reserved(u8),
    /// Vehicle manufacturer specific authentication,
    CentralSecurity,
    /// Available for additional VM-specific use
    VehicleManufacturerSpecific(u8),
}

impl From<u8> for ActivationTypeCode {
    fn from(value: u8) -> Self {
        match value {
            0x00 => ActivationTypeCode::Default,
            0x01 => ActivationTypeCode::RegulationRequired,
            0x02..=0xDF => ActivationTypeCode::Reserved(value),
            0xE0 => ActivationTypeCode::CentralSecurity,
            0xE1..=0xFF => ActivationTypeCode::VehicleManufacturerSpecific(value),
        }
    }
}

impl From<ActivationTypeCode> for u8 {
    fn from(value: ActivationTypeCode) -> Self {
        match value {
            ActivationTypeCode::Default => 0x00,
            ActivationTypeCode::RegulationRequired => 0x01,
            ActivationTypeCode::Reserved(value) => value,
            ActivationTypeCode::CentralSecurity => 0xE0,
            ActivationTypeCode::VehicleManufacturerSpecific(value) => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutingActivationRequest {
    /// Address of DoIP entity that requests routing activation.
    pub source_address: [u8; 2],
    pub activation_type: ActivationTypeCode,
    pub reserved: [u8; 4],
    pub reserved_vehicle_manufacturer: Option<[u8; 4]>,
}

impl RoutingActivationRequest {
    pub(crate) fn read<T: Read>(reader: &mut T) -> Result<Self, DoIPError> {
        let mut source_address = [0x00; 2];
        reader.read_exact(&mut source_address)?;
        let activation_type = ActivationTypeCode::from(reader.read_u8()?);

        let mut reserved = [0x00; 4];
        reader.read_exact(&mut reserved)?;

        let reserved_vehicle_manufacturer = None; // TODO

        Ok(Self {
            source_address,
            activation_type,
            reserved,
            reserved_vehicle_manufacturer,
        })
    }

    pub(crate) fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPError> {
        writer.write_all(&self.source_address)?;
        writer.write_u8(self.activation_type.into())?;
        writer.write_all(&self.reserved)?;
        if let Some(reserved_vehicle_manufacturer) = self.reserved_vehicle_manufacturer {
            writer.write_all(&reserved_vehicle_manufacturer)?;
        }
        Ok(())
    }
}
