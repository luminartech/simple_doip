use std::fmt::Debug;

/// A trait for types that can be serialized to and deserialized from a byte stream.
pub trait WireFormat: Sized {
    /// Deserialize a value from a byte stream.
    /// Returns `Ok(Some(value))` if the stream contains a complete value.
    /// Returns `Ok(None)` if the stream is empty.
    fn decode<T: std::io::Read>(reader: &mut T) -> Result<Option<Self>, std::io::Error>;

    /// Returns the number of bytes required to serialize this value.
    fn required_size(&self) -> usize;

    /// Serialize a value to a byte stream.
    /// Returns the number of bytes written.
    fn encode<T: std::io::Write>(&self, writer: &mut T) -> Result<usize, std::io::Error>;
}

pub trait WirePayload: WireFormat + Send + Debug {}

impl<T: WireFormat + Debug + Send> WirePayload for T {}
