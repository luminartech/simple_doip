use core::fmt;
use core::fmt::UpperHex;

use crate::LogicalAddress;

use super::decode_util::{read_array, read_u8, read_u16_be};
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
            ActivationTypeCode::CentralSecurity => 0xE0,
            ActivationTypeCode::Reserved(value)
            | ActivationTypeCode::VehicleManufacturerSpecific(value) => value,
        }
    }
}
impl UpperHex for ActivationTypeCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val: u8 = (*self).into();
        write!(f, "{val:02X}")
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct RoutingActivationRequest {
    /// Address of `DoIP` entity that requests routing activation.
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

impl<'a> Decode<'a> for RoutingActivationRequest {
    /// Deserialize a routing activation request from a byte slice
    ///
    /// The optional manufacturer-specific tail is decoded when at least 4 bytes remain
    /// after the fixed fields.
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (source_address, rest) = read_u16_be(buf)?;
        let (activation_type, rest) = read_u8(rest)?;
        let activation_type = ActivationTypeCode::from(activation_type);

        let (reserved, rest) = read_array::<4>(rest)?;

        let (reserved_vehicle_manufacturer, rest) = if rest.len() >= 4 {
            let (value, rest) = read_array::<4>(rest)?;
            (Some(value), rest)
        } else {
            (None, rest)
        };

        Ok((
            Self {
                source_address: LogicalAddress(source_address),
                activation_type,
                reserved,
                reserved_vehicle_manufacturer,
            },
            rest,
        ))
    }
}

impl Encode for RoutingActivationRequest {
    fn encoded_size(&self) -> usize {
        if self.reserved_vehicle_manufacturer.is_some() {
            11
        } else {
            7
        }
    }

    /// Serialize this routing activation request into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    // TODO: Investigate if we should write the optional vehicle manufacturer specific data if none
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        writer
            .write_all(&u16::to_be_bytes(self.source_address.into()))
            .map_err(MessageError::io)?;
        writer
            .write_all(&[self.activation_type.into()])
            .map_err(MessageError::io)?;
        writer.write_all(&self.reserved).map_err(MessageError::io)?;
        if let Some(reserved_vehicle_manufacturer) = self.reserved_vehicle_manufacturer {
            writer
                .write_all(&reserved_vehicle_manufacturer)
                .map_err(MessageError::io)?;
            return Ok(11);
        }
        Ok(7)
    }
}
