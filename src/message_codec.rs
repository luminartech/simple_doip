use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::messages::{header::DoIPHeader, message_error::DoIPMessageError, DoIPMessage};

pub struct DoIPMessageCodec;

impl Decoder for DoIPMessageCodec {
    type Item = DoIPMessage;
    type Error = DoIPMessageError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 8 {
            return Ok(None);
        }
        // Read length marker.
        let mut header_bytes = [0u8; 8];
        header_bytes.copy_from_slice(&src[..8]);
        let header = DoIPHeader::read(&mut header_bytes.as_ref())?;
        if header.payload_length + 8 > src.len() as u32 {
            return Ok(None);
        }
        Ok(Some(DoIPMessage::read(src)?))
    }
}

impl Encoder<&DoIPMessage> for DoIPMessageCodec {
    type Error = DoIPMessageError;

    fn encode(&mut self, message: &DoIPMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        message.write(&mut dst.writer())?;
        Ok(())
    }
}
