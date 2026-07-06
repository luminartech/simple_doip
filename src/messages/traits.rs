use super::MessageError;

/// TX-side trait: encode a value into an [`embedded_io::Write`] implementor.
pub trait Encode {
    /// Number of bytes this value will write.
    fn encoded_size(&self) -> usize;

    /// Serialize into `writer`, returning the number of bytes written.
    ///
    /// # Errors
    /// Returns [`MessageError::Io`] if the writer fails.
    fn encode(&self, writer: &mut impl embedded_io::Write) -> Result<usize, MessageError>;
}

/// RX-side trait: zero-copy decode from a byte slice. The decoded value may borrow
/// from `buf` and is valid only as long as `buf` lives.
pub trait Decode<'a>: Sized {
    /// Decode from `buf`, returning `(value, remaining_bytes)`.
    ///
    /// # Errors
    /// Returns an error if `buf` is too short or contains invalid data.
    fn decode(buf: &'a [u8]) -> Result<(Self, &'a [u8]), MessageError>;

    /// Decode from `buf`, requiring the entire buffer to be consumed.
    ///
    /// # Errors
    /// Returns [`MessageError::TrailingBytes`] if bytes remain after decoding.
    fn decode_exact(buf: &'a [u8]) -> Result<Self, MessageError> {
        let (value, rest) = Self::decode(buf)?;
        if rest.is_empty() {
            Ok(value)
        } else {
            Err(MessageError::TrailingBytes { count: rest.len() })
        }
    }
}
