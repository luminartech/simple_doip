#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Payload, PayloadType};

// Payload decoding for an arbitrary payload type, independent of header
// validation: the first two bytes choose the `PayloadType`, the rest are the
// body. That reaches decode branches a valid header would never select --
// a response type arriving where a request belongs, for instance.
fuzz_target!(|data: &[u8]| {
    let Some((type_bytes, payload_bytes)) = data.split_at_checked(2) else {
        return;
    };
    let payload_type = PayloadType::from(u16::from_be_bytes([type_bytes[0], type_bytes[1]]));
    let _ = Payload::decode(payload_bytes, payload_type);
});
