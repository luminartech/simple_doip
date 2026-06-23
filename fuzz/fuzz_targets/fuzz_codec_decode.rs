#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use simple_doip::message_codec::MessageCodec;
use tokio_util::codec::Decoder;

// Fuzz the tokio `MessageCodec` decoder.
//
// This simulates a streaming TCP connection where arbitrary bytes arrive.
// The codec must handle partial frames, malformed headers, and truncated
// payloads without panicking.
fuzz_target!(|data: &[u8]| {
    let mut codec = MessageCodec::new();
    let mut buf = BytesMut::from(data);

    // Attempt to decode repeatedly — the codec may consume partial frames
    // and request more data (returning Ok(None)).
    loop {
        match codec.decode(&mut buf) {
            Ok(Some(_message)) => {
                // Successfully decoded a message; continue trying for more
                // frames in the remaining buffer.
            }
            Ok(None) => {
                // Not enough data for a complete frame; done.
                break;
            }
            Err(_) => {
                // Parse error — expected for fuzzed input, not a crash.
                break;
            }
        }
    }
});
