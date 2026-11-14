use crate::base::BaseStats;
use crate::game::{Game, Generation, Version};
use crate::pic::{self, Sprite, SpriteError};
use crate::rom::BANK_SIZE;
use crate::scan::{Pattern, bank_of, banked, byte, same_bank, word};

const GEN2_PIC_RECORD: usize = 6;
const UNOWN_LETTERS: usize = 26;
const GEN2_BACK_SIZE: u8 = 6;
const GEN1_DEX: usize = 151;

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
            Generation::Three | Generation::Four | Generation::Five => Self::default(),
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
            (Generation::Three | Generation::Four | Generation::Five, _) => {
                Err(SpriteError::NoTable)
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
    let (table, ids) = gen1_palette_ids(rom, count)?;
    let colors = gen1_colors(game, rom, bank_of(table))?;
    ids[1..]
        .iter()
        .map(|&id| {
            let palette = Palette::read(rom, colors + usize::from(id) * 8)?;
            Some([palette, palette])
        })
        .collect()
}

pub(crate) fn gen1_palette(game: Game, rom: &[u8], id: u8) -> Option<Palette> {
    let (table, _) = gen1_palette_ids(rom, GEN1_DEX)?;
    let colors = gen1_colors(game, rom, bank_of(table))?;
    Palette::read(rom, colors + usize::from(id) * 8)
}

fn gen1_palette_ids(rom: &[u8], count: usize) -> Option<(usize, &[u8])> {
    Pattern::new("5F 16 00 21 ?? ?? 19 7E C9")
        .find_all(rom)
        .find_map(|at| {
            let table = same_bank(rom, at, 4)?;
            let ids = rom.get(table..table + count + 1)?;
            let distinct = ids.iter().collect::<std::collections::HashSet<_>>().len();
            (ids.iter().all(|&id| id < 0x30) && distinct >= 8).then_some((table, ids))
        })
}

fn gen1_colors(game: Game, rom: &[u8], bank: usize) -> Option<usize> {
    gen1_cgb_colors(game, rom, bank).or_else(|| gen1_sgb_colors(rom, bank))
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

#[cfg(test)]
mod tests {
    use crate::fixtures::{game, put};
    use crate::test_support::{
        Planes, crystal_rom_with_pictures, encode_gen1, offset, red_rom, type_table, words,
    };
    use crate::{
        Caught, Gender, Language, Place, RomTables, Side, SpriteError, TimeOfDay, Version,
    };

    fn gen1_record(dex: u8, front: u16, back: u16) -> Vec<u8> {
        let mut record = vec![0u8; 28];
        record[0] = dex;
        record[1..6].copy_from_slice(&[40, 50, 60, 70, 80]);
        record[6] = 0x14;
        record[7] = 0x02;
        record[10] = 0x55;
        record[11..13].copy_from_slice(&front.to_le_bytes());
        record[13..15].copy_from_slice(&back.to_le_bytes());
        record[19] = 3;
        record
    }

    fn gen1_planes(bank: usize) -> (Planes, Planes) {
        (
            Planes::random(5, 5, bank as u32, 40),
            Planes::random(4, 4, 100 + bank as u32, 40),
        )
    }

    fn red_rom_with_pictures() -> Vec<u8> {
        let mut rom = red_rom();
        let bank_code = [
            0xFA, 0x1E, 0xD0, 0x47, 0xFE, 0x01, 0x3E, 0x01, 0x28, 0x16, 0x78, 0xFE, 0x1F, 0x3E,
            0x09, 0x38, 0x10, 0x78, 0xFE, 0x4A, 0x3E, 0x0A, 0x38, 0x0A, 0x78, 0xFE, 0x74, 0x3E,
            0x0B, 0x38, 0x04, 0x3E, 0x0C, 0xC3, 0x00, 0x15,
        ];
        put(&mut rom, 0x2000, &bank_code);
        let mut mew_code = vec![0x21];
        mew_code.extend(0x7800u16.to_le_bytes());
        mew_code.extend([
            0x11, 0x00, 0xD0, 0x01, 0x1C, 0x00, 0x3E, 0x01, 0xCD, 0x00, 0x00,
        ]);
        put(&mut rom, 0x2100, &mew_code);
        put(&mut rom, 0x7800, &gen1_record(151, 0x6000, 0x6800));
        let table: Vec<u8> = (1..=150)
            .flat_map(|dex| gen1_record(dex, 0x6000, 0x6800))
            .collect();
        put(&mut rom, 0x38000, &table);
        for bank in [1, 0x0A, 0x0B, 0x0C] {
            let (front, back) = gen1_planes(bank);
            put(&mut rom, offset(bank, 0x6000), &encode_gen1(&front, 2, 1));
            put(&mut rom, offset(bank, 0x6800), &encode_gen1(&back, 1, 0));
        }
        let palette_bank = 0x1D;
        put(
            &mut rom,
            offset(palette_bank, 0x4000),
            &[0x5F, 0x16, 0x00, 0x21, 0x00, 0x50, 0x19, 0x7E, 0xC9],
        );
        put(
            &mut rom,
            offset(palette_bank, 0x4010),
            &[0x11, 0x00, 0x51, 0x21, 0x00, 0x52, 0xCD, 0x00, 0x00],
        );
        let ids: Vec<u8> = (0..152).map(|dex| (dex % 10) as u8).collect();
        put(&mut rom, offset(palette_bank, 0x5000), &ids);
        put(&mut rom, offset(palette_bank, 0x5100), &[0x59]);
        let colors: Vec<u16> = (0..10u16)
            .flat_map(|id| [0x7FFF, 0x100 + id, 0x200 + id, 0])
            .collect();
        put(&mut rom, offset(palette_bank, 0x5200), &words(&colors));
        type_table(&mut rom, offset(palette_bank, 0x4020), 0x6000, 0x1B);
        rom
    }

    #[test]
    fn gen1_pictures_come_from_the_bank_the_game_picks() {
        let rom = red_rom_with_pictures();
        let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
        for (dex, bank) in [
            (1, 0x0C),
            (40, 0x0C),
            (80, 0x0B),
            (117, 0x0B),
            (118, 0x0A),
            (150, 0x0A),
            (151, 1),
        ] {
            let species = tables.species_for_dex(dex).expect("species");
            let (front, back) = gen1_planes(bank);
            let sprite = tables
                .sprite(&rom, species, None, Side::Front)
                .unwrap_or_else(|e| panic!("dex {dex}: {e}"));
            assert_eq!((sprite.width, sprite.height), (5, 5));
            for y in 0..40 {
                for x in 0..40 {
                    assert_eq!(sprite.pixel(x, y), front.shade(x, y), "dex {dex}");
                }
            }
            let sprite = tables
                .sprite(&rom, species, None, Side::Back)
                .expect("back");
            assert_eq!((sprite.width, sprite.height), (4, 4));
            assert!((0..32).all(|y| (0..32).all(|x| sprite.pixel(x, y) == back.shade(x, y))));
        }
        assert_eq!(
            tables.sprite(&rom, 2, None, Side::Front),
            Err(SpriteError::NoSpecies)
        );
    }

    #[test]
    fn gen1_palettes_types_and_base_stats_come_from_the_rom() {
        let rom = red_rom_with_pictures();
        let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
        let mew = tables.species_for_dex(151).expect("mew");
        let first = tables.species_for_dex(1).expect("first");
        assert_eq!(tables.base_stats(mew).map(|b| b.dex), Some(151));
        let base = tables.base_stats(first).expect("base");
        assert_eq!(
            (base.hp, base.special_attack, base.special_defense),
            (40, 80, 80)
        );
        assert_eq!(base.gender_ratio, None);
        let palette = tables.palette(first, false).expect("palette");
        assert_eq!(palette.colors, [0x7FFF, 0x101, 0x201, 0]);
        assert_eq!(tables.palette(mew, true).expect("mew").colors[1], 0x101);
        assert_eq!(
            tables
                .palette(tables.species_for_dex(27).unwrap(), false)
                .unwrap()
                .colors[2],
            0x207
        );
        assert_eq!(tables.type_name(0x14), Some("TYPEAU"));
        assert_eq!(tables.species_types(first), vec!["TYPEAU", "TYPEAC"]);
        assert_eq!(tables.type_name(0x1B), None);
    }

    fn uniform(sprite: &crate::Sprite, byte: u8) -> bool {
        (0..sprite.height_px()).all(|y| {
            (0..sprite.width_px()).all(|x| {
                let bit = (byte >> (7 - x % 8)) & 1;
                sprite.pixel(x, y) == bit * 3
            })
        })
    }

    #[test]
    fn gen2_pictures_follow_the_bank_fix_and_the_unown_letter() {
        let rom = crystal_rom_with_pictures();
        let tables =
            RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
        for dex in [1u8, 2, 3, 150, 251] {
            let n = [5, 6, 7][usize::from(dex) % 3];
            let front = tables
                .sprite(&rom, u16::from(dex), None, Side::Front)
                .unwrap_or_else(|e| panic!("{dex}: {e}"));
            assert_eq!((front.width, front.height), (n, n));
            assert!(uniform(&front, dex), "{dex}");
            let back = tables
                .sprite(&rom, u16::from(dex), None, Side::Back)
                .expect("back");
            assert_eq!((back.width, back.height), (6, 6));
            assert!(uniform(&back, !dex), "{dex}");
        }
        for (letter, value) in [('A', 0x80), ('C', 0x82), ('Z', 0x99)] {
            let sprite = tables
                .sprite(&rom, 201, Some(letter), Side::Front)
                .expect("unown");
            assert!(uniform(&sprite, value), "{letter}");
            let back = tables
                .sprite(&rom, 201, Some(letter), Side::Back)
                .expect("unown back");
            assert!(uniform(&back, value - 0x40), "{letter}");
        }
        let plain = tables.sprite(&rom, 201, None, Side::Front).expect("unown");
        assert!(uniform(&plain, 0x80));
        let ignored = tables
            .sprite(&rom, 200, Some('C'), Side::Front)
            .expect("200");
        assert!(uniform(&ignored, 200));
        assert_eq!(
            tables.sprite(&rom, 0, None, Side::Front),
            Err(SpriteError::NoSpecies)
        );
    }

    #[test]
    fn gen2_palettes_names_and_formulas_come_from_the_rom() {
        let rom = crystal_rom_with_pictures();
        let tables =
            RomTables::load(game(Version::Crystal, Language::English), &rom).expect("tables");
        let normal = tables.palette(25, false).expect("normal");
        let shiny = tables.palette(25, true).expect("shiny");
        assert_eq!(normal.colors, [0x7FFF, 0x1019, 0x2019, 0]);
        assert_eq!(shiny.colors, [0x7FFF, 0x3019, 0x4019, 0]);
        assert_eq!(tables.type_name(0x14), Some("TYPEAU"));
        assert_eq!(tables.species_types(25), vec!["TYPEAU"]);
        let caught = |location| Caught {
            time_of_day: TimeOfDay::Day,
            level: 5,
            ot_gender: Gender::Female,
            location,
        };
        assert_eq!(
            tables.caught_place(&caught(1)),
            Some(Place::Landmark("PLACEAB"))
        );
        assert_eq!(
            tables.caught_place(&caught(3)),
            Some(Place::Landmark("NEW TOWN"))
        );
        assert_eq!(tables.caught_place(&caught(0x7E)), Some(Place::Gift));
        assert_eq!(tables.caught_place(&caught(0x7F)), Some(Place::Event));
        assert_eq!(tables.caught_place(&caught(0x70)), None);
        assert_eq!(
            tables.base_stats(25).map(|b| b.gender_ratio),
            Some(Some(0x00))
        );
        assert_eq!(tables.base_stats(25).map(|b| b.egg_cycles), Some(Some(20)));
    }

    #[test]
    fn tables_missing_from_the_rom_leave_sprites_unavailable() {
        let rom = red_rom();
        let tables = RomTables::load(game(Version::Red, Language::English), &rom).expect("tables");
        assert_eq!(
            tables.sprite(&rom, 190, None, Side::Front),
            Err(SpriteError::NoTable)
        );
        assert_eq!(tables.palette(190, false), None);
        assert_eq!(tables.base_stats(190), None);
        assert_eq!(tables.type_name(0), None);
    }

    #[test]
    fn base_stats_parse_from_a_single_record() {
        use crate::{BaseStats, Generation, GrowthRate};
        let record = gen1_record(151, 0x5500, 0x6600);
        let mew = BaseStats::from_record(Generation::One, &record).expect("gen 1 record");
        assert_eq!((mew.dex, mew.hp, mew.special_defense), (151, 40, 80));
        assert_eq!(mew.growth_rate, GrowthRate::MediumSlow);
        assert_eq!(mew.egg_cycles, None);
        assert!(BaseStats::from_record(Generation::One, &record[..27]).is_none());

        let mut record = [0u8; 32];
        record[0] = 172;
        record[15] = 10;
        record[22] = 0;
        let pichu = BaseStats::from_record(Generation::Two, &record).expect("gen 2 record");
        assert_eq!((pichu.dex, pichu.egg_cycles), (172, Some(10)));
        record[22] = 9;
        assert!(BaseStats::from_record(Generation::Two, &record).is_none());
    }
}
