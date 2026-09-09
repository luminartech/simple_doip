#![no_main]

use libfuzzer_sys::fuzz_target;
use simple_doip::messages::{Decode, Message};

// The whole-frame parser -- header plus payload -- against arbitrary bytes.
// Every input must produce `Ok` or `Err`, never a panic and never undefined
// behavior. `Message` borrows from the input buffer, so this also covers the
// zero-copy slicing that borrowing depends on.
fuzz_target!(|data: &[u8]| {
    let _ = Message::decode(data);
});
