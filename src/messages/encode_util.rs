use super::MessageError;

/// Write a single byte, returning the number of bytes written (1).
///
/// # Errors
/// Returns [`MessageError::Io`] if the writer fails.
pub(crate) fn write_u8(
    writer: &mut impl embedded_io::Write,
    value: u8,
) -> Result<usize, MessageError> {
    writer.write_all(&[value]).map_err(MessageError::io)?;
    Ok(1)
}

/// Write a big-endian `u16`, returning the number of bytes written (2).
///
/// # Errors
/// Returns [`MessageError::Io`] if the writer fails.
pub(crate) fn write_u16_be(
    writer: &mut impl embedded_io::Write,
    value: u16,
) -> Result<usize, MessageError> {
    writer
        .write_all(&value.to_be_bytes())
        .map_err(MessageError::io)?;
    Ok(2)
}

/// Write a big-endian `u32`, returning the number of bytes written (4).
///
/// # Errors
/// Returns [`MessageError::Io`] if the writer fails.
pub(crate) fn write_u32_be(
    writer: &mut impl embedded_io::Write,
    value: u32,
) -> Result<usize, MessageError> {
    writer
        .write_all(&value.to_be_bytes())
        .map_err(MessageError::io)?;
    Ok(4)
}
