use crate::game::{Game, Generation, Language, Version};

pub(crate) struct Layout {
    pub sram_size: usize,
    pub live_starts: &'static [usize],
}

impl Layout {
    pub fn of(game: Game) -> Self {
        match game.generation() {
            Generation::One => {
                let layout = Gen1Layout::of(game);
                Self {
                    sram_size: 0x8000,
                    live_starts: layout.live_starts,
                }
            }
            Generation::Two => {
                let layout = Gen2Layout::of(game);
                Self {
                    sram_size: layout.sram_size,
                    live_starts: layout.live_starts,
                }
            }
        }
    }
}

pub(crate) struct Gen1Layout {
    pub player_name: usize,
    pub pokedex_owned: usize,
    pub pokedex_seen: usize,
    pub bag: usize,
    pub money: usize,
    pub badges: usize,
    pub player_id: usize,
    pub pc_items: usize,
    pub current_box: usize,
    pub coins: usize,
    pub play_time: usize,
    pub party: usize,
    pub box_data: usize,
    pub checksum: usize,
    pub box_slots: usize,
    pub boxes_per_bank: usize,
    pub live_starts: &'static [usize],
}

pub(crate) const GEN1_INTERNATIONAL: Gen1Layout = Gen1Layout {
    player_name: 0x2598,
    pokedex_owned: 0x25A3,
    pokedex_seen: 0x25B6,
    bag: 0x25C9,
    money: 0x25F3,
    badges: 0x2602,
    player_id: 0x2605,
    pc_items: 0x27E6,
    current_box: 0x284C,
    coins: 0x2850,
    play_time: 0x2CED,
    party: 0x2F2C,
    box_data: 0x30C0,
    checksum: 0x3523,
    box_slots: 20,
    boxes_per_bank: 6,
    live_starts: &[0x2598, 0x25A3, 0x2F2C, 0x30C0],
};

pub(crate) const GEN1_JAPANESE: Gen1Layout = Gen1Layout {
    player_name: 0x2598,
    pokedex_owned: 0x259E,
    pokedex_seen: 0x25B1,
    bag: 0x25C4,
    money: 0x25EE,
    badges: 0x25F8,
    player_id: 0x25FB,
    pc_items: 0x27DC,
    current_box: 0x2842,
    coins: 0x2846,
    play_time: 0x2CA0,
    party: 0x2ED5,
    box_data: 0x302D,
    checksum: 0x3594,
    box_slots: 30,
    boxes_per_bank: 4,
    live_starts: &[0x2598, 0x259E, 0x2ED5, 0x302D],
};

impl Gen1Layout {
    pub fn of(game: Game) -> &'static Self {
        if game.language == Language::Japanese {
            &GEN1_JAPANESE
        } else {
            &GEN1_INTERNATIONAL
        }
    }
}

pub(crate) struct Gen2Layout {
    pub sram_size: usize,
    pub main: (usize, usize),
    pub main_checksum: usize,
    pub main_checks: (usize, usize),
    pub backup: &'static [(usize, usize, usize)],
    pub backup_checksum: usize,
    pub backup_checks: (usize, usize),
    pub player_id: usize,
    pub player_name: usize,
    pub play_time: usize,
    pub money: usize,
    pub johto_badges: usize,
    pub kanto_badges: usize,
    pub tms_hms: usize,
    pub items: usize,
    pub key_items: usize,
    pub balls: usize,
    pub pc_items: usize,
    pub current_box: usize,
    pub box_names: usize,
    pub box_name_len: usize,
    pub party: usize,
    pub pokedex_caught: usize,
    pub pokedex_seen: usize,
    pub box_data: usize,
    pub boxes: &'static [usize],
    pub box_slots: usize,
    pub gender: Option<usize>,
    pub visited_spawns: usize,
    pub live_starts: &'static [usize],
}

const BOXES_INTERNATIONAL: [usize; 14] = [
    0x4000, 0x4450, 0x48A0, 0x4CF0, 0x5140, 0x5590, 0x59E0, 0x6000, 0x6450, 0x68A0, 0x6CF0, 0x7140,
    0x7590, 0x79E0,
];

const BOXES_JAPANESE: [usize; 9] = [
    0x4000, 0x454A, 0x4A94, 0x4FDE, 0x5528, 0x5A72, 0x6000, 0x654A, 0x6A94,
];

pub(crate) const GS_INTERNATIONAL: Gen2Layout = Gen2Layout {
    sram_size: 0x8000,
    main: (0x2009, 0x2D69),
    main_checksum: 0x2D69,
    main_checks: (0x2008, 0x2D6B),
    backup: &[
        (0x2009, 0x15C7, 0x226),
        (0x222F, 0x3D96, 0x1AA),
        (0x23D9, 0x0C6B, 0x47D),
        (0x2856, 0x7E39, 0x34),
        (0x288A, 0x10E8, 0x4DF),
    ],
    backup_checksum: 0x7E6D,
    backup_checks: (0x7E38, 0x7E6F),
    player_id: 0x2009,
    player_name: 0x200B,
    play_time: 0x2053,
    money: 0x23DB,
    johto_badges: 0x23E4,
    kanto_badges: 0x23E5,
    tms_hms: 0x23E6,
    items: 0x241F,
    key_items: 0x2449,
    balls: 0x2464,
    pc_items: 0x247E,
    current_box: 0x2724,
    box_names: 0x2727,
    box_name_len: 9,
    party: 0x288A,
    pokedex_caught: 0x2A4C,
    pokedex_seen: 0x2A6C,
    box_data: 0x2D6C,
    boxes: &BOXES_INTERNATIONAL,
    box_slots: 20,
    gender: None,
    visited_spawns: 0x2856,
    live_starts: &[0x2009, 0x2856, 0x288A],
};

pub(crate) const CRYSTAL_INTERNATIONAL: Gen2Layout = Gen2Layout {
    sram_size: 0x8000,
    main: (0x2009, 0x2B83),
    main_checksum: 0x2D0D,
    main_checks: (0x2008, 0x2D0F),
    backup: &[(0x2009, 0x1209, 0xB7A)],
    backup_checksum: 0x1F0D,
    backup_checks: (0x1208, 0x1F0F),
    player_id: 0x2009,
    player_name: 0x200B,
    play_time: 0x2052,
    money: 0x23DC,
    johto_badges: 0x23E5,
    kanto_badges: 0x23E6,
    tms_hms: 0x23E7,
    items: 0x2420,
    key_items: 0x244A,
    balls: 0x2465,
    pc_items: 0x247F,
    current_box: 0x2700,
    box_names: 0x2703,
    box_name_len: 9,
    party: 0x2865,
    pokedex_caught: 0x2A27,
    pokedex_seen: 0x2A47,
    box_data: 0x2D10,
    boxes: &BOXES_INTERNATIONAL,
    box_slots: 20,
    gender: Some(0x3E3D),
    visited_spawns: 0x2833,
    live_starts: &[0x2009, 0x2833, 0x2865, 0x3E3D],
};

pub(crate) const GS_JAPANESE: Gen2Layout = Gen2Layout {
    sram_size: 0x8000,
    main: (0x2009, 0x2C8C),
    main_checksum: 0x2D0D,
    main_checks: (0x2008, 0x2D0F),
    backup: &[(0x2009, 0x7209, 0xC83)],
    backup_checksum: 0x7F0D,
    backup_checks: (0x7208, 0x7F0F),
    player_id: 0x2009,
    player_name: 0x200B,
    play_time: 0x2034,
    money: 0x23BC,
    johto_badges: 0x23C5,
    kanto_badges: 0x23C6,
    tms_hms: 0x23C7,
    items: 0x2400,
    key_items: 0x242A,
    balls: 0x2445,
    pc_items: 0x245F,
    current_box: 0x2705,
    box_names: 0x2708,
    box_name_len: 9,
    party: 0x283E,
    pokedex_caught: 0x29CE,
    pokedex_seen: 0x29EE,
    box_data: 0x2D10,
    boxes: &BOXES_JAPANESE,
    box_slots: 30,
    gender: None,
    visited_spawns: 0x280A,
    live_starts: &[0x2009, 0x283E],
};

pub(crate) const CRYSTAL_JAPANESE: Gen2Layout = Gen2Layout {
    sram_size: 0x10000,
    main: (0x2009, 0x2AE3),
    main_checksum: 0x2D0D,
    main_checks: (0x2008, 0x2D0F),
    backup: &[(0x2009, 0x7209, 0xADA)],
    backup_checksum: 0x7F0D,
    backup_checks: (0x7208, 0x7F0F),
    player_id: 0x2009,
    player_name: 0x200B,
    play_time: 0x2034,
    money: 0x23BE,
    johto_badges: 0x23C7,
    kanto_badges: 0x23C8,
    tms_hms: 0x23C9,
    items: 0x2402,
    key_items: 0x242C,
    balls: 0x2447,
    pc_items: 0x2461,
    current_box: 0x26E2,
    box_names: 0x26E5,
    box_name_len: 9,
    party: 0x281A,
    pokedex_caught: 0x29AA,
    pokedex_seen: 0x29CA,
    box_data: 0x2D10,
    boxes: &BOXES_JAPANESE,
    box_slots: 30,
    gender: Some(0x8000),
    visited_spawns: 0x27E8,
    live_starts: &[0x2009, 0x281A],
};

pub(crate) const GS_KOREAN: Gen2Layout = Gen2Layout {
    sram_size: 0x8000,
    main: (0x2009, 0x2DAB),
    main_checksum: 0x2DAB,
    main_checks: (0x2008, 0x2DAD),
    backup: &[],
    backup_checksum: 0,
    backup_checks: (0, 0),
    player_id: 0x2009,
    player_name: 0x200B,
    play_time: 0x204D,
    money: 0x23D3,
    johto_badges: 0x23DC,
    kanto_badges: 0x23DD,
    tms_hms: 0x23DE,
    items: 0x2417,
    key_items: 0x2441,
    balls: 0x245C,
    pc_items: 0x2476,
    current_box: 0x26FC,
    box_names: 0x26FF,
    box_name_len: 17,
    party: 0x28CC,
    pokedex_caught: 0x2A8E,
    pokedex_seen: 0x2AAE,
    box_data: 0x2DAE,
    boxes: &BOXES_INTERNATIONAL,
    box_slots: 20,
    gender: None,
    visited_spawns: 0x289A,
    live_starts: &[0x2009, 0x28CC],
};

impl Gen2Layout {
    pub fn of(game: Game) -> &'static Self {
        match (game.version, game.language) {
            (Version::Crystal, Language::Japanese) => &CRYSTAL_JAPANESE,
            (Version::Crystal, _) => &CRYSTAL_INTERNATIONAL,
            (_, Language::Japanese) => &GS_JAPANESE,
            (_, Language::Korean) => &GS_KOREAN,
            _ => &GS_INTERNATIONAL,
        }
    }
}
