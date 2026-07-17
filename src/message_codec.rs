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

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match crate::try_frame(src.as_ref()) {
            Ok(Some((frame, consumed))) => {
                let payload = Payload::decode(frame.payload, frame.header.payload_type)?;
                let message = Message {
                    header: frame.header,
                    payload,
                };
                let owned = message.to_owned_message();
                let _ = src.split_to(consumed);
                Ok(Some(owned))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Encoder<&OwnedMessage> for MessageCodec {
    type Error = MessageError;
    fn encode(&mut self, message: &OwnedMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(message.encoded_size()?);
        let mut out = std::vec::Vec::with_capacity(message.encoded_size()?);
        message.encode(&mut out)?;
        dst.extend_from_slice(&out);
        Ok(())
    }
}
