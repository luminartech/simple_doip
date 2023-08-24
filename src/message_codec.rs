use bytes::{Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::messages::{
    header::DoIPHeader, message_error::DoIPMessageError, DoIPMessage, DoIPParser,
};

struct DoIPMessageCodec;

impl Decoder for DoIPMessageCodec {
    type Item = DoIPMessage;
    type Error = DoIPMessageError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(Some(DoIPParser::parse_doip_message(src)?))
    }
}

impl Encoder<(&DoIPMessage, &[u8])> for DoIPMessageCodec {
    type Error = DoIPMessageError;

    fn encode(
        &mut self,
        message: (&DoIPMessage, &[u8]),
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        message.write(&mut dst.writer())?;
        dst.put(payload);
        Ok(())
    }
}
