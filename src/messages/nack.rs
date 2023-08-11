#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegativeAckCode {
    IncorrectPatternFormat,
    UnknownPayloadType,
    MessageTooLarge,
    OutOfMemory,
    InvalidPayloadLength,
    Reserved(u8),
}
