use crate::Game;
use crate::game::{Language, Version};
use crate::gen2::checksum;
use crate::layout::{CRYSTAL_INTERNATIONAL, Gen2Layout};

pub fn game(version: Version, language: Language) -> Game {
    Game {
        version,
        language,
        revision: 0,
    }
}

pub fn en(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            'A'..='Z' => 0x80 + (c as u8 - b'A'),
            'a'..='z' => 0xA0 + (c as u8 - b'a'),
            '0'..='9' => 0xF6 + (c as u8 - b'0'),
            ' ' => 0x7F,
            '.' => 0xE8,
            _ => panic!("no test encoding for {c}"),
        })
        .collect()
}

pub fn name(text: &[u8], len: usize) -> Vec<u8> {
    let mut bytes = text.to_vec();
    bytes.resize(len, 0x50);
    bytes
}

pub fn put(buffer: &mut [u8], at: usize, bytes: &[u8]) {
    buffer[at..at + bytes.len()].copy_from_slice(bytes);
}

pub struct TestMon {
    pub listed: u8,
    pub mon: Vec<u8>,
    pub ot: Vec<u8>,
    pub nickname: Vec<u8>,
}

pub fn put_list(buffer: &mut [u8], at: usize, capacity: usize, name_len: usize, mons: &[TestMon]) {
    let mon_size = mons.first().map_or(0, |m| m.mon.len());
    buffer[at] = mons.len() as u8;
    for (i, mon) in mons.iter().enumerate() {
        buffer[at + 1 + i] = mon.listed;
        let mons_at = at + 2 + capacity;
        put(buffer, mons_at + i * mon_size, &mon.mon);
        let ot_at = mons_at + capacity * mon_size;
        put(buffer, ot_at + i * name_len, &name(&mon.ot, name_len));
        let nick_at = ot_at + capacity * name_len;
        put(
            buffer,
            nick_at + i * name_len,
            &name(&mon.nickname, name_len),
        );
    }
    buffer[at + 1 + mons.len()] = 0xFF;
}

pub fn copy_routine(hl: u16, de: u16, bc: u16) -> Vec<u8> {
    let mut code = vec![0x21];
    code.extend(hl.to_le_bytes());
    code.push(0x11);
    code.extend(de.to_le_bytes());
    code.push(0x01);
    code.extend(bc.to_le_bytes());
    code.extend([0xCD, 0x00, 0x30]);
    code
}

pub fn fixed_names(prefix: &str, count: usize, len: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|i| name(&en(&label(prefix, i)), len))
        .collect()
}

pub fn name_list(prefix: &str, count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|i| {
            let mut bytes = en(&label(prefix, i));
            bytes.push(0x50);
            bytes
        })
        .collect()
}

pub fn label(prefix: &str, i: usize) -> String {
    format!(
        "{prefix}{}{}",
        (b'A' + (i / 26) as u8) as char,
        (b'A' + (i % 26) as u8) as char
    )
}

pub fn mon(species: u8, level: u8, party: bool) -> Vec<u8> {
    let mut m = vec![0u8; if party { 48 } else { 32 }];
    m[0] = species;
    m[1] = 0x12;
    m[2] = 0x21;
    m[6..8].copy_from_slice(&54321u16.to_be_bytes());
    m[8..11].copy_from_slice(&[0x00, 0x10, 0x00]);
    m[11..13].copy_from_slice(&1000u16.to_be_bytes());
    m[21] = 0xAA;
    m[22] = 0xAA;
    m[23] = 0x5E;
    m[27] = 70;
    m[28] = 0x21;
    m[31] = level;
    if party {
        m[34..36].copy_from_slice(&30u16.to_be_bytes());
        m[36..38].copy_from_slice(&31u16.to_be_bytes());
        m[44..46].copy_from_slice(&15u16.to_be_bytes());
        m[46..48].copy_from_slice(&16u16.to_be_bytes());
    }
    m
}

pub fn entry(species: u8, party: bool, nickname: &[u8], ot: &[u8]) -> TestMon {
    TestMon {
        listed: species,
        mon: mon(species, 12, party),
        ot: ot.to_vec(),
        nickname: nickname.to_vec(),
    }
}

pub fn egg(name_bytes: &[u8]) -> TestMon {
    TestMon {
        listed: 0xFD,
        mon: mon(0x98, 5, true),
        ot: en("GOLD"),
        nickname: name_bytes.to_vec(),
    }
}

pub(crate) fn save(layout: &Gen2Layout, name_bytes: &[u8], name_len: usize) -> Vec<u8> {
    let mut s = vec![0u8; layout.sram_size];
    s[layout.main_checks.0] = 0x63;
    s[layout.main_checks.1] = 0x7F;
    put(&mut s, layout.player_id, &[0x01, 0x02]);
    put(&mut s, layout.player_name, &name(name_bytes, name_len));
    put(&mut s, layout.play_time, &[0x01, 0x2C, 30, 15, 0]);
    put(
        &mut s,
        layout.money,
        &[0x01, 0x86, 0xA0, 0x00, 0x03, 0xE8, 0x00, 0x01, 0xF4],
    );
    s[layout.johto_badges] = 0x0F;
    s[layout.tms_hms] = 1;
    s[layout.tms_hms + 4] = 2;
    s[layout.tms_hms + 50] = 1;
    put(&mut s, layout.items, &[1, 0x12, 4, 0xFF]);
    put(&mut s, layout.key_items, &[2, 0x3B, 0x47, 0xFF]);
    put(&mut s, layout.balls, &[1, 0x05, 10, 0xFF]);
    put(&mut s, layout.pc_items, &[0, 0xFF]);
    s[layout.current_box] = 1;
    for i in 0..layout.boxes.len() {
        let box_name = name(&en(&format!("BOX{}", i + 1)), layout.box_name_len);
        put(
            &mut s,
            layout.box_names + i * layout.box_name_len,
            &box_name,
        );
    }
    s[layout.visited_spawns + 1] = 0b1100_0000;
    put_list(
        &mut s,
        layout.party,
        6,
        name_len,
        &[entry(0x9B, true, name_bytes, name_bytes), egg(name_bytes)],
    );
    s[layout.pokedex_caught + 18] = 0b1000_0000;
    s[layout.pokedex_seen + 18] = 0b1000_0000;
    s[layout.pokedex_seen + 19] = 0b0000_0100;
    let boxed = |n: usize| -> Vec<TestMon> {
        (0..n)
            .map(|_| entry(0x10, false, name_bytes, name_bytes))
            .collect()
    };
    for &slot in layout.boxes {
        put_list(&mut s, slot, layout.box_slots, name_len, &[]);
        s[slot + 1] = 0xFF;
    }
    put_list(
        &mut s,
        layout.boxes[0],
        layout.box_slots,
        name_len,
        &boxed(1),
    );
    put_list(
        &mut s,
        layout.boxes[1],
        layout.box_slots,
        name_len,
        &boxed(2),
    );
    put_list(
        &mut s,
        layout.box_data,
        layout.box_slots,
        name_len,
        &boxed(2),
    );
    if let Some(gender) = layout.gender {
        s[gender] = 1;
    }
    seal(layout, &mut s);
    s
}

pub(crate) fn seal_main(layout: &Gen2Layout, s: &mut [u8]) {
    let sum = checksum(&s[layout.main.0..layout.main.1]);
    put(s, layout.main_checksum, &sum.to_le_bytes());
}

pub(crate) fn seal(layout: &Gen2Layout, s: &mut [u8]) {
    seal_main(layout, s);
    if layout.backup.is_empty() {
        return;
    }
    let mut sum = 0u16;
    for &(main, backup, len) in layout.backup {
        s.copy_within(main..main + len, backup);
        sum = sum.wrapping_add(checksum(&s[backup..backup + len]));
    }
    s[layout.backup_checks.0] = 0x63;
    s[layout.backup_checks.1] = 0x7F;
    put(s, layout.backup_checksum, &sum.to_le_bytes());
}

pub fn crystal_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x200000];
    let mut pointers = Vec::new();
    for (bank, addr) in [
        (0x14u8, 0x7384u16),
        (0x72, 0x5F29),
        (0, 0),
        (0x72, 0x4000),
        (0, 0xDDFF),
        (0, 0xD3A8),
        (0x0B, 0x41EF),
        (0x04, 0x4B52),
    ] {
        pointers.push(bank);
        pointers.extend(addr.to_le_bytes());
    }
    put(&mut rom, 0x33AB, &pointers);
    put(&mut rom, 0x53384, &fixed_names("P", 251, 10));
    put(&mut rom, 0x1C9F29, &name_list("MOVE", 251));
    put(&mut rom, 0x1C8000, &name_list("THING", 249));
    let mut code = Vec::new();
    for (hl, de, bc) in [
        (0xD47B, 0xA009, 0x82A),
        (0xD47B, 0xB209, 0x82A),
        (0xDCA5, 0xA833, 0x32),
        (0xC608, 0xAA8E, 0x594),
        (0xDCD7, 0xA865, 0x31E),
        (0xD472, 0xBE3D, 0x7),
    ] {
        code.extend(copy_routine(hl, de, bc));
    }
    put(&mut rom, 0x14DDC, &code);
    rom
}

pub fn crystal_cartridge() -> Vec<u8> {
    let mut rom = crystal_rom();
    put(&mut rom, 0x134, b"PM_CRYSTAL\0BYTE");
    rom[0x143] = 0xC0;
    rom[0x147] = 0x10;
    rom[0x148] = 0x06;
    rom[0x149] = 0x03;
    rom[0x14A] = 0x01;
    rom
}

pub fn japanese_cartridge_in_latin() -> Vec<u8> {
    let mut rom = crystal_cartridge();
    rom[0x134 + 14] = b'J';
    put(&mut rom, 0x53384, &fixed_names("P", 251, 5));
    seal_cartridge(&mut rom);
    rom
}

pub fn seal_cartridge(rom: &mut [u8]) {
    rom[0x14E] = 0;
    rom[0x14F] = 0;
    let sum = rom
        .iter()
        .fold(0u16, |sum, &b| sum.wrapping_add(u16::from(b)));
    put(rom, 0x14E, &sum.to_be_bytes());
}

pub fn crystal_save() -> Vec<u8> {
    save(&CRYSTAL_INTERNATIONAL, &en("KRIS"), 11)
}

pub fn crystal_save_with_badges(johto: u8, kanto: u8, reached_kanto: bool) -> Vec<u8> {
    let layout = &CRYSTAL_INTERNATIONAL;
    let mut s = crystal_save();
    s[layout.johto_badges] = johto;
    s[layout.kanto_badges] = kanto;
    if reached_kanto {
        s[layout.visited_spawns] |= 0x80;
    }
    seal(layout, &mut s);
    s
}

pub fn crystal_live_wram(save: &[u8]) -> Vec<u8> {
    let mut wram = vec![0u8; 0x2000];
    for (addr, at, len) in [
        (0xD47B, 0x2009, 0x82A),
        (0xDCA5, 0x2833, 0x32),
        (0xDCD7, 0x2865, 0x31E),
        (0xD472, 0x3E3D, 7),
    ] {
        put(&mut wram, addr - 0xC000, &save[at..at + len]);
    }
    wram
}

pub fn set_crystal_live_money(wram: &mut [u8], money: u32) {
    put(wram, 0xD84E - 0xC000, &money.to_be_bytes()[1..]);
}
