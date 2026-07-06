//! Sans-io framer: extract complete `DoIP` messages from a byte buffer without owning
//! any I/O resource itself.

use crate::messages::{Decode, Header, MessageError, MessageRef};

/// Try to extract one complete `DoIP` message from the front of `buf`.
///
/// Returns `Ok(None)` if `buf` does not yet contain a complete message (read more
/// bytes and call again). On success returns the decoded message (borrowing from
/// `buf`) and the total number of bytes consumed; the caller advances its buffer by
/// that amount.
///
/// # Errors
/// Returns a [`MessageError`] if `buf` contains a malformed header (e.g. an incorrect
/// inverse protocol version) or payload.
pub fn try_frame(buf: &[u8]) -> Result<Option<(MessageRef<'_>, usize)>, MessageError> {
    if buf.len() < Header::SIZE {
        return Ok(None);
    }
    let (header, _) = Header::decode(&buf[..Header::SIZE])?;
    let total = Header::SIZE + header.payload_length as usize;
    if buf.len() < total {
        return Ok(None);
    }
    let (message, _) = MessageRef::decode(&buf[..total])?;
    Ok(Some((message, total)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::MessageError;

    #[test]
    fn incomplete_header_returns_none() {
        // Fewer than Header::SIZE (8) bytes available.
        let buf: [u8; 5] = [0x02, 0xFD, 0x00, 0x00, 0x00];
        assert!(matches!(try_frame(&buf), Ok(None)));
    }

    #[test]
    fn complete_nack_frame_is_decoded() {
        // 8-byte header + 1-byte NACK payload = 9 bytes total.
        let buf: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
        let (message, consumed) = try_frame(&buf).unwrap().unwrap();
        assert_eq!(consumed, 9);
        assert_eq!(
            message.header.payload_type,
            crate::messages::PayloadType::NegativeAcknowledge
        );
    }

    #[test]
    fn corrupt_inverse_errors() {
        let buf: [u8; 8] = [0x01, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(matches!(
            try_frame(&buf),
            Err(MessageError::VersionInverseIncorrect { .. })
        ));
    }
}
