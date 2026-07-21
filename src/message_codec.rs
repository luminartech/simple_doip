use crate::messages::{Encode, Message, MessageError, OwnedMessage, Payload};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

/// Codec for the `DoIP` messages, used to encode and decode messages from
/// the TCP stream
#[derive(Debug, Default)]
pub struct MessageCodec {
    // No phantom data needed since we're no longer generic
}

impl MessageCodec {
    /// Create a new `DoIP` message codec
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

impl Decoder for MessageCodec {
    type Item = OwnedMessage;
    type Error = MessageError;

    /// Decode one `DoIP` message from `src`.
    ///
    /// Frames whose header is valid but whose body cannot be decoded are handled
    /// according to [`MessageError::is_framing_fatal`]: a RECOVERABLE body error (e.g. an
    /// unmodeled payload type) is **skipped** — the frame is consumed and decoding
    /// continues with the next one, so one unsupported message does not tear down the
    /// connection. A truncated body yields [`MessageError::Incomplete`], which is
    /// classified framing-FATAL (stream sync cannot be trusted), so it propagates to the
    /// caller instead of being skipped, and the buffer is left untouched for the caller
    /// to decide how to proceed.
    ///
    /// # Errors
    /// Returns a [`MessageError`] when framing fails fatally and the connection must be
    /// closed.
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            let Some((frame, consumed)) = crate::try_frame(src.as_ref())? else {
                return Ok(None);
            };
            let decoded = Payload::decode(frame.payload, frame.header.payload_type).map(|payload| {
                Message {
                    header: frame.header,
                    payload,
                }
                .to_owned_message()
            });
            match decoded {
                Ok(owned) => {
                    let _ = src.split_to(consumed);
                    return Ok(Some(owned));
                }
                Err(e) if e.is_framing_fatal() => return Err(e),
                Err(e) => {
                    // Recoverable: the header was sound, so `consumed` is trustworthy.
                    // Drop this frame and resync on the next one.
                    let _ = src.split_to(consumed);
                    tracing::debug!("skipping undecodable DoIP frame: {e}");
                }
            }
        }
    }
}

impl Encoder<&OwnedMessage> for MessageCodec {
    type Error = MessageError;
    fn encode(&mut self, message: &OwnedMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let size = message.encoded_size()?;
        dst.reserve(size);
        let mut out = std::vec::Vec::with_capacity(size);
        message.encode(&mut out)?;
        dst.extend_from_slice(&out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::OwnedPayload;

    /// Well-formed NACK frame: 8-byte header (V2012, payload type 0x0000, length 1) plus
    /// a 1-byte body.
    const NACK_FRAME: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];

    /// Valid header, but payload type 0x9999 is unmodeled. Framing succeeds; the body
    /// decode fails with a RECOVERABLE `UnsupportedPayloadType`.
    const UNSUPPORTED_FRAME: [u8; 9] = [0x02, 0xFD, 0x99, 0x99, 0x00, 0x00, 0x00, 0x01, 0x00];

    /// Corrupt inverse protocol version (0xFE, expected 0xFD): framing-FATAL.
    const CORRUPT_HEADER: [u8; 8] = [0x02, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

    #[test]
    fn partial_frame_returns_none_and_leaves_buffer_intact() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&NACK_FRAME[..4]);
        assert!(codec.decode(&mut src).unwrap().is_none());
        assert_eq!(src.len(), 4, "a partial frame must not be consumed");
    }

    #[test]
    fn two_frames_in_one_buffer_both_decode() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&NACK_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        let first = codec.decode(&mut src).unwrap().expect("first frame decodes");
        assert!(matches!(first.payload, OwnedPayload::DoIPNack(_)));
        let second = codec.decode(&mut src).unwrap().expect("second frame decodes");
        assert!(matches!(second.payload, OwnedPayload::DoIPNack(_)));
        assert!(src.is_empty(), "both frames should be consumed");
    }

    /// The regression this task exists for: a recoverable body error must skip the bad
    /// frame and keep the stream alive, not tear down the `FramedRead`.
    #[test]
    fn unsupported_payload_type_is_skipped_not_fatal() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::new();
        src.extend_from_slice(&UNSUPPORTED_FRAME);
        src.extend_from_slice(&NACK_FRAME);

        let decoded = codec
            .decode(&mut src)
            .expect("a recoverable body error must not surface as a Decoder error")
            .expect("the following valid frame should decode");
        assert!(matches!(decoded.payload, OwnedPayload::DoIPNack(_)));
        assert!(src.is_empty(), "both the skipped and the valid frame are consumed");
    }

    /// A framing-fatal error still propagates: stream sync is lost and the connection
    /// must be torn down.
    #[test]
    fn corrupt_header_is_fatal() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&CORRUPT_HEADER[..]);
        let err = codec.decode(&mut src).unwrap_err();
        assert!(err.is_framing_fatal(), "got a non-fatal error: {err:?}");
        assert!(matches!(err, MessageError::VersionInverseIncorrect { .. }));
    }

    /// Header declares a 2-byte NACK body but only 1 body byte is present. This pins the
    /// `Err(e) if e.is_framing_fatal() => return Err(e)` arm in `decode` specifically -
    /// distinct from `corrupt_header_is_fatal`, which exercises the `?` on
    /// `crate::try_frame` and never reaches `Payload::decode` at all. Here framing
    /// succeeds (the header is well-formed) and it is `Payload::decode` itself that
    /// returns a framing-fatal `MessageError::Incomplete`, which `decode` must propagate
    /// rather than skip.
    const TRUNCATED_BODY_FRAME: [u8; 9] = [0x02, 0xFD, 0x00, 0x05, 0x00, 0x00, 0x00, 0x01, 0x03];

    #[test]
    fn truncated_body_is_fatal_via_classifier() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&TRUNCATED_BODY_FRAME[..]);
        let err = codec.decode(&mut src).unwrap_err();
        assert!(err.is_framing_fatal(), "got a non-fatal error: {err:?}");
        assert!(matches!(err, MessageError::Incomplete { .. }));
    }

    #[test]
    fn unsupported_payload_alone_yields_none() {
        let mut codec = MessageCodec::new();
        let mut src = BytesMut::from(&UNSUPPORTED_FRAME[..]);
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "skipping the only frame leaves nothing to return"
        );
        assert!(src.is_empty(), "the skipped frame is still consumed");
    }
}
