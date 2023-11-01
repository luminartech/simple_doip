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
        // Peel off the header from the rx buffer
        let header_bytes = src.split_to(8);
        let copied_bytes = header_bytes.clone();
        if let Ok(header) = DoIPHeader::read(&mut copied_bytes.as_ref()) {
            if header.payload_length as usize > src.len() {
                // We haven't received the full message yet, put the header back
                src.unsplit(header_bytes);
                Ok(None)
            } else {
                // We have the full message, split off the payload from the rx buffer
                let payload = src.split_to(header.payload_length as usize);

                Ok(Some(DoIPMessage {
                    header,
                    payload: payload.to_vec(),
                }))
            }
        } else {
            println!("{:X}", header_bytes);
            // We don't have a valid header, put the header back
            src.unsplit(header_bytes);
            println!("{:X}", src);
            return Ok(None);
        }
    }
}

impl Encoder<&DoIPMessage> for DoIPMessageCodec {
    type Error = DoIPMessageError;

    fn encode(&mut self, message: &DoIPMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        message.write(&mut dst.writer())?;
        Ok(())
    }
}
