use super::MessageError;

pub(crate) fn take(buf: &[u8], n: usize) -> Result<(&[u8], &[u8]), MessageError> {
    if buf.len() < n {
        return Err(MessageError::InsufficientData {
            needed: n,
            available: buf.len(),
        });
    }
    Ok(buf.split_at(n))
}

pub(crate) fn read_u8(buf: &[u8]) -> Result<(u8, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 1)?;
    Ok((bytes[0], rest))
}

pub(crate) fn read_u16_be(buf: &[u8]) -> Result<(u16, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 2)?;
    Ok((u16::from_be_bytes([bytes[0], bytes[1]]), rest))
}

pub(crate) fn read_u32_be(buf: &[u8]) -> Result<(u32, &[u8]), MessageError> {
    let (bytes, rest) = take(buf, 4)?;
    Ok((
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        rest,
    ))
}

pub(crate) fn read_array<const N: usize>(buf: &[u8]) -> Result<([u8; N], &[u8]), MessageError> {
    let (bytes, rest) = take(buf, N)?;
    let mut array = [0u8; N];
    array.copy_from_slice(bytes);
    Ok((array, rest))
}
