use std::cell::RefCell;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

const THREAD: &str = "state-fuzz";
const HANG: Duration = Duration::from_secs(60);

thread_local! {
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn word(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.word() % n.max(1) as u64) as usize
    }
}

#[derive(Clone, Copy, Debug)]
enum Item {
    Int {
        at: usize,
        len: usize,
        value: i128,
    },
    Bin {
        at: usize,
        header: usize,
        len: usize,
    },
}

fn be(bytes: &[u8]) -> usize {
    bytes.iter().fold(0, |n, &b| (n << 8) | usize::from(b))
}

fn items(data: &[u8]) -> Vec<Item> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let marker = data[at];
        let rest = &data[at + 1..];
        let field = |n: usize| (rest.len() >= n).then(|| be(&rest[..n]));
        let step = match marker {
            0x00..=0x7F => {
                out.push(Item::Int {
                    at,
                    len: 1,
                    value: i128::from(marker),
                });
                Some(1)
            }
            0x80..=0x9F | 0xC0 | 0xC2 | 0xC3 => Some(1),
            0xA0..=0xBF => Some(1 + usize::from(marker & 0x1F)),
            0xC4..=0xC6 => {
                let width = 1 << (marker - 0xC4);
                field(width).map(|len| {
                    if rest.len() >= width + len {
                        out.push(Item::Bin {
                            at,
                            header: 1 + width,
                            len,
                        });
                    }
                    1 + width + len
                })
            }
            0xC7..=0xC9 => {
                let width = 1 << (marker - 0xC7);
                field(width).map(|len| 2 + width + len)
            }
            0xCA => Some(5),
            0xCB => Some(9),
            0xCC..=0xD3 => {
                let width = 1 << ((marker - 0xCC) & 3);
                field(width).map(|raw| {
                    let value = if marker >= 0xD0 {
                        let shift = 64 - 8 * width;
                        i128::from(((raw as u64) << shift) as i64 >> shift)
                    } else {
                        raw as i128
                    };
                    out.push(Item::Int {
                        at,
                        len: 1 + width,
                        value,
                    });
                    1 + width
                })
            }
            0xD4..=0xD8 => Some(2 + (1 << (marker - 0xD4))),
            0xD9..=0xDB => {
                let width = 1 << (marker - 0xD9);
                field(width).map(|len| 1 + width + len)
            }
            0xDC | 0xDE => Some(3),
            0xDD | 0xDF => Some(5),
            0xE0..=0xFF => {
                out.push(Item::Int {
                    at,
                    len: 1,
                    value: i128::from(marker as i8),
                });
                Some(1)
            }
            0xC1 => None,
        };
        match step {
            Some(step) if at + step <= data.len() => at += step,
            _ => break,
        }
    }
    out
}

fn int(value: i128) -> Vec<u8> {
    let value = value.clamp(i128::from(i64::MIN), i128::from(u64::MAX));
    match value {
        0..=0x7F => vec![value as u8],
        0x80..=0xFF => vec![0xCC, value as u8],
        0x100..=0xFFFF => [&[0xCD][..], &(value as u16).to_be_bytes()].concat(),
        0x1_0000..=0xFFFF_FFFF => [&[0xCE][..], &(value as u32).to_be_bytes()].concat(),
        0x1_0000_0000.. => [&[0xCF][..], &(value as u64).to_be_bytes()].concat(),
        -32..=-1 => vec![value as i8 as u8],
        -0x80..=-33 => vec![0xD0, value as i8 as u8],
        -0x8000..=-0x81 => [&[0xD1][..], &(value as i16).to_be_bytes()].concat(),
        -0x8000_0000..=-0x8001 => [&[0xD2][..], &(value as i32).to_be_bytes()].concat(),
        _ => [&[0xD3][..], &(value as i64).to_be_bytes()].concat(),
    }
}

fn bin(len: usize) -> Vec<u8> {
    match len {
        0..=0xFF => vec![0xC4, len as u8],
        0x100..=0xFFFF => [&[0xC5][..], &(len as u16).to_be_bytes()].concat(),
        _ => [&[0xC6][..], &(len as u32).to_be_bytes()].concat(),
    }
}

fn new_value(old: i128, rng: &mut Rng) -> i128 {
    let bit = 1i128 << rng.below(32);
    match rng.below(14) {
        0 => 0,
        1 => 1,
        2 => -1,
        3 => old + 1,
        4 => old - 1,
        5 => old ^ bit,
        6 => 0xFF,
        7 => 0xFFFF,
        8 => i128::from(u32::MAX),
        9 => i128::from(i32::MIN),
        10 => i128::from(u64::MAX),
        11 => i128::from(i64::MIN),
        12 => old * 2 + 1,
        _ => i128::from(rng.word()),
    }
}

pub fn mutate(data: &mut Vec<u8>, rng: &mut Rng) -> String {
    let found = items(data);
    let ints: Vec<Item> = found
        .iter()
        .copied()
        .filter(|item| matches!(item, Item::Int { .. }))
        .collect();
    let bins: Vec<Item> = found
        .iter()
        .copied()
        .filter(|item| matches!(item, Item::Bin { .. }))
        .collect();
    let pick = rng.below(10);
    if pick < 6
        && let Some(&Item::Int { at, len, value }) = ints.get(rng.below(ints.len()))
    {
        let new = new_value(value, rng);
        data.splice(at..at + len, int(new));
        return format!("int at {at}: {value} to {new}");
    }
    if pick < 8
        && let Some(&Item::Bin { at, header, len }) = bins.get(rng.below(bins.len()))
    {
        let new = match rng.below(5) {
            0 => 0,
            1 => len / 2,
            2 => len.saturating_sub(1),
            3 => len + 1,
            _ => len + 1 + rng.below(64),
        };
        let mut body = data[at + header..at + header + len.min(new)].to_vec();
        body.resize(new, rng.word() as u8);
        data.splice(at..at + header + len, [bin(new), body].concat());
        return format!("bin at {at}: {len} bytes to {new}");
    }
    if pick < 9 && !data.is_empty() {
        let at = rng.below(data.len());
        let bit = rng.below(8);
        data[at] ^= 1 << bit;
        return format!("bit {bit} of byte {at} flipped");
    }
    let len = rng.below(data.len());
    data.truncate(len);
    format!("cut to {len} bytes")
}

fn install_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if std::thread::current().name() == Some(THREAD) {
                let at = info
                    .location()
                    .map(|at| format!("{}:{}", at.file(), at.line()))
                    .unwrap_or_default();
                let at = format!("{at} {}", info.payload_as_str().unwrap_or_default());
                PANIC.with(|slot| *slot.borrow_mut() = Some(at));
            } else {
                previous(info);
            }
        }));
    });
}

pub fn runs(default: usize) -> usize {
    std::env::var("CITRINE_STATE_FUZZ_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(default)
}

fn first_run() -> usize {
    std::env::var("CITRINE_STATE_FUZZ_FROM")
        .ok()
        .and_then(|first| first.parse().ok())
        .unwrap_or(0)
}

fn seed() -> u64 {
    std::env::var("CITRINE_STATE_FUZZ_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or(0x5EED)
}

enum Note {
    Start(usize, String),
    Done(BTreeMap<String, (usize, String)>),
}

pub fn fuzz<M: 'static>(
    name: &str,
    state: Vec<u8>,
    runs: usize,
    load: impl Fn(&[u8]) -> Option<M> + Send + 'static,
    run: impl Fn(&mut M) + Send + 'static,
) {
    if let Some(report) = panic_sites(name, state, runs, load, run) {
        panic!("{report}");
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the fuzz worker needs a 64 MiB stack"
)]
pub fn panic_sites<M: 'static>(
    name: &str,
    state: Vec<u8>,
    runs: usize,
    load: impl Fn(&[u8]) -> Option<M> + Send + 'static,
    run: impl Fn(&mut M) + Send + 'static,
) -> Option<String> {
    install_hook();
    let seed = seed();
    let first = first_run();
    let (notes, heard) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name(THREAD.into())
        .stack_size(64 << 20)
        .spawn(move || {
            let mut found = BTreeMap::new();
            for index in first..runs {
                let mut rng = Rng::new(seed ^ (index as u64).wrapping_mul(0x2545_F491));
                let mut data = state.clone();
                let mut changes = Vec::new();
                let mutated = catch_unwind(AssertUnwindSafe(|| {
                    for _ in 0..1 + rng.below(3) {
                        changes.push(mutate(&mut data, &mut rng));
                    }
                }));
                let change = changes.join(", ");
                let _ = notes.send(Note::Start(index, change.clone()));
                let outcome = mutated.and_then(|()| {
                    catch_unwind(AssertUnwindSafe(|| {
                        if let Some(mut machine) = load(&data) {
                            run(&mut machine);
                        }
                    }))
                });
                if outcome.is_err() {
                    let at = PANIC
                        .with(|slot| slot.borrow_mut().take())
                        .unwrap_or_default();
                    found
                        .entry(at)
                        .or_insert((0, format!("run {index}: {change}")))
                        .0 += 1;
                }
            }
            let _ = notes.send(Note::Done(found));
        })
        .expect("the fuzz thread starts");
    let mut last = (0, String::new());
    loop {
        match heard.recv_timeout(HANG) {
            Ok(Note::Start(index, change)) => last = (index, change),
            Ok(Note::Done(found)) => {
                let _ = worker.join();
                eprintln!(
                    "{name}, seed {seed}: runs {first}..{runs} done, {} panic sites",
                    found.len()
                );
                let report: Vec<String> = found
                    .iter()
                    .map(|(at, (count, first))| format!("{at} x{count}, first {first}"))
                    .collect();
                return (!report.is_empty()).then(|| {
                    format!(
                        "{name}, seed {seed}: {} panic sites\n{}",
                        report.len(),
                        report.join("\n")
                    )
                });
            }
            Err(RecvTimeoutError::Timeout) => panic!(
                "{name}, seed {seed}: run {} still running after {} s: {}",
                last.0,
                HANG.as_secs(),
                last.1
            ),
            Err(RecvTimeoutError::Disconnected) => {
                panic!(
                    "{name}, seed {seed}: the fuzz thread died in run {}: {}",
                    last.0, last.1
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scanner_finds_every_int_and_byte_string() {
        let data =
            rmp_serde::to_vec(&(5u8, 300u16, -2i8, -200i32, serde_bytes(&[1, 2, 3]))).unwrap();
        let found = items(&data);
        let ints: Vec<i128> = found
            .iter()
            .filter_map(|item| match item {
                Item::Int { value, .. } => Some(*value),
                Item::Bin { .. } => None,
            })
            .collect();
        assert_eq!(ints, [5, 300, -2, -200]);
        assert!(matches!(found.last(), Some(Item::Bin { len: 3, .. })));
    }

    #[test]
    fn a_byte_string_cut_short_is_not_offered_for_mutation() {
        let mut data = rmp_serde::to_vec(&(5u8, serde_bytes(&[1, 2, 3, 4]))).unwrap();
        data.truncate(data.len() - 2);
        assert!(
            items(&data)
                .iter()
                .all(|item| matches!(item, Item::Int { .. }))
        );
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            let mut copy = data.clone();
            mutate(&mut copy, &mut rng);
        }
    }

    #[test]
    fn an_int_written_back_reads_the_same() {
        for value in [
            0,
            127,
            128,
            255,
            256,
            65_535,
            65_536,
            -1,
            -32,
            -33,
            -129,
            -40_000,
            i128::from(u64::MAX),
            i128::from(i64::MIN),
        ] {
            let bytes = int(value);
            let found = items(&bytes);
            assert!(
                matches!(found[..], [Item::Int { value: read, len, .. }] if read == value && len == bytes.len())
            );
        }
    }

    fn serde_bytes(bytes: &[u8]) -> Bytes<'_> {
        Bytes(bytes)
    }

    struct Bytes<'a>(&'a [u8]);

    impl serde::Serialize for Bytes<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_bytes(self.0)
        }
    }
}

#[cfg(test)]
mod consoles {
    #[cfg(feature = "gba")]
    use super::panic_sites;
    use super::{fuzz, runs};
    use crate::console::{Console, Rollback};
    use crate::gb::{GameBoy, GbModel};
    use crate::persistence::full_dump::FullDump;
    use crate::rom::Rom;

    const CARTS: [(u8, u8, u8); 13] = [
        (0x00, 0, 0),
        (0x03, 2, 3),
        (0x06, 1, 0),
        (0x0F, 2, 0),
        (0x13, 3, 3),
        (0x1B, 4, 4),
        (0x20, 3, 0),
        (0x22, 2, 0),
        (0xFC, 2, 4),
        (0xFD, 2, 0),
        (0xFE, 2, 2),
        (0xFF, 2, 2),
        (0x10, 2, 3),
    ];

    fn gb_rom(cart: u8, rom_size: u8, ram_size: u8) -> Vec<u8> {
        let mut data = vec![0; 0x8000 << rom_size];
        data[0x100..0x102].copy_from_slice(&[0x18, 0xFE]);
        data[0x147] = cart;
        data[0x148] = rom_size;
        data[0x149] = ram_size;
        data
    }

    fn running_gb(data: &[u8], model: GbModel) -> GameBoy {
        let mut gb = GameBoy::new_empty(model);
        gb.load_rom(&Rom::new(data)).expect("the test cart loads");
        for _ in 0..4 {
            gb.run_frame();
        }
        gb
    }

    fn three_frames<M: Console>(machine: &mut M) {
        for _ in 0..3 {
            machine.run_frame();
        }
    }

    #[test]
    fn a_damaged_game_boy_rollback_state_never_panics() {
        for (cart, rom_size, ram_size) in CARTS {
            for model in [GbModel::Dmg, GbModel::Cgb] {
                let base = running_gb(&gb_rom(cart, rom_size, ram_size), model);
                let state = base.encode_state().expect("the state encodes");
                fuzz(
                    &format!("gb rollback cart {cart:02X} {model:?}"),
                    state,
                    runs(16),
                    move |data| base.decode_state(data).ok(),
                    three_frames,
                );
            }
        }
    }

    #[test]
    fn a_damaged_game_boy_snapshot_never_panics() {
        for (cart, rom_size, ram_size) in CARTS {
            let data = gb_rom(cart, rom_size, ram_size);
            let mut base = running_gb(&data, GbModel::Cgb);
            let sram = base.poll_sram_dump(true, 0);
            let state = base.full_dump(sram).to_rmp().expect("the dump encodes");
            fuzz(
                &format!("gb snapshot cart {cart:02X}"),
                state,
                runs(16),
                move |dump| {
                    let dump = FullDump::from_rmp(dump).ok()?;
                    GameBoy::from_full_dump(dump, &Rom::new(&data), 0).ok()
                },
                three_frames,
            );
        }
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_damaged_gba_state_never_panics() {
        use crate::gba::machine::Gba;
        let mut reports = Vec::new();
        for tag in [
            &b""[..],
            b"SRAM_V113",
            b"FLASH_V126",
            b"FLASH1M_V103",
            b"EEPROM_V124",
        ] {
            let mut rom: Vec<u8> = std::iter::repeat_n(0xEAFF_FFFEu32.to_le_bytes(), 0x4000)
                .flatten()
                .collect();
            rom[0x1000..0x1000 + tag.len()].copy_from_slice(tag);
            let mut base = Gba::new(&rom);
            three_frames(&mut base);
            let state = base.encode_state().expect("the state encodes");
            reports.extend(panic_sites(
                &format!("gba {}", String::from_utf8_lossy(tag)),
                state,
                runs(24),
                move |data| base.decode_state(data).ok(),
                |gba: &mut Gba| {
                    #[cfg(feature = "jit")]
                    gba.set_jit(true);
                    three_frames(gba);
                },
            ));
        }
        assert!(reports.is_empty(), "{}", reports.join("\n\n"));
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_damaged_ds_state_never_panics() {
        use crate::nds::machine::test_rom::{BRANCH_TO_SELF, rom};
        use crate::nds::machine::{Nds, NdsFirmware};
        const COUNT: [u32; 6] = [0xE3A0_0000, 0xE280_0001, 0xE58F_0004, 0xEAFF_FFFC, 0, 0];
        let mut base = Nds::new(rom(&COUNT, &[BRANCH_TO_SELF]), NdsFirmware::default())
            .expect("the test game loads");
        three_frames(&mut base);
        let state = base.encode_state().expect("the state encodes");
        fuzz(
            "ds",
            state,
            runs(48),
            move |data| base.decode_state(data).ok(),
            |nds: &mut Nds| {
                #[cfg(feature = "jit")]
                nds.set_jit(true);
                three_frames(nds);
            },
        );
    }
}
