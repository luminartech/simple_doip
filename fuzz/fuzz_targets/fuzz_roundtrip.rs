#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Decode, Encode, Message};

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

    // The payload, not the whole message: `encode` derives the header's
    // declared length from the payload, so a frame that arrived claiming a
    // length its payload did not occupy comes back with the real one. That
    // normalization is the fix for #15 -- asserting full `Message` equality
    // here would assert the bug back into existence.
    assert_eq!(
        message.payload, reparsed.payload,
        "round trip changed the payload"
    );
    assert_eq!(
        message.header.payload_type, reparsed.header.payload_type,
        "round trip changed the payload type"
    );

    // Encoding is idempotent: having normalized once, a second pass must
    // produce the very same bytes. This is the property that would catch a
    // field order asymmetry, now that the length no longer masks it.
    let mut again = vec![0u8; reparsed.encoded_size().expect("size of a decoded message")];
    {
        let mut writer: &mut [u8] = &mut again;
        reparsed
            .encode(&mut writer)
            .expect("re-encode must not fail");
    }
    assert_eq!(encoded, again, "encoding is not idempotent");
});
