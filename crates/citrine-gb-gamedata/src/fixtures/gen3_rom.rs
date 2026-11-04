use super::gen3::gen3_text;

pub const ROM: usize = 0x40000;
pub const SPECIES: usize = 412;
pub const MOVES: usize = 355;
pub const ABILITIES: usize = 78;
pub const ITEMS: usize = 377;
pub const UNUSED_ITEM: usize = 60;
pub const SHEDINJA: u16 = 317;
pub const MACHINES: u16 = 58;

#[derive(Debug, Clone, Copy)]
pub struct Spots {
    pub species: usize,
    pub moves: usize,
    pub abilities: usize,
    pub types: usize,
    pub items: usize,
    pub national: usize,
    pub base: usize,
    pub battle_moves: usize,
    pub machines: usize,
    pub pictures: usize,
}

pub const SPOTS: Spots = Spots {
    species: 0x1000,
    moves: 0x3000,
    abilities: 0x5000,
    types: 0x5800,
    items: 0x6000,
    national: 0xB002,
    base: 0xC000,
    battle_moves: 0xF000,
    machines: 0x1_0200,
    pictures: 0x1_1000,
};

pub const MOVED: Spots = Spots {
    species: 0x2_0001,
    moves: 0x2_3003,
    abilities: 0x2_6005,
    types: 0x2_7007,
    items: 0x2_8000,
    national: 0x3_0000,
    base: 0x3_1004,
    battle_moves: 0x3_4000,
    machines: 0x3_6002,
    pictures: 0x3_7000,
};

pub fn national_for(species: u16) -> u16 {
    match species {
        0 => 0,
        1..=251 => species,
        252..=276 => 387 + species - 252,
        411 => 358,
        277..=382 => species - 25,
        _ => species - 24,
    }
}

fn record(text: &str, size: usize) -> Vec<u8> {
    let mut bytes = gen3_text(text);
    bytes.push(0xFF);
    bytes.resize(size, 0);
    bytes
}

fn put(rom: &mut [u8], at: usize, bytes: &[u8]) {
    rom[at..at + bytes.len()].copy_from_slice(bytes);
}

fn table(rom: &mut [u8], at: usize, size: usize, records: impl Iterator<Item = Vec<u8>>) {
    for (i, bytes) in records.enumerate() {
        put(rom, at + i * size, &bytes);
    }
}

pub fn species_name(species: u16) -> String {
    match species {
        0 => "??????????".into(),
        1 => "BULBASAUR".into(),
        277 => "TREECKO".into(),
        411 => "CHIMECHO".into(),
        _ => format!("MON{species}"),
    }
}

pub fn base_record(species: u16) -> [u8; 28] {
    let mut r = [0u8; 28];
    if species == 0 {
        return r;
    }
    let s = species as u8;
    r[..6].copy_from_slice(&[45, 49, 49, 45, 65, 65]);
    r[0] = r[0].wrapping_add(s % 50);
    r[6] = (species % 18) as u8;
    r[7] = ((species + 3) % 18) as u8;
    r[8] = 45;
    r[9] = 64;
    r[16] = [0, 31, 127, 254, 255][usize::from(species) % 5];
    r[19] = (species % 6) as u8;
    r[20] = 1;
    r[21] = 7;
    r[22] = (species % 77 + 1) as u8;
    r[23] = if species.is_multiple_of(2) { 0 } else { 2 };
    r
}

pub fn move_record(id: u16) -> [u8; 12] {
    let m = id as u8;
    match id {
        0 => [0; 12],
        1 => [0, 40, 0, 100, 35, 0, 0, 0, 0x33, 0, 0, 0],
        2 => [43, 50, 1, 100, 25, 0, 0, 0, 0x33, 0, 0, 0],
        _ => [
            0,
            40 + m % 60,
            (id % 18) as u8,
            100 - m % 30,
            5 * (1 + m % 8),
            0,
            0,
            0,
            0x12,
            0,
            0,
            0,
        ],
    }
}

pub fn machine_move(index: u16) -> u16 {
    match index {
        0 => 264,
        1 => 337,
        2 => 352,
        3 => 347,
        _ => 100 + index,
    }
}

pub const PICTURE_BYTE: u8 = 0x21;
pub const RED: u16 = 0x001F;
pub const BLUE: u16 = 0x7C00;

pub fn literal_lz77(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x10];
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
    for chunk in data.chunks(8) {
        out.push(0);
        out.extend_from_slice(chunk);
    }
    out
}

fn palette_bytes(first: u16) -> Vec<u8> {
    let mut colors = [0u16; 16];
    colors[1] = first;
    colors[2] = 0x7FFF;
    colors.iter().flat_map(|c| c.to_le_bytes()).collect()
}

fn put_pictures(rom: &mut [u8], at: usize) {
    let (front, palettes, shiny) = (at, at + 0x1000, at + 0x2000);
    let (pic, normal, bright) = (at + 0x3000, at + 0x4000, at + 0x4100);
    put(rom, pic, &literal_lz77(&[PICTURE_BYTE; 0x800]));
    put(rom, normal, &literal_lz77(&palette_bytes(RED)));
    put(rom, bright, &literal_lz77(&palette_bytes(BLUE)));
    let address = |offset: usize| (0x0800_0000 + offset as u32).to_le_bytes();
    for species in 0..SPECIES as u16 {
        let entry = usize::from(species) * 8;
        put(rom, front + entry, &address(pic));
        put(rom, front + entry + 4, &0x800u16.to_le_bytes());
        put(rom, front + entry + 6, &species.to_le_bytes());
        put(rom, palettes + entry, &address(normal));
        put(rom, palettes + entry + 4, &species.to_le_bytes());
        put(rom, shiny + entry, &address(bright));
        put(rom, shiny + entry + 4, &(species + 500).to_le_bytes());
    }
}

pub fn gen3_rom(code: &[u8; 4], revision: u8, at: Spots) -> Vec<u8> {
    let mut rom = vec![0u8; ROM];
    put(&mut rom, 0xA0, b"POKEMON TEST");
    put(&mut rom, 0xAC, code);
    put(&mut rom, 0xB0, b"01");
    rom[0xBC] = revision;
    table(
        &mut rom,
        at.species,
        11,
        (0..SPECIES as u16).map(|s| record(&species_name(s), 11)),
    );
    let mut moves = vec![vec![0xAE, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0]];
    moves.push(record("POUND", 13));
    moves.extend((2..MOVES).map(|m| record(&format!("MOVE{m}"), 13)));
    table(&mut rom, at.moves, 13, moves.into_iter());
    table(
        &mut rom,
        at.abilities,
        13,
        (0..ABILITIES).map(|a| match a {
            0 => record("-------", 13),
            1 => record("STENCH", 13),
            _ => record(&format!("SKILL{a}"), 13),
        }),
    );
    table(
        &mut rom,
        at.types,
        7,
        (0..18).map(|t| match t {
            0 => record("NORMAL", 7),
            9 => record("???", 7),
            17 => record("DARK", 7),
            _ => record(&format!("TYPE{t}"), 7),
        }),
    );
    table(
        &mut rom,
        at.items,
        44,
        (0..ITEMS).map(|i| {
            let mut item = match i {
                0 => record("????????", 44),
                1 => record("MASTER BALL", 44),
                UNUSED_ITEM => record("????????", 44),
                _ => record(&format!("ITEM{i}"), 44),
            };
            let id = if i == UNUSED_ITEM { 0 } else { i as u16 };
            item[14..16].copy_from_slice(&id.to_le_bytes());
            item
        }),
    );
    table(
        &mut rom,
        at.national,
        2,
        (1..SPECIES as u16).map(|s| national_for(s).to_le_bytes().to_vec()),
    );
    table(
        &mut rom,
        at.base,
        28,
        (0..SPECIES as u16).map(|s| base_record(s).to_vec()),
    );
    table(
        &mut rom,
        at.battle_moves,
        12,
        (0..MOVES as u16).map(|m| move_record(m).to_vec()),
    );
    table(
        &mut rom,
        at.machines,
        2,
        (0..MACHINES).map(|i| machine_move(i).to_le_bytes().to_vec()),
    );
    put_pictures(&mut rom, at.pictures);
    rom
}
