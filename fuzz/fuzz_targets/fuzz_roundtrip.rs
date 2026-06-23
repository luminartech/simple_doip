#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::Message;

// Fuzz the encode/decode roundtrip property.
//
// If `Message::read()` successfully parses arbitrary bytes into a message,
// then serializing it back with `Message::write()` and re-parsing should
// produce an identical message. This tests the serialization invariant:
//
//   decode(encode(decode(bytes))) == decode(bytes)
//
// Any violation indicates a serialization/deserialization asymmetry.
fuzz_target!(|data: &[u8]| {
    // Try to parse the fuzzed input
    let Ok(message) = Message::read(&mut &data[..]) else {
        return;
    };

    // Serialize the parsed message back to bytes
    let mut encoded = Vec::new();
    if message.write(&mut encoded).is_err() {
        return;
    }

    // Re-parse the serialized bytes. This must succeed and produce
    // the same message.
    let reparsed = Message::read(&mut encoded.as_slice())
        .expect("re-parsing a serialized message must not fail");

    assert_eq!(
        message, reparsed,
        "roundtrip mismatch: original != re-parsed"
    );
});
