#![cfg(feature = "persistence")]

use citrine_gb::cheats::{Cheat, CheatTarget};
use citrine_gb::console::{Console, ConsoleKind, Keys};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;
use citrine_gb::{ReadMemory, WriteMemory};
use citrine_gb_test_roms as test_roms;
use std::path::PathBuf;

fn roms() -> PathBuf {
    std::env::var_os("CITRINE_CHEAT_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(test_roms::roms_dir)
}

fn boot(file: &str, kind: ConsoleKind) -> Option<Machine> {
    let bytes = test_roms::read("cheat_roms", roms().join(file))?;
    Some(Machine::load(&Rom::new(&bytes), kind, &Firmware::default()).expect("load"))
}

fn play(machine: &mut Machine, cheats: &[Cheat], frames: usize, keys: Keys) {
    for _ in 0..frames {
        machine.set_keys(keys);
        machine.apply_cheats(cheats);
        machine.run_frame();
    }
}

fn read(machine: &mut Machine, address: u32, width: u8) -> u32 {
    match machine {
        Machine::Gb(gb) => gb.cheat_read(address, width),
        #[cfg(feature = "gba")]
        Machine::Gba(gba) => gba.cheat_read(address, width),
        #[cfg(feature = "nds")]
        Machine::Nds(nds) => nds.cheat_read(address, width),
    }
}

fn bytes(machine: &mut Machine, address: u32, count: u32) -> Vec<u8> {
    (0..count)
        .map(|index| read(machine, address + index, 1) as u8)
        .collect()
}

#[test]
#[ignore]
fn red_game_shark_max_money_reaches_the_money_bytes() {
    let Some(mut red) = boot("red.gb", ConsoleKind::Gb) else {
        return;
    };
    let cheat = Cheat::parse(ConsoleKind::Gb, "019947D3\n019948D3\n019949D3").unwrap();
    play(&mut red, &[], 300, Keys::empty());
    assert_ne!(bytes(&mut red, 0xD347, 3), [0x99, 0x99, 0x99]);
    play(&mut red, &[cheat], 60, Keys::empty());
    assert_eq!(bytes(&mut red, 0xD347, 3), [0x99, 0x99, 0x99]);
}

#[test]
#[ignore]
fn red_game_genie_patches_the_banks_its_compare_byte_matches() {
    let Some(mut red) = boot("red.gb", ConsoleKind::Gb) else {
        return;
    };
    let cheat = Cheat::parse(ConsoleKind::Gb, "1A8-258-A22").unwrap();
    let gb = red.as_gb_mut().expect("game boy");
    let pristine = gb.cheat_rom().to_vec();
    let matching: Vec<usize> = (1..pristine.len() / 0x4000)
        .filter(|bank| pristine[bank * 0x4000 + 0x3825] == 0x12)
        .collect();
    assert!(!matching.is_empty());
    play(&mut red, &[cheat], 60, Keys::empty());
    let gb = red.as_gb_mut().expect("game boy");
    for bank in 1..pristine.len() / 0x4000 {
        gb.cartridge.write_naive(0x2000, bank as u8);
        let expected = if matching.contains(&bank) {
            0x1A
        } else {
            pristine[bank * 0x4000 + 0x3825]
        };
        assert_eq!(gb.cartridge.read_naive(0x7825), expected, "bank {bank}");
    }
    assert_eq!(&*gb.rom_bytes(), &pristine[..]);
}

#[test]
#[ignore]
fn crystal_game_shark_max_money_reaches_the_money_bytes() {
    let Some(mut crystal) = boot("crystal.gbc", ConsoleKind::Gbc) else {
        return;
    };
    let cheat = Cheat::parse(ConsoleKind::Gbc, "010F4ED8\n01424FD8\n013F50D8").unwrap();
    play(&mut crystal, &[cheat], 300, Keys::empty());
    assert_eq!(bytes(&mut crystal, 0xD84E, 3), [0x0F, 0x42, 0x3F]);
}

#[cfg(feature = "gba")]
#[test]
#[ignore]
fn ruby_code_breaker_max_money_reaches_the_money_word() {
    let Some(mut ruby) = boot("ruby.gba", ConsoleKind::Gba) else {
        return;
    };
    let cheat = Cheat::parse(
        ConsoleKind::Gba,
        "0000B138 000A\n10000428 0007\n82025BC4 E0FF\n82025BC6 05F5",
    )
    .unwrap();
    assert_eq!(cheat.format().label(), "CodeBreaker");
    play(&mut ruby, &[cheat], 300, Keys::empty());
    assert_eq!(read(&mut ruby, 0x0202_5BC4, 4), 0x05F5_E0FF);
}

#[cfg(feature = "nds")]
#[test]
#[ignore]
fn heartgold_action_replay_max_money_follows_the_save_pointer() {
    let Some(mut hg) = boot("hg.nds", ConsoleKind::Nds) else {
        return;
    };
    if let Some(save) = test_roms::read("cheat_roms", roms().join("hg.sav")) {
        let battery = hg.battery_from_file(&save);
        hg.put_battery(battery, 1_700_000_000);
    }
    let cheat = Cheat::parse(
        ConsoleKind::Nds,
        "94000130 FFFB0000\n62111860 00000000\nB2111860 00000000\n00000088 000F423F\nD2000000 00000000",
    )
    .unwrap();
    play(&mut hg, &[], 200, Keys::empty());
    for _ in 0..27 {
        play(&mut hg, &[], 8, Keys::A);
        play(&mut hg, &[], 22, Keys::empty());
    }
    play(&mut hg, &[], 300, Keys::empty());
    let pointer = read(&mut hg, 0x0211_1860, 4);
    assert_ne!(pointer, 0);
    assert_ne!(read(&mut hg, pointer + 0x88, 4), 0x000F_423F);
    play(&mut hg, std::slice::from_ref(&cheat), 30, Keys::empty());
    assert_ne!(read(&mut hg, pointer + 0x88, 4), 0x000F_423F);
    play(&mut hg, &[cheat], 30, Keys::SELECT);
    assert_eq!(read(&mut hg, pointer + 0x88, 4), 0x000F_423F);
}
