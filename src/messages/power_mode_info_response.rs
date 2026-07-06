use super::decode_util::read_u8;
use super::encode_util::write_u8;
use super::message_error::MessageError;
use super::traits::{Decode, Encode};

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
        match value {
            0x00 => DiagnosticPowerModeCode::NotReady,
            0x01 => DiagnosticPowerModeCode::Ready,
            0x02 => DiagnosticPowerModeCode::NotSupported,
            _ => DiagnosticPowerModeCode::Reserved(value),
        }
    }
}
impl From<DiagnosticPowerModeCode> for u8 {
    fn from(value: DiagnosticPowerModeCode) -> Self {
        match value {
            DiagnosticPowerModeCode::NotReady => 0x00,
            DiagnosticPowerModeCode::Ready => 0x01,
            DiagnosticPowerModeCode::NotSupported => 0x02,
            DiagnosticPowerModeCode::Reserved(value) => value,
        }
    }
}

impl<'a> Decode<'a> for DiagnosticPowerModeCode {
    /// Deserialize a diagnostic power mode code from a byte slice
    ///
    /// # Errors
    /// Returns [`MessageError::InsufficientData`] if `buf` is too short
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError> {
        let (value, rest) = read_u8(buf)?;
        Ok((value.into(), rest))
    }
}

impl Encode for DiagnosticPowerModeCode {
    fn encoded_size(&self) -> usize {
        1
    }

    /// Serialize this diagnostic power mode code into `writer`
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError> {
        write_u8(writer, (*self).into())?;
        Ok(1)
    }
}
