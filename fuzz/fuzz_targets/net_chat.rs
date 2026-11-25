#![no_main]

use citrine_gb_net::{Envelope, unpack};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = unpack(data, 1 << 20);
    let _ = Envelope::unpacked(data);
});
