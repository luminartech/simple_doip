use std::io::{Read, Write};

use super::{
    alive_check_response::AliveCheckResponse, header::PayloadType, message_error::DoIPMessageError,
};

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Payload {
    AliveCheckResponse(AliveCheckResponse),
}

impl Payload {
    pub fn read<T: Read>(
        mut payload_bytes: &mut T,
        payload_type: PayloadType,
    ) -> Result<Self, DoIPMessageError> {
        match payload_type {
            PayloadType::AliveCheckResponse => {
                let alive_check_response = AliveCheckResponse::read(&mut payload_bytes)?;
                Ok(Self::AliveCheckResponse(alive_check_response))
            }
            _ => Err(DoIPMessageError::UnexpectedPayloadType(payload_type)),
        }
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<usize, DoIPMessageError> {
        match self {
            Self::AliveCheckResponse(alive_check_response) => alive_check_response.write(writer),
        }
    }
}
