#![no_main]

use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use simple_doip::message_codec::MessageCodec;
use tokio_util::codec::Decoder;

// The tokio `Decoder`, which is where a streaming TCP connection meets the
// parser: arbitrary bytes arrive in arbitrary chunk boundaries, so the codec
// has to handle partial frames, malformed headers and truncated payloads
// without panicking.
fuzz_target!(|data: &[u8]| {
    let mut codec = MessageCodec::new();
    let mut buf = BytesMut::from(data);

    // Decode repeatedly: one buffer can hold several frames, and the codec
    // returns `Ok(None)` when it wants more bytes.
    loop {
        match codec.decode(&mut buf) {
            // Decoded a frame; there may be more behind it.
            Ok(Some(_message)) => {}
            // Incomplete frame, or a parse error. Both are expected of
            // fuzzed input; neither is a crash.
            Ok(None) | Err(_) => break,
        }
    }
});
