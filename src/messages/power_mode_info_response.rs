use std::io::{Read, Write};

use byteorder::{ReadBytesExt, WriteBytesExt};

use crate::error::DoIPError;

///Identifies whether or not the vehicle is in diagnostic power mode and ready to perform reliable diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticPowerModeCode {
    NotReady,
    Ready,
    NotSupported,
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
    pub(crate) fn read<T: Read>(reader: &mut T) -> Result<Self, DoIPError> {
        Ok(reader.read_u8()?.into())
    }

    pub(crate) fn write<T: Write>(&self, writer: &mut T) -> Result<(), DoIPError> {
        writer.write_u8((*self).into())?;
        Ok(())
    }
}
