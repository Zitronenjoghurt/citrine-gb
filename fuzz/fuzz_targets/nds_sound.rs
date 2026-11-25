#![no_main]

use citrine_gb_nds_files::{Sbnk, Sdat, SoundKind, Swar, Wave, sequence_data};
use libfuzzer_sys::fuzz_target;

const MAX_ITEMS: usize = 64;
const KEYS: [u8; 4] = [0, 36, 60, 127];

fn wave(wave: &Wave) {
    let _ = wave.format.label();
    let _ = wave.words();
    let _ = wave.sample_count();
    let _ = wave.loop_sample();
}

fn sound_file(bytes: &[u8]) {
    let _ = sequence_data(bytes);
    if let Ok(bank) = Sbnk::parse(bytes) {
        for program in 0..bank.len().min(MAX_ITEMS) {
            if let Some(instrument) = bank.instrument(program) {
                for key in KEYS {
                    let _ = instrument.note(key);
                }
            }
        }
    }
    if let Ok(archive) = Swar::parse(bytes) {
        for index in 0..archive.len().min(MAX_ITEMS) {
            if let Ok(found) = archive.wave(index) {
                wave(&found);
            }
        }
    }
    if let Ok(found) = Wave::parse_swav(bytes) {
        wave(&found);
    }
    if let Ok(found) = Wave::parse_info(bytes) {
        wave(&found);
    }
}

fuzz_target!(|data: &[u8]| {
    sound_file(data);
    let Ok(sdat) = Sdat::parse(data) else {
        return;
    };
    for kind in SoundKind::ALL {
        let _ = kind.label();
        for entry in sdat.entries(kind).iter().take(MAX_ITEMS) {
            let _ = entry.label();
            let _ = sdat.entry(kind, entry.index);
            let _ = sdat.entry_for_file(kind, entry.file);
            if let Ok(file) = sdat.file(entry.file) {
                sound_file(file);
            }
        }
    }
});
