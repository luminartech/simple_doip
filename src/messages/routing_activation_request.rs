use core::fmt;
use std::{
    fmt::UpperHex,
    io::{Read, Write},
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use crate::LogicalAddress;

use super::message_error::MessageError;

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
impl UpperHex for ActivationTypeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val: u8 = (*self).into();
        let val = format!("{val:02X}");
        f.write_str(&val)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RoutingActivationRequest {
    /// Address of DoIP entity that requests routing activation.
    pub source_address: LogicalAddress,
    pub activation_type: ActivationTypeCode,
    pub reserved: [u8; 4],
    pub reserved_vehicle_manufacturer: Option<[u8; 4]>,
}

impl fmt::Debug for RoutingActivationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RoutingActivationRequest")
            .field(
                "source_address",
                &format_args!("{:#X}", &self.source_address),
            )
            .field("activation_type", &self.activation_type)
            .field("reserved", &self.reserved)
            .field(
                "reserved_vehicle_manufacturer",
                &self.reserved_vehicle_manufacturer,
            )
            .field(
                "raw",
                &format_args!("{:#X} {:#X}", &self.source_address, &self.activation_type),
            )
            .finish()
    }
}

impl RoutingActivationRequest {
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        let source_address = LogicalAddress(reader.read_u16::<BigEndian>()?);
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
    // TODO: Investigate if we should write the optional vehicle manufacturer specific data if none
    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u16::<BigEndian>(self.source_address.into())?;
        writer.write_u8(self.activation_type.into())?;
        writer.write_all(&self.reserved)?;
        if let Some(reserved_vehicle_manufacturer) = self.reserved_vehicle_manufacturer {
            writer.write_all(&reserved_vehicle_manufacturer)?;
            return Ok(11);
        }
        Ok(7)
    }
}
