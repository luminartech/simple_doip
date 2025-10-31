use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

use super::message_error::MessageError;

///Identifies whether or not the vehicle is in diagnostic power mode and ready to perform reliable diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum DiagnosticPowerModeCode {
    NotReady = 0x00,
    Ready = 0x01,
    NotSupported = 0x02,
    Reserved(u8),
}

impl From<u8> for DiagnosticPowerModeCode {
    fn from(value: u8) -> Self {
        use DiagnosticPowerModeCode::*;
        match value {
            0x00 => NotReady,
            0x01 => Ready,
            0x02 => NotSupported,
            _ => Reserved(value),
        }
    }
}
impl From<DiagnosticPowerModeCode> for u8 {
    fn from(value: DiagnosticPowerModeCode) -> Self {
        use DiagnosticPowerModeCode::*;
        match value {
            NotReady => 0x00,
            Ready => 0x01,
            NotSupported => 0x02,
            Reserved(value) => value,
        }
    }
}

impl DiagnosticPowerModeCode {
    pub fn read<T: Read>(reader: &mut T) -> Result<Self, MessageError> {
        Ok(reader.read_u8()?.into())
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, MessageError> {
        writer.write_u8((*self).into())?;
        Ok(1)
    }
}
