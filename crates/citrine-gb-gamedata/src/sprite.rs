use crate::base::BaseStats;
use crate::game::{Game, Generation, Version};
use crate::pic::{self, Sprite, SpriteError};
use crate::rom::BANK_SIZE;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};

const GEN2_PIC_RECORD: usize = 6;
const UNOWN_LETTERS: usize = 26;
const GEN2_BACK_SIZE: u8 = 6;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpriteTables {
    generation: Option<Generation>,
    pics: Vec<Option<Pics>>,
    unown: Vec<Pics>,
    palettes: Vec<[Palette; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pics {
    front: usize,
    back: usize,
    size: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub colors: [u16; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Back,
}

impl Palette {
    pub const GRAY: Palette = Palette {
        colors: [0x7FFF, 0x56B5, 0x294A, 0x0000],
    };

    pub fn rgb(&self, shade: u8) -> [u8; 3] {
        let color = self.colors[usize::from(shade & 3)];
        let channel = |shift: u16| {
            let value = ((color >> shift) & 0x1F) as u8;
            (value << 3) | (value >> 2)
        };
        [channel(0), channel(5), channel(10)]
    }

    fn read(rom: &[u8], at: usize) -> Option<Self> {
        let mut colors = [0; 4];
        for (i, color) in colors.iter_mut().enumerate() {
            *color = word(rom, at + 2 * i).filter(|&c| c < 0x8000)?;
        }
        Some(Self { colors })
    }

    fn middle(rom: &[u8], at: usize) -> Option<Self> {
        let light = word(rom, at).filter(|&c| c < 0x8000)?;
        let dark = word(rom, at + 2).filter(|&c| c < 0x8000)?;
        Some(Self {
            colors: [0x7FFF, light, dark, 0x0000],
        })
    }
}

impl SpriteTables {
    pub(crate) fn load(game: Game, rom: &[u8], base: &[BaseStats], dex_order: &[u8]) -> Self {
        match game.generation() {
            Generation::One => load_gen1(game, rom, base, dex_order),
            Generation::Two => load_gen2(rom, base),
        }
    }

    pub(crate) fn sprite(
        &self,
        rom: &[u8],
        dex: u16,
        unown: Option<usize>,
        side: Side,
    ) -> Result<Sprite, SpriteError> {
        let generation = self.generation.ok_or(SpriteError::NoTable)?;
        let pics = match unown {
            Some(letter) if !self.unown.is_empty() => {
                *self.unown.get(letter).ok_or(SpriteError::NoSpecies)?
            }
            _ => usize::from(dex)
                .checked_sub(1)
                .and_then(|index| self.pics.get(index).copied().flatten())
                .ok_or(SpriteError::NoSpecies)?,
        };
        match (generation, side) {
            (Generation::One, Side::Front) => pic::decode_gen1(rom, pics.front),
            (Generation::One, Side::Back) => pic::decode_gen1(rom, pics.back),
            (Generation::Two, Side::Front) => {
                let size = pics.size & 0x0F;
                if !(1..=7).contains(&size) {
                    return Err(SpriteError::BadData);
                }
                pic::decode_gen2(rom, pics.front, size, size)
            }
            (Generation::Two, Side::Back) => {
                pic::decode_gen2(rom, pics.back, GEN2_BACK_SIZE, GEN2_BACK_SIZE)
            }
        }
    }

    pub(crate) fn palette(&self, dex: u16, shiny: bool) -> Option<Palette> {
        let pair = self.palettes.get(usize::from(dex).checked_sub(1)?)?;
        Some(pair[usize::from(shiny)])
    }
}

fn load_gen1(game: Game, rom: &[u8], base: &[BaseStats], dex_order: &[u8]) -> SpriteTables {
    let Some(banks) = gen1_pic_banks(rom) else {
        return SpriteTables::default();
    };
    let pics = base
        .iter()
        .map(|stats| {
            let species = dex_order
                .iter()
                .position(|&dex| dex == stats.dex)
                .map(|index| index as u8 + 1)?;
            let bank = banks.bank_for(species);
            Some(Pics {
                front: banked(rom, bank, stats.front_pic)?,
                back: banked(rom, bank, stats.back_pic)?,
                size: stats.pic_size,
            })
        })
        .collect();
    let palettes = gen1_palettes(game, rom, base.len()).unwrap_or_default();
    SpriteTables {
        generation: Some(Generation::One),
        pics,
        unown: Vec::new(),
        palettes,
    }
}

struct Gen1Banks {
    rules: Vec<(u8, bool, u8)>,
    otherwise: u8,
}

impl Gen1Banks {
    fn bank_for(&self, species: u8) -> usize {
        let bank = self
            .rules
            .iter()
            .find(|&&(value, equal, _)| {
                if equal {
                    species == value
                } else {
                    species < value
                }
            })
            .map_or(self.otherwise, |rule| rule.2);
        usize::from(bank)
    }
}

fn gen1_pic_banks(rom: &[u8]) -> Option<Gen1Banks> {
    let start = Pattern::new("FA ?? ?? 47 FE");
    let home = &rom[..rom.len().min(BANK_SIZE)];
    start
        .find_all(home)
        .find_map(|at| parse_gen1_banks(home, at + 4))
}

fn parse_gen1_banks(code: &[u8], mut at: usize) -> Option<Gen1Banks> {
    let rule = Pattern::new("FE ?? 3E ?? ?? ??");
    let mut rules = Vec::new();
    loop {
        if byte(code, at) == Some(0x78) {
            at += 1;
        }
        if rule.matches(code, at) {
            let equal = match code[at + 4] {
                0x28 => true,
                0x38 => false,
                _ => return None,
            };
            rules.push((code[at + 1], equal, code[at + 3]));
            at += rule.len();
            continue;
        }
        if byte(code, at) == Some(0x3E) && byte(code, at + 2) == Some(0xC3) {
            let otherwise = code[at + 1];
            let ordered = rules.iter().filter(|rule| !rule.1).count() >= 3;
            return ordered.then_some(Gen1Banks { rules, otherwise });
        }
        return None;
    }
}

fn gen1_palettes(game: Game, rom: &[u8], count: usize) -> Option<Vec<[Palette; 2]>> {
    let ids = Pattern::new("5F 16 00 21 ?? ?? 19 7E C9")
        .find_all(rom)
        .find_map(|at| {
            let table = same_bank(rom, at, 4)?;
            let ids = rom.get(table..table + count + 1)?;
            let distinct = ids.iter().collect::<std::collections::HashSet<_>>().len();
            (ids.iter().all(|&id| id < 0x30) && distinct >= 8).then_some((table, ids))
        })?;
    let (table, ids) = ids;
    let colors = gen1_cgb_colors(game, rom, bank_of(table))
        .or_else(|| gen1_sgb_colors(rom, bank_of(table)))?;
    ids[1..]
        .iter()
        .map(|&id| {
            let palette = Palette::read(rom, colors + usize::from(id) * 8)?;
            Some([palette, palette])
        })
        .collect()
}

fn gen1_sgb_colors(rom: &[u8], bank: usize) -> Option<usize> {
    let code = Pattern::new("11 ?? ?? 21 ?? ?? CD");
    let start = bank * BANK_SIZE;
    let window = rom.get(start..start + BANK_SIZE)?;
    code.find_all(window).find_map(|at| {
        let packet = same_bank(rom, start + at, 1)?;
        (byte(rom, packet)? == 0x59).then_some(())?;
        same_bank(rom, start + at, 4)
    })
}

fn gen1_cgb_colors(game: Game, rom: &[u8], bank: usize) -> Option<usize> {
    if game.version != Version::Yellow {
        return None;
    }
    let code = Pattern::new("6F AF 67 29 29 29 11 ?? ?? 19");
    let start = bank * BANK_SIZE;
    let window = rom.get(start..start + BANK_SIZE)?;
    code.find_all(window)
        .find_map(|at| same_bank(rom, start + at, 7))
}

fn load_gen2(rom: &[u8], base: &[BaseStats]) -> SpriteTables {
    let banks = gen2_bank_map(rom);
    let Some(table) = find_gen2_pic_table(rom, &banks, base.len()) else {
        return SpriteTables::default();
    };
    let size = |dex: usize| base.get(dex).map_or(0, |stats| stats.pic_size);
    let pics = (0..base.len())
        .map(|i| gen2_pics(rom, &banks, table + i * GEN2_PIC_RECORD, size(i)))
        .collect();
    let unown_size = size(200);
    let unown = find_unown_table(rom, &banks, table)
        .map(|at| {
            (0..UNOWN_LETTERS)
                .filter_map(|i| gen2_pics(rom, &banks, at + i * GEN2_PIC_RECORD, unown_size))
                .collect()
        })
        .unwrap_or_default();
    SpriteTables {
        generation: Some(Generation::Two),
        pics,
        unown,
        palettes: gen2_palettes(rom, base.len()).unwrap_or_default(),
    }
}

fn gen2_bank_map(rom: &[u8]) -> [u8; 256] {
    let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
    let crystal = Pattern::new("E5 C5 D6 ?? 4F 06 00 21 ?? ?? 09 7E C1 E1 C9");
    if let Some(at) = crystal
        .find_all(rom)
        .find(|&at| same_bank(rom, at, 8) == Some(at + crystal.len()))
    {
        let table = at + crystal.len();
        let first = rom[at + 3];
        for i in 0..24 {
            if let Some(bank) = byte(rom, table + i) {
                map[usize::from(first) + i] = bank;
            }
        }
        return map;
    }
    let pairs = Pattern::new("E5 C5 47 21 ?? ?? 2A FE FF 28 ?? 23 B8 20");
    if let Some(at) = pairs.find_all(rom).next()
        && let Some(table) = same_bank(rom, at, 4)
    {
        let mut at = table;
        while let (Some(from), Some(to)) = (byte(rom, at), byte(rom, at + 1)) {
            if from == 0xFF {
                break;
            }
            map[usize::from(from)] = to;
            at += 2;
        }
    }
    map
}

fn gen2_pointer(rom: &[u8], banks: &[u8; 256], at: usize) -> Option<usize> {
    let bank = banks[usize::from(byte(rom, at)?)];
    if bank == 0 {
        return None;
    }
    banked(rom, usize::from(bank), word(rom, at + 1)?)
}

fn gen2_pics(rom: &[u8], banks: &[u8; 256], at: usize, size: u8) -> Option<Pics> {
    Some(Pics {
        front: gen2_pointer(rom, banks, at)?,
        back: gen2_pointer(rom, banks, at + 3)?,
        size,
    })
}

fn valid_pic_table(rom: &[u8], banks: &[u8; 256], at: usize, count: usize) -> bool {
    let allowed = count / 64;
    let mut missing = 0;
    let mut fronts = std::collections::HashSet::new();
    for i in 0..count {
        let entry = at + i * GEN2_PIC_RECORD;
        let pointers = matches!(byte(rom, entry + 2), Some(0x40..=0x7F))
            && matches!(byte(rom, entry + 5), Some(0x40..=0x7F));
        match gen2_pointer(rom, banks, entry).filter(|_| pointers) {
            Some(front) => {
                fronts.insert(front);
            }
            None => {
                missing += 1;
                if missing > allowed || i == 0 || i + 1 == count {
                    return false;
                }
            }
        }
    }
    fronts.len() * 5 >= count * 4
}

fn find_gen2_pic_table(rom: &[u8], banks: &[u8; 256], count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    (0..rom.len().saturating_sub(count * GEN2_PIC_RECORD))
        .find(|&at| valid_pic_table(rom, banks, at, count))
}

fn find_unown_table(rom: &[u8], banks: &[u8; 256], main: usize) -> Option<usize> {
    let choice = Pattern::new("16 ?? FE C9 20 ??");
    choice.find_all(rom).find_map(|at| {
        let skip = at + choice.len() + usize::from(rom[at + 5]);
        let unown_bank = usize::from(byte(rom, skip.checked_sub(1)?)?);
        let main_bank = usize::from(rom[at + 1]);
        if byte(rom, skip - 2)? != 0x16 || main_bank != bank_of(main) {
            return None;
        }
        let table = unown_bank * BANK_SIZE + main % BANK_SIZE;
        valid_pic_table(rom, banks, table, UNOWN_LETTERS).then_some(table)
    })
}

fn gen2_palettes(rom: &[u8], count: usize) -> Option<Vec<[Palette; 2]>> {
    Pattern::new("6F 26 00 29 29 29 01 ?? ?? 09 C9")
        .find_all(rom)
        .find_map(|at| {
            let table = same_bank(rom, at, 7)?;
            (1..=count)
                .map(|dex| {
                    let entry = table + dex * 8;
                    Some([
                        Palette::middle(rom, entry)?,
                        Palette::middle(rom, entry + 4)?,
                    ])
                })
                .collect()
        })
}
