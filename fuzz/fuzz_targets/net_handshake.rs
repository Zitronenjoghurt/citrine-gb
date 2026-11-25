#![no_main]

use citrine_gb_net::{PeerAnswer, RelaySecret, accept, keyed_answer};
use libfuzzer_sys::fuzz_target;

const PSK: [u8; 32] = [9; 32];

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let secret = RelaySecret::from_bytes([7; 32]);
    match mode % 4 {
        0 => {
            let _ = accept(&secret, rest);
        }
        1 | 2 => {
            let psk = (mode % 4 == 2).then_some(&PSK);
            if let Ok((answer, _)) = PeerAnswer::accept(rest, &secret, psk) {
                let _ = answer.hello();
                let _ = answer.finish(rest);
            }
        }
        _ => {
            let _ = keyed_answer(b"citrine-gb fuzz", &PSK, rest);
        }
    }
});
