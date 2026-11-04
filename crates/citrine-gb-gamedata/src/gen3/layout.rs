use super::Layout;

pub(crate) const NAME_LEN: usize = 8;
pub(crate) const DEX_OWNED: usize = 0x28;
pub(crate) const DEX_SEEN: usize = 0x5C;
pub(crate) const DEX_LEN: usize = 49;
pub(crate) const GAME_CODE: usize = 0xAC;
pub(crate) const BOX_DATA: usize = 0x04;
pub(crate) const BOX_NAMES: usize = 0x8344;

pub(crate) struct Offsets {
    pub key: Option<usize>,
    pub small_size: usize,
    pub rival_size: usize,
    pub team_size: usize,
    pub team: usize,
    pub money: usize,
    pub pc_items: (usize, usize),
    pub items: (usize, usize),
    pub key_items: (usize, usize),
    pub balls: (usize, usize),
    pub tms_hms: (usize, usize),
    pub berries: (usize, usize),
    pub flags: usize,
    pub pokedex_flag: usize,
    pub badge_flag: usize,
    pub champion_flag: usize,
}

const RUBY_SAPPHIRE: Offsets = Offsets {
    key: None,
    small_size: 0x890,
    rival_size: 0xC40,
    team_size: 0x234,
    team: 0x238,
    money: 0x490,
    pc_items: (0x498, 50),
    items: (0x560, 20),
    key_items: (0x5B0, 20),
    balls: (0x600, 16),
    tms_hms: (0x640, 64),
    berries: (0x740, 46),
    flags: 0x1220,
    pokedex_flag: 0x801,
    badge_flag: 0x807,
    champion_flag: 0x804,
};

const EMERALD: Offsets = Offsets {
    key: Some(GAME_CODE),
    small_size: 0xF2C,
    rival_size: 0xF08,
    team_size: 0x234,
    team: 0x238,
    money: 0x490,
    pc_items: (0x498, 50),
    items: (0x560, 30),
    key_items: (0x5D8, 30),
    balls: (0x650, 16),
    tms_hms: (0x690, 64),
    berries: (0x790, 46),
    flags: 0x1270,
    pokedex_flag: 0x861,
    badge_flag: 0x867,
    champion_flag: 0x864,
};

const FIRE_RED_LEAF_GREEN: Offsets = Offsets {
    key: Some(0xF20),
    small_size: 0xF24,
    rival_size: 0xEE8,
    team_size: 0x34,
    team: 0x38,
    money: 0x290,
    pc_items: (0x298, 30),
    items: (0x310, 42),
    key_items: (0x3B8, 30),
    balls: (0x430, 13),
    tms_hms: (0x464, 58),
    berries: (0x54C, 43),
    flags: 0xEE0,
    pokedex_flag: 0x829,
    badge_flag: 0x820,
    champion_flag: 0x82C,
};

impl Offsets {
    pub fn of(layout: Layout) -> &'static Offsets {
        match layout {
            Layout::RubySapphire => &RUBY_SAPPHIRE,
            Layout::Emerald => &EMERALD,
            Layout::FireRedLeafGreen => &FIRE_RED_LEAF_GREEN,
        }
    }
}
