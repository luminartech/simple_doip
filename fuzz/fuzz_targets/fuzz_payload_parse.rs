#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Payload, PayloadType};

// Fuzz payload deserialization with arbitrary payload types.
//
// Uses the first two bytes to select a PayloadType, then feeds the
// remaining bytes to `Payload::read()`. This exercises all payload
// parsing branches independently of header validation.
fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let payload_type_raw = u16::from_be_bytes([data[0], data[1]]);
    let payload_type = PayloadType::from(payload_type_raw);
    let payload_bytes = &data[2..];
    let _ = Payload::read(&mut &payload_bytes[..], payload_type);
});
