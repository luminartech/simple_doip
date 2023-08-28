pub mod alive_check_response;
pub mod diagnostic_message;
pub mod diagnostic_message_ack;
pub mod entity_status_response;
pub mod header;
pub mod message_error;
pub mod nack;
pub mod power_mode_info_response;
pub mod routing_activation_request;
pub mod routing_activation_response;
pub mod vehicle_identification_response;

use std::io::Write;

use crate::messages::{header::DoIPHeader, message_error::DoIPMessageError};

pub struct DoIPMessage {
    pub header: header::DoIPHeader,
    pub payload: Vec<u8>,
}

impl DoIPMessage {
    // TODO: This needs careful review and should do a lot more error checking than it does now
    pub fn read(message_bytes: &mut [u8]) -> Result<DoIPMessage, DoIPMessageError> {
        let header = DoIPHeader::read(&mut message_bytes.as_ref())?;
        header.version_inverse_correct()?;
        let payload = message_bytes[8..].to_vec();
        if header.payload_length != payload.len() as u32 {
            return Err(DoIPMessageError::PayloadLengthIncorrect {
                value: payload.len(),
                expected: header.payload_length,
            });
        }

        Ok(DoIPMessage { header, payload })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        self.header.write(writer)?;
        let payload_len = writer.write(&self.payload)?;
        assert!(payload_len == self.header.payload_length as usize);
        return Ok(self.payload.len() + 8);
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use header::{PayloadType, ProtocolVersion};

    /// Check that we properly decode and encode hex bytes
    #[test]
    fn test_valid_messages() {
        let mut buf: Vec<u8> = vec![
            0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        let deserialized_message = DoIPMessage::read(&mut buf).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 8);
        buf = vec![
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message = DoIPMessage::read(&mut buf).unwrap();
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2010);
        assert!(
            deserialized_message.header.payload_type == PayloadType::VehicleIdentificationRequest
        );
        assert!(deserialized_message.header.payload_length == 7);
    }
    #[test]
    fn test_invalid_inverse() {
        let mut buf: Vec<u8> = vec![0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // This parsing should panic for the bad inverse
        // TODO: Instead of panicking need to add error handling and return a result
        assert!(DoIPMessage::read(&mut buf).is_err());
    }
}
