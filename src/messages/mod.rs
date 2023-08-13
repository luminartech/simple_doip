pub mod header;
pub mod nack;
pub mod routing_activation_request;
pub mod vehicle_identification_response;

use header::DoIpHeader;

pub struct DoIPMessage {
    pub header: header::DoIpHeader,
    pub payload: Vec<u8>,
}

pub struct DoIPParser {}

impl DoIPParser {
    pub fn parse_doip_message(message_bytes: &mut [u8]) -> DoIPMessage {
        let header = DoIpHeader::read(&mut message_bytes.as_ref());
        assert!(header.version_inverse_correct());
        assert!(header.payload_length == (message_bytes.len() - 8) as u32);
        let payload = message_bytes[8..].to_vec();
        DoIPMessage { header, payload }
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
        let deserialized_message = DoIPParser::parse_doip_message(&mut buf);
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2012);
        assert!(deserialized_message.header.payload_type == PayloadType::NegativeAcknowledge);
        assert!(deserialized_message.header.payload_length == 8);
        assert!(deserialized_message.payload.len() == 8);
        buf = vec![
            0x01, 0xFE, 0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let deserialized_message = DoIPParser::parse_doip_message(&mut buf);
        assert!(deserialized_message.header.protocol_version == ProtocolVersion::V2010);
        assert!(
            deserialized_message.header.payload_type == PayloadType::VehicleIdentificationRequest
        );
        assert!(deserialized_message.header.payload_length == 7);
        assert!(deserialized_message.payload.len() == 7);
    }
    #[test]
    #[should_panic]
    fn test_invalid_inverse() {
        let mut buf: Vec<u8> = vec![0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        // This parsing should panic for the bad inverse
        // TODO: Instead of panicking need to add error handling and return a result
        let _deserialized_message = DoIPParser::parse_doip_message(&mut buf);
    }
}
