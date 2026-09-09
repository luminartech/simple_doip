#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Decode, Encode, Header, Message};

// The asymmetry hunt: if a frame decodes, re-encoding it and decoding that
// again must yield the same message.
//
//   decode(encode(decode(bytes))) == decode(bytes)
//
// A field written in one order and read in another survives a hand-written
// round-trip test whenever both sides share the mistake, but not this -- the
// re-encoded bytes have to be acceptable to the decoder that produced them.
fuzz_target!(|data: &[u8]| {
    let Ok((message, _rest)) = Message::decode(data) else {
        return;
    };

    let Ok(size) = message.encoded_size() else {
        return;
    };

    // Skip frames whose header declares a length the payload does not actually
    // occupy. `decode` accepts those, keeps the declared length verbatim, and
    // `encode` then writes it beside a payload of the real size -- emitting a
    // frame no decoder will accept. That is luminartech/simple_doip#15, not an
    // asymmetry in the field order this target is hunting for, and the check
    // comes out when #15 is fixed.
    if message.header.payload_length as usize != size - Header::SIZE {
        return;
    }
    let mut encoded = vec![0u8; size];
    {
        let mut writer: &mut [u8] = &mut encoded;
        if message.encode(&mut writer).is_err() {
            return;
        }
    }

    let (reparsed, rest) =
        Message::decode(&encoded).expect("re-decoding an encoded message must not fail");
    assert!(rest.is_empty(), "re-encoding left trailing bytes");
    assert_eq!(message, reparsed, "round trip changed the message");
});
