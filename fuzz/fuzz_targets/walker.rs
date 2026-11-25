#![no_main]

use citrine_gb_walker::{Eeprom, Memory, REPLY_DELAY, Ram, Walker, airtime};
use libfuzzer_sys::fuzz_target;

const WIRE_XOR: u8 = 0xAA;
const HEADER: usize = 8;
const ASSERT_MASTER: u8 = 0xFA;
const MAX_PACKETS: usize = 64;

fn sum(bytes: &[u8], seed: u16) -> u16 {
    let mut total = u32::from(seed);
    for (i, &byte) in bytes.iter().enumerate() {
        let value = u32::from(byte);
        total += if i % 2 == 0 { value << 8 } else { value };
    }
    while total >> 16 != 0 {
        total = (total & 0xFFFF) + (total >> 16);
    }
    total as u16
}

fn checksum(bytes: &[u8]) -> u16 {
    let (header, payload) = bytes.split_at(bytes.len().min(HEADER));
    let seed = sum(header, 0x0002);
    if payload.is_empty() {
        seed
    } else {
        sum(payload, seed)
    }
}

fn wire(cmd: u8, extra: u8, session: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![cmd, extra, 0, 0];
    bytes.extend_from_slice(&session);
    bytes.extend_from_slice(payload);
    let check = checksum(&bytes);
    bytes[2..4].copy_from_slice(&check.to_le_bytes());
    bytes.iter().map(|b| b ^ WIRE_XOR).collect()
}

fn drain(walker: &mut Walker, now: u64) {
    for _ in 0..32 {
        if walker.take(now).is_none() {
            break;
        }
    }
    let _ = walker.events();
}

fuzz_target!(|data: &[u8]| {
    let _ = Memory::from_bytes(data);
    let _ = Ram::from_bytes(data);
    let Some((&head, mut input)) = data.split_first() else {
        return;
    };
    let mut eeprom = Eeprom::blank();
    for _ in 0..(head & 0x0F) {
        let [hi, lo, len, rest @ ..] = input else {
            break;
        };
        let len = usize::from(*len).min(rest.len());
        eeprom.write(usize::from(u16::from_be_bytes([*hi, *lo])), &rest[..len]);
        input = &rest[len..];
    }
    let mut walker = Walker::from_eeprom(eeprom, u64::from(head));
    let _ = walker.info();
    let _ = walker.health();
    let _ = walker.walking_pokemon();
    let step = REPLY_DELAY + airtime(512);
    let mut now = 0;
    walker.connect(now);
    walker.send(&wire(ASSERT_MASTER, 1, [0; 4], &[]), now);
    now += step;
    let own = walker
        .take(now)
        .and_then(|reply| {
            reply
                .get(4..HEADER)
                .map(|s| [s[0] ^ WIRE_XOR, s[1] ^ WIRE_XOR, s[2] ^ WIRE_XOR, s[3] ^ WIRE_XOR])
        })
        .unwrap_or([0; 4]);
    drain(&mut walker, now);
    for _ in 0..MAX_PACKETS {
        let [tag, cmd, extra, len, rest @ ..] = input else {
            break;
        };
        let len = usize::from(*len).min(rest.len());
        let payload = &rest[..len];
        input = &rest[len..];
        let bytes = if tag & 1 == 0 {
            wire(*cmd, *extra, own, payload)
        } else {
            payload.to_vec()
        };
        walker.send(&bytes, now);
        now += step;
        drain(&mut walker, now);
    }
    let _ = walker.info();
    let _ = walker.memory();
});
