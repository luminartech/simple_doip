use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::messages::{message_error::DoIPMessageError, DoIPMessage};

struct DoIPMessageCodec;

impl Decoder for DoIPMessageCodec {
    type Item = DoIPMessage;
    type Error = DoIPMessageError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
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
