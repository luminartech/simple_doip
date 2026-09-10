//! `Message::encode` must never emit a frame `Message::decode` rejects.
//!
//! Regression tests for the asymmetry the `fuzz_roundtrip` target found:
//! `decode` takes exactly `header.payload_length` bytes and lets
//! `Payload::decode` consume fewer without complaint, so a decoded `Message`
//! could carry a declared length its payload did not occupy. `encode` wrote
//! that stale field verbatim, producing a frame that failed to decode with
//! `Incomplete`.
//!
//! Both inputs below are frames a peer can actually send. Neither is
//! well-formed, but both are accepted, and being accepted is what put them on
//! the re-encode path.

use simple_doip::messages::{Decode, Encode, Message, Payload, PayloadType};

/// Encode `message` into a fresh buffer sized by `encoded_size`.
fn encode(message: &Message<'_>) -> Vec<u8> {
    let mut buf = vec![0u8; message.encoded_size().expect("encoded_size failed")];
    {
        let mut writer: &mut [u8] = &mut buf;
        message.encode(&mut writer).expect("encode failed");
    }
    buf
}

/// A NACK body is one byte. This frame's header claims five.
#[test]
fn a_fixed_size_payload_with_an_overlong_declared_length_still_round_trips() {
    let framed: [u8; 13] = [
        0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x03, 0x00, 0x00, 0x00, 0x00,
    ];
    let (message, _rest) = Message::decode(&framed).expect("the frame is accepted as-is");
    assert_eq!(
        message.header.payload_length, 5,
        "decode still reports what the wire declared"
    );

    let encoded = encode(&message);
    let (reparsed, rest) = Message::decode(&encoded).expect("re-decode must succeed");
    assert!(rest.is_empty());
    assert_eq!(
        reparsed.header.payload_length, 1,
        "the encoded frame declares the length its payload actually occupies"
    );
    assert_eq!(message.payload, reparsed.payload);
}

/// `VehicleIdentificationRequest` is a unit variant -- an empty body. This
/// frame's header claims one byte, which `Payload::decode` discards.
#[test]
fn a_unit_payload_with_a_nonzero_declared_length_still_round_trips() {
    let framed: [u8; 9] = [0x00, 0xFF, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00];
    let (message, _rest) = Message::decode(&framed).expect("the frame is accepted as-is");
    assert_eq!(message.payload, Payload::VehicleIdentificationRequest);
    assert_eq!(message.header.payload_length, 1);

    let encoded = encode(&message);
    assert_eq!(encoded.len(), 8, "a unit payload encodes to a bare header");
    let (reparsed, rest) = Message::decode(&encoded).expect("re-decode must succeed");
    assert!(rest.is_empty());
    assert_eq!(reparsed.header.payload_length, 0);
    assert_eq!(
        reparsed.header.payload_type,
        PayloadType::VehicleIdentificationRequest
    );
}

/// A well-formed frame is untouched: the derived length equals the declared
/// one, so the bytes are identical. This is what keeps the golden vectors
/// valid.
#[test]
fn a_well_formed_frame_encodes_to_the_bytes_it_came_from() {
    let framed: [u8; 9] = [0x02, 0xFD, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x03];
    let (message, rest) = Message::decode(&framed).expect("decode");
    assert!(rest.is_empty());
    assert_eq!(encode(&message), framed);
}
