use crate::fixtures::{
    TestMon, copy_routine, crystal_rom, en, fixed_names, game, label, name, name_list, put,
    put_list,
};
use crate::gen1::checksum;
use crate::{Language, Location, RomTables, Version};

pub(crate) fn mon(species: u8, level: u8, party: bool) -> Vec<u8> {
    let mut m = vec![0u8; if party { 44 } else { 33 }];
    m[0] = species;
    m[1..3].copy_from_slice(&35u16.to_be_bytes());
    m[3] = level;
    m[8] = 0x21;
    m[9] = 0x2D;
    m[12..14].copy_from_slice(&12345u16.to_be_bytes());
    m[14..17].copy_from_slice(&[0x00, 0x01, 0x2C]);
    m[17..19].copy_from_slice(&500u16.to_be_bytes());
    m[27] = 0xA5;
    m[28] = 0x3C;
    m[29] = 0x23;
    m[30] = 0xC5;
    if party {
        m[33] = level;
        m[34..36].copy_from_slice(&40u16.to_be_bytes());
        m[36..38].copy_from_slice(&21u16.to_be_bytes());
        m[38..40].copy_from_slice(&22u16.to_be_bytes());
        m[40..42].copy_from_slice(&23u16.to_be_bytes());
        m[42..44].copy_from_slice(&24u16.to_be_bytes());
    }
    m
}

pub(crate) fn entry(species: u8, level: u8, party: bool, nickname: &str) -> TestMon {
    TestMon {
        listed: species,
        mon: mon(species, level, party),
        ot: en("RED"),
        nickname: en(nickname),
    }
}

pub(crate) fn empty_banks(save: &mut [u8], boxes_per_bank: usize, box_size: usize) {
    for bank in [0x4000, 0x6000] {
        for i in 0..boxes_per_bank {
            save[bank + i * box_size] = 0;
            save[bank + i * box_size + 1] = 0xFF;
        }
    }
}

pub(crate) fn international_save() -> Vec<u8> {
    let mut s = vec![0u8; 0x8000];
    put(&mut s, 0x2598, &name(&en("RED"), 11));
    s[0x25A3] = 0b0000_0001;
    s[0x25A3 + 3] = 0b0000_0001;
    s[0x25B6] = 0b0000_1001;
    s[0x25B6 + 3] = 0b0000_0001;
    s[0x25B6 + 18] = 0b0100_0000;
    put(&mut s, 0x25C9, &[2, 0x04, 5, 0x14, 3, 0xFF]);
    put(&mut s, 0x25F3, &[0x01, 0x23, 0x45]);
    s[0x2602] = 0b0000_0101;
    put(&mut s, 0x2605, &[0x30, 0x39]);
    put(&mut s, 0x27E6, &[1, 0x0A, 1, 0xFF]);
    s[0x284C] = 0x80 | 2;
    put(&mut s, 0x2850, &[0x09, 0x99]);
    put(&mut s, 0x2CED, &[12, 0, 34, 56, 7]);
    put_list(
        &mut s,
        0x2F2C,
        6,
        11,
        &[entry(0x99, 5, true, "BULB"), entry(0x54, 7, true, "PIKA")],
    );
    put_list(&mut s, 0x30C0, 20, 11, &[entry(0x24, 3, false, "BIRD")]);
    empty_banks(&mut s, 6, 0x462);
    put_list(&mut s, 0x4000, 20, 11, &[entry(0x05, 9, false, "ONE")]);
    put_list(
        &mut s,
        0x6000 + 0x462,
        20,
        11,
        &[
            entry(0x06, 10, false, "TWO"),
            entry(0x07, 11, false, "THREE"),
        ],
    );
    seal_international(&mut s);
    s
}

pub(crate) fn seal_international(save: &mut [u8]) {
    save[0x3523] = checksum(&save[0x2598..0x3523]);
}

pub(crate) fn red_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x80000];
    let mut pointers = Vec::new();
    for word in [0x4000u16, 0x4000, 0x7000, 0x5000, 0xD273, 0xD9AC, 0x7100] {
        pointers.extend(word.to_le_bytes());
    }
    put(&mut rom, 0x1000, &pointers);
    put(&mut rom, 7 * 0x4000, &fixed_names("M", 190, 10));
    put(&mut rom, 10 * 0x4000, &name_list("MOVE", 165));
    put(&mut rom, 0x5000, &name_list("ITEM", 83));
    let order: Vec<u8> = (1..=190u16)
        .map(|i| match i {
            1 => 151,
            2..=40 => 0,
            _ => (191 - i) as u8,
        })
        .collect();
    put(&mut rom, 0x41024, &order);
    let mut code = Vec::new();
    for (hl, de, bc) in [
        (0xA5A3, 0xD2F7, 0x789),
        (0xD158, 0xA598, 0x0B),
        (0xD2F7, 0xA5A3, 0x26),
        (0xD2F7, 0xA5A3, 0x789),
        (0xD163, 0xAF2C, 0x194),
        (0xDA80, 0xB0C0, 0x462),
        (0xC100, 0xAD2C, 0x200),
    ] {
        code.extend(copy_routine(hl, de, bc));
    }
    put(&mut rom, 0x73652, &code);
    rom
}

pub(crate) struct Planes {
    pub width: u8,
    pub height: u8,
    pub low: Vec<u8>,
    pub high: Vec<u8>,
}

impl Planes {
    pub fn random(width: u8, height: u8, seed: u32, density: u32) -> Self {
        let len = usize::from(width) * usize::from(height) * 8;
        let mut state = seed.max(1);
        let mut next = move || {
            let mut byte = 0u8;
            for bit in 0..8 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                if state % 100 < density {
                    byte |= 1 << bit;
                }
            }
            byte
        };
        let low = (0..len).map(|_| next()).collect();
        let high = (0..len).map(|_| next()).collect();
        Self {
            width,
            height,
            low,
            high,
        }
    }

    pub fn shade(&self, x: usize, y: usize) -> u8 {
        let rows = usize::from(self.height) * 8;
        let at = (x / 8) * rows + y;
        let bit = 7 - x % 8;
        ((self.low[at] >> bit) & 1) | (((self.high[at] >> bit) & 1) << 1)
    }
}

pub(crate) struct BitWriter {
    bytes: Vec<u8>,
    used: u8,
}

impl BitWriter {
    pub(crate) fn bit(&mut self, bit: u8) {
        if self.used == 0 {
            self.bytes.push(0);
        }
        let last = self.bytes.last_mut().unwrap();
        *last |= (bit & 1) << (7 - self.used);
        self.used = (self.used + 1) % 8;
    }

    pub(crate) fn bits(&mut self, value: usize, count: u32) {
        for i in (0..count).rev() {
            self.bit(((value >> i) & 1) as u8);
        }
    }
}

pub(crate) fn delta_encode(plane: &[u8], rows: usize) -> Vec<u8> {
    let mut out = plane.to_vec();
    for y in 0..rows {
        let mut previous = 0u8;
        for column in 0..plane.len() / rows {
            let byte = plane[column * rows + y];
            let mut encoded = 0u8;
            for bit in (0..8).rev() {
                let value = (byte >> bit) & 1;
                encoded |= (value ^ previous) << bit;
                previous = value;
            }
            out[column * rows + y] = encoded;
        }
    }
    out
}

pub(crate) fn pack(writer: &mut BitWriter, plane: &[u8], rows: usize) {
    let mut pairs = Vec::new();
    for column in 0..plane.len() / rows {
        for pair in 0..4 {
            for y in 0..rows {
                pairs.push((plane[column * rows + y] >> (6 - 2 * pair)) & 3);
            }
        }
    }
    writer.bit(u8::from(pairs[0] != 0));
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i] == 0 {
            let run = pairs[i..].iter().take_while(|&&p| p == 0).count();
            let mut ones = 0;
            while run >= (4 << ones) - 1 {
                ones += 1;
            }
            writer.bits((1 << ones) - 1, ones as u32);
            writer.bit(0);
            writer.bits(run - ((2 << ones) - 1), ones as u32 + 1);
            i += run;
        } else {
            while i < pairs.len() && pairs[i] != 0 {
                writer.bits(usize::from(pairs[i]), 2);
                i += 1;
            }
            if i < pairs.len() {
                writer.bits(0, 2);
            }
        }
    }
}

pub(crate) fn encode_gen1(planes: &Planes, mode: u8, first: usize) -> Vec<u8> {
    let rows = usize::from(planes.height) * 8;
    let both = [&planes.low, &planes.high];
    let (a, b) = (both[first], both[first ^ 1]);
    let a_xor_b: Vec<u8> = a.iter().zip(b.iter()).map(|(x, y)| x ^ y).collect();
    let encoded_a = delta_encode(a, rows);
    let encoded_b = match mode {
        0 => delta_encode(b, rows),
        1 => a_xor_b,
        _ => delta_encode(&a_xor_b, rows),
    };
    let mut writer = BitWriter {
        bytes: vec![(planes.width << 4) | planes.height],
        used: 0,
    };
    writer.bit(first as u8);
    pack(&mut writer, &encoded_a, rows);
    match mode {
        0 => writer.bit(0),
        1 => writer.bits(0b10, 2),
        _ => writer.bits(0b11, 2),
    }
    pack(&mut writer, &encoded_b, rows);
    writer.bytes
}

pub(crate) fn offset(bank: usize, addr: u16) -> usize {
    bank * 0x4000 + usize::from(addr) - 0x4000
}

pub(crate) fn words(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub(crate) fn type_table(rom: &mut [u8], code_at: usize, table_addr: u16, count: usize) {
    let bank = code_at / 0x4000;
    let mut code = vec![0x87, 0x21];
    code.extend(table_addr.to_le_bytes());
    code.extend([0x5F, 0x16, 0x00, 0x19, 0x2A, 0x5F, 0x56, 0xE1]);
    put(rom, code_at, &code);
    let names_addr = table_addr + 0x100;
    let mut names = Vec::new();
    let mut pointers = Vec::new();
    for i in 0..count {
        pointers.push(names_addr + names.len() as u16);
        names.extend(en(&label("TYPE", i)));
        names.push(0x50);
    }
    put(rom, offset(bank, table_addr), &words(&pointers));
    put(rom, offset(bank, names_addr), &names);
}

pub(crate) fn crystal_rom_with_pictures() -> Vec<u8> {
    let mut rom = crystal_rom();
    let mut fix = vec![
        0xE5, 0xC5, 0xD6, 0x12, 0x4F, 0x06, 0x00, 0x21, 0x0F, 0x40, 0x09, 0x7E, 0xC1, 0xE1, 0xC9,
    ];
    fix.extend((0..24).map(|i| 0x48 + i as u8));
    put(&mut rom, offset(4, 0x4000), &fix);
    put(
        &mut rom,
        offset(4, 0x4100),
        &[0x78, 0x16, 0x48, 0xFE, 0xC9, 0x20, 0x03, 0x79, 0x16, 0x50],
    );
    let base: Vec<u8> = (1..=251usize)
        .flat_map(|dex| {
            let mut record = vec![0u8; 32];
            record[0] = dex as u8;
            record[1..7].copy_from_slice(&[40, 50, 60, 70, 80, 90]);
            record[7] = 0x14;
            record[8] = 0x14;
            record[13] = [0x00, 0x1F, 0x7F, 0xFE, 0xFF][dex % 5];
            record[15] = 20;
            record[17] = [0x55, 0x66, 0x77][dex % 3];
            record[22] = 0;
            record
        })
        .collect();
    put(&mut rom, 0x60000, &base);
    let mut pointers = Vec::new();
    for dex in 1..=251usize {
        let n = [5usize, 6, 7][dex % 3];
        let front = 0x4000 + dex as u16 * 8;
        let back = 0x6000 + dex as u16 * 8;
        pointers.extend([0x13, front as u8, (front >> 8) as u8, 0x13]);
        pointers.extend(back.to_le_bytes());
        let need = n * n * 16 - 1;
        put(
            &mut rom,
            offset(0x49, front),
            &[0xE4 | (need >> 8) as u8, need as u8, dex as u8, 0xFF],
        );
        put(
            &mut rom,
            offset(0x49, back),
            &[0xE6, 0x3F, !(dex as u8), 0xFF],
        );
    }
    put(&mut rom, offset(0x48, 0x4000), &pointers);
    let mut unown = Vec::new();
    for letter in 0..26u16 {
        let front = 0x4000 + letter * 8;
        let back = 0x6000 + letter * 8;
        unown.push(0x14);
        unown.extend(front.to_le_bytes());
        unown.push(0x14);
        unown.extend(back.to_le_bytes());
        put(
            &mut rom,
            offset(0x4A, front),
            &[0xE5, 0x8F, 0x80 + letter as u8, 0xFF],
        );
        put(
            &mut rom,
            offset(0x4A, back),
            &[0xE6, 0x3F, 0x40 + letter as u8, 0xFF],
        );
    }
    put(&mut rom, offset(0x50, 0x4000), &unown);
    let bank = 9;
    put(
        &mut rom,
        offset(bank, 0x4000),
        &[
            0x6F, 0x26, 0x00, 0x29, 0x29, 0x29, 0x01, 0x00, 0x50, 0x09, 0xC9,
        ],
    );
    let palettes: Vec<u16> = (0..=251u16)
        .flat_map(|dex| [0x1000 + dex, 0x2000 + dex, 0x3000 + dex, 0x4000 + dex])
        .collect();
    put(&mut rom, offset(bank, 0x5000), &words(&palettes));
    put(
        &mut rom,
        offset(bank, 0x4020),
        &[
            0x6B, 0x26, 0x00, 0x29, 0x29, 0x11, 0x02, 0x60, 0x19, 0x2A, 0x66, 0x6F,
        ],
    );
    let mut landmarks = Vec::new();
    let mut names = Vec::new();
    for i in 0..0x60 {
        landmarks.extend([0, 0]);
        landmarks.extend((0x6400 + names.len() as u16).to_le_bytes());
        let mut name = en(&label("PLACE", i));
        if i == 3 {
            name = [en("NEW"), vec![0x1F], en("TOWN"), vec![0x25]].concat();
        }
        names.extend(name);
        names.push(0x50);
    }
    put(&mut rom, offset(bank, 0x6000), &landmarks);
    put(&mut rom, offset(bank, 0x6400), &names);
    type_table(&mut rom, offset(bank, 0x4040), 0x6800, 0x1C);
    rom
}

pub(crate) fn at(bank: usize, addr: u16) -> usize {
    bank * 0x4000 + usize::from(addr) - 0x4000
}

pub(crate) fn text(name: &str) -> Vec<u8> {
    let mut bytes = en(name);
    bytes.push(0x50);
    bytes
}

pub(crate) fn gen1_places(rom: &mut [u8]) {
    let bank = 0x1C;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0xFE, 0x25, 0x38, 0x0B, 0x01, 0x04, 0x00, 0x21, 0x00, 0x52, 0xBE, 0x38, 0x03, 0x09,
            0x18, 0xFA, 0x23, 0x18, 0x03, 0x21, 0x00, 0x50, 0x4F, 0x06, 0x00, 0x09, 0x09, 0x09,
        ],
    );
    put(rom, at(bank, 0x6000), &text("PALLET TOWN"));
    put(rom, at(bank, 0x6010), &text("ROUTE 1"));
    put(rom, at(bank, 0x6020), &text("PLACE"));
    put(rom, at(bank, 0x6030), &text("OAK LAB"));
    put(rom, at(bank, 0x6040), &text("MT MOON"));
    for map in 0..0x25u16 {
        let (coords, name): (u8, u16) = match map {
            0 => (0xB2, 0x6000),
            0x0C => (0x92, 0x6010),
            _ => (0x00, 0x6020),
        };
        let mut entry = vec![coords];
        entry.extend(name.to_le_bytes());
        put(rom, at(bank, 0x5000) + 3 * usize::from(map), &entry);
    }
    put(
        rom,
        at(bank, 0x5200),
        &[0x30, 0xB2, 0x30, 0x60, 0x40, 0x26, 0x40, 0x60, 0xFF],
    );
}

pub(crate) fn gen1_wild(rom: &mut [u8]) {
    let bank = 0x1D;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x21, 0x00, 0x41, 0xFA, 0x5E, 0xD3, 0x4F, 0x06, 0x00, 0x09, 0x09, 0x2A, 0x66, 0x6F,
            0x2A, 0xEA, 0x87, 0xD8, 0xA7, 0x28, 0x0A,
        ],
    );
    let mut pointers = Vec::new();
    for map in 0..0x30 {
        let pointer: u16 = if map == 0x0C { 0x4200 } else { 0x4300 };
        pointers.extend(pointer.to_le_bytes());
    }
    pointers.extend([0xFF, 0xFF]);
    put(rom, at(bank, 0x4100), &pointers);
    let mut grass = vec![25];
    for (level, species) in [
        (3, 0x24),
        (3, 0xA5),
        (4, 0x24),
        (4, 0xA5),
        (2, 0x24),
        (5, 0xA5),
        (5, 0x24),
        (6, 0xA5),
        (3, 0x24),
        (7, 0x24),
    ] {
        grass.extend([level, species]);
    }
    grass.push(0);
    put(rom, at(bank, 0x4200), &grass);
}

pub(crate) fn red_world_rom() -> Vec<u8> {
    let mut rom = red_rom();
    gen1_places(&mut rom);
    gen1_wild(&mut rom);
    rom
}

pub(crate) fn red_world() -> RomTables {
    RomTables::load(game(Version::Red, Language::English), &red_world_rom()).expect("tables")
}

pub(crate) fn gen2_places(rom: &mut [u8]) {
    let bank = 0x61;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x6B, 0x26, 0x00, 0x29, 0x29, 0x11, 0x02, 0x41, 0x19, 0x2A, 0x66, 0x6F,
        ],
    );
    put(rom, at(bank, 0x5000), &text("SPOT"));
    put(rom, at(bank, 0x5010), &text("NEW BARK TOWN"));
    put(rom, at(bank, 0x5020), &text("ROUTE 29"));
    put(rom, at(bank, 0x5030), &text("PALLET TOWN"));
    for i in 0..0x60u8 {
        let name: u16 = match i {
            1 => 0x5010,
            3 => 0x5020,
            0x2F => 0x5030,
            _ => 0x5000,
        };
        let mut entry = vec![8 + i, 16 + i];
        entry.extend(name.to_le_bytes());
        put(rom, at(bank, 0x4100) + 4 * usize::from(i), &entry);
    }
    put(
        rom,
        0x3800,
        &[
            0xC5, 0x05, 0x48, 0x06, 0x00, 0x21, 0x00, 0x40, 0x09, 0x09, 0x2A, 0x66, 0x6F, 0xC1,
            0x0D,
        ],
    );
    let bank = 0x62;
    for group in 0..26u16 {
        let start = 0x4100 + group * 0x40;
        put(
            rom,
            at(bank, 0x4000) + 2 * usize::from(group),
            &start.to_le_bytes(),
        );
        let landmarks: [u8; 3] = match group + 1 {
            24 => [1, 3, 0],
            1 => [2, 0x2F, 2],
            _ => [2, 2, 2],
        };
        for (i, landmark) in landmarks.into_iter().enumerate() {
            put(
                rom,
                at(bank, start) + 9 * i,
                &[0x25, 1, 1, 0x00, 0x40, landmark, 0x10, 0, 0],
            );
        }
    }
}

pub(crate) fn gen2_wild(rom: &mut [u8]) {
    let bank = 0x63;
    put(
        rom,
        at(bank, 0x4000),
        &[
            0x21, 0x00, 0x41, 0x11, 0x00, 0x42, 0xCD, 0x00, 0x30, 0x01, 0x2F, 0x00,
        ],
    );
    put(
        rom,
        at(bank, 0x4010),
        &[
            0x21, 0x00, 0x43, 0x11, 0x00, 0x44, 0xCD, 0x00, 0x30, 0x01, 0x09, 0x00,
        ],
    );
    let mut johto = vec![24, 2, 10, 10, 5];
    let morning = [(2, 0x10); 7];
    let day = [
        (3, 0x10),
        (3, 0x13),
        (4, 0x10),
        (5, 0x13),
        (4, 0x10),
        (6, 0x10),
        (7, 0xA1),
    ];
    let night: Vec<(u8, u8)> = (3..10).map(|level| (level, 0xA3)).collect();
    for (level, species) in morning.iter().chain(&day).chain(&night) {
        johto.extend([*level, *species]);
    }
    johto.push(0xFF);
    put(rom, at(bank, 0x4100), &johto);
    let mut kanto = vec![1, 1, 10, 10, 10];
    kanto.extend([5, 0x13].repeat(21));
    kanto.push(0xFF);
    put(rom, at(bank, 0x4200), &kanto);
    put(
        rom,
        at(bank, 0x4300),
        &[24, 2, 4, 20, 0x81, 15, 0x81, 20, 0x48, 0xFF],
    );
    put(rom, at(bank, 0x4400), &[0xFF]);
}

pub(crate) fn crystal_world_rom() -> Vec<u8> {
    let mut rom = crystal_rom();
    gen2_places(&mut rom);
    gen2_wild(&mut rom);
    rom
}

pub(crate) fn crystal_world() -> RomTables {
    RomTables::load(
        game(Version::Crystal, Language::English),
        &crystal_world_rom(),
    )
    .expect("tables")
}

pub(crate) fn location(group: u8, map: u8, outside: Option<(u8, u8)>) -> Location {
    Location {
        group,
        map,
        x: 0,
        y: 0,
        outside,
    }
}
