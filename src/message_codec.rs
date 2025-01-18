use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use uds_protocol::SingleValueWireFormat;

use crate::messages::{DoIPHeader, DoIPMessage, DoIPMessageError, Payload};

pub struct DoIPMessageCodec<DiagnosticsDefinition> {
    _diagnostics_definition: std::marker::PhantomData<DiagnosticsDefinition>,
}

impl<DiagnosticsDefinition> DoIPMessageCodec<DiagnosticsDefinition> {
    pub fn new() -> Self {
        Self {
            _diagnostics_definition: std::marker::PhantomData,
        }
    }
}

impl<DiagnosticsDefinition: SingleValueWireFormat> Decoder
    for DoIPMessageCodec<DiagnosticsDefinition>
{
    type Item = DoIPMessage<DiagnosticsDefinition>;
    type Error = DoIPMessageError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 8 {
            return Ok(None);
        }
        // Peel off the header from the rx buffer
        let mut header_bytes = [0u8; 8];
        header_bytes.copy_from_slice(&src[0..8]);
        if let Ok(header) = DoIPHeader::read(&mut header_bytes.as_slice()) {
            let message_length = header.payload_length as usize + 8;
            if message_length > src.len() {
                // We haven't received the full message yet, put the header back
                Ok(None)
            } else {
                // Drop these, we already copied them into the header
                let _ = src.split_to(8);
                // We have the full message, split off the payload from the rx buffer
                let payload_bytes = src.split_to(header.payload_length as usize);
                let payload = Payload::<DiagnosticsDefinition>::read(
                    &mut payload_bytes.as_ref(),
                    header.payload_type,
                )?;
                Ok(Some(DoIPMessage { header, payload }))
            }
        } else {
            // We don't have a valid header, put the header back
            println!("{:X}", src);
            Ok(None)
        }
    }
}

impl<DiagnosticsDefinition: SingleValueWireFormat> Encoder<&DoIPMessage<DiagnosticsDefinition>>
    for DoIPMessageCodec<DiagnosticsDefinition>
{
    type Error = DoIPMessageError;

    fn encode(
        &mut self,
        message: &DoIPMessage<DiagnosticsDefinition>,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        message.write(&mut dst.writer())?;
        Ok(())
    }
}
