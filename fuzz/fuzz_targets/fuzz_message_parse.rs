#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::Message;

// Fuzz the complete DoIP message parser (header + payload).
//
// This exercises `Message::read()` with arbitrary byte sequences,
// checking that the parser never panics on malformed input.
fuzz_target!(|data: &[u8]| {
    // The parser should gracefully handle any input, returning Ok or Err,
    // but never panicking or invoking undefined behavior.
    let _ = Message::read(&mut &data[..]);
});
