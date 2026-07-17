//! Bare-metal-style codec usage: no `alloc`, no `std` collections in the
//! message-handling path — everything lives on the stack.
//!
//! This example is compiled and run on the host (so `println!` is available
//! for reporting results), but the encode/decode/framing logic below only
//! touches the `core`-only API surface of `simple_doip`. It compiles cleanly
//! with `cargo build --example bare_metal_codec --no-default-features`.

use simple_doip::messages::{ActivationTypeCode, Encode, Message, Payload, ProtocolVersion};
use simple_doip::{LogicalAddress, try_frame};

fn main() {
    // --- Routing activation request ---------------------------------------
    let routing_request: Message<'_> = Message::routing_activation_request(
        ProtocolVersion::V2012,
        LogicalAddress(0x0E00),
        ActivationTypeCode::Default,
        None,
    );

    let mut routing_buf = [0u8; 64];
    let routing_written = {
        let mut writer: &mut [u8] = &mut routing_buf[..];
        routing_request
            .encode(&mut writer)
            .expect("encoding a routing activation request should not fail")
    };

    // Feed the bytes through the sans-io framer incrementally: a partial
    // slice must report "not enough data yet" rather than erroring.
    let partial = &routing_buf[..routing_written - 1];
    assert!(matches!(try_frame(partial), Ok(None)));

    let (routing_frame, routing_consumed) = try_frame(&routing_buf[..routing_written])
        .expect("framing a complete message should not fail")
        .expect("a complete message should be available");
    assert_eq!(routing_consumed, routing_written);
    let routing_payload =
        Payload::decode(routing_frame.payload, routing_frame.header.payload_type)
            .expect("decoding a routing activation request payload should not fail");
    let decoded_routing = Message {
        header: routing_frame.header,
        payload: routing_payload,
    };
    println!("Decoded routing activation request: {decoded_routing:?}");

    // --- Diagnostic message, built from a stack array -----------------------
    let user_data: [u8; 2] = [0x10, 0x02];
    let diagnostic_message: Message<'_> = Message::diagnostic_message(
        ProtocolVersion::V2012,
        LogicalAddress(0x0E00),
        LogicalAddress(0x1000),
        &user_data[..],
    );

    let mut diagnostic_buf = [0u8; 64];
    let diagnostic_written = {
        let mut writer: &mut [u8] = &mut diagnostic_buf[..];
        diagnostic_message
            .encode(&mut writer)
            .expect("encoding a diagnostic message should not fail")
    };

    let partial = &diagnostic_buf[..diagnostic_written - 1];
    assert!(matches!(try_frame(partial), Ok(None)));

    let (diagnostic_frame, diagnostic_consumed) =
        try_frame(&diagnostic_buf[..diagnostic_written])
            .expect("framing a complete message should not fail")
            .expect("a complete message should be available");
    assert_eq!(diagnostic_consumed, diagnostic_written);
    let diagnostic_payload =
        Payload::decode(diagnostic_frame.payload, diagnostic_frame.header.payload_type)
            .expect("decoding a diagnostic message payload should not fail");
    let decoded_diagnostic = Message {
        header: diagnostic_frame.header,
        payload: diagnostic_payload,
    };
    println!("Decoded diagnostic message: {decoded_diagnostic:?}");
}
