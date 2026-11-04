use super::rom::identify;
use super::text::{self, TERMINATOR};
use super::{u16_at, u32_at};
use crate::game::{Game, Version};
use crate::region_map::{Marker, RegionMap};

const ROM_BASE: u32 = 0x0800_0000;
const ROM_END: u32 = 0x0A00_0000;
const HEADER: usize = 0x1C;
const HOENN_TILES: usize = 233;
const HOENN_TILEMAP: usize = 64;
const HOENN_COLORS: usize = 32;
const HOENN_FIRST_COLOR: u8 = 0x70;
const KANTO_TILEMAP: usize = 30 * 20;
const KANTO_MAPS: usize = 4;
const KANTO_EDGE: usize = 2;
const KANTO_LEFT: usize = 3;
const KANTO_COLUMNS: usize = 24;
const KANTO_ROWS: usize = 18;
const KANTO_FIRST_SECTION: u8 = 0x58;
const KANTO_SECTIONS: usize = 109;
const KANTO_COLORS: usize = 80;
const KANTO_PALETTES_BEFORE_GFX: usize = 32 + 32 + 8 + KANTO_COLORS * 2;
const SEVII_FIRST_SECTION: u8 = 0x8F;
const SEVII_ROW: usize = 30;
const SECTION_NONE_KANTO: u8 = 0xC5;
const DYNAMIC_HOENN: u8 = 0x57;
const DYNAMIC_KANTO: u8 = 0xC4;
const SCREEN_COLUMNS: usize = 30;
const SCREEN_ROWS: usize = 20;
const NAME_MAX: usize = 24;
const NAME_END: u8 = 0xFC;
const MIN_GROUPS: usize = 30;
const MAX_GROUP_MAPS: usize = 0x100;
const MAP_TYPE_UNDERGROUND: u8 = 4;
const MAP_TYPE_UNKNOWN: u8 = 7;
const MAP_TYPE_INDOOR: u8 = 8;
const MAP_TYPE_SECRET_BASE: u8 = 9;
const KANTO_OVERRIDES: [(u8, u16, u16); 27] = [
    (0x88, 12, 12),
    (0x86, 14, 6),
    (0x87, 4, 14),
    (0x8C, 18, 6),
    (0x8E, 18, 4),
    (0x80, 14, 9),
    (0x89, 2, 3),
    (0x85, 11, 6),
    (0x81, 14, 7),
    (0x82, 12, 6),
    (0xBB, 18, 13),
    (0xAE, 10, 8),
    (0xB3, 5, 6),
    (0xAF, 2, 3),
    (0xB0, 14, 12),
    (0xB6, 17, 3),
    (0xB2, 17, 11),
    (0xBF, 9, 12),
    (0xBD, 9, 12),
    (0xBC, 9, 12),
    (0xC1, 9, 12),
    (0xC0, 9, 12),
    (0xB8, 9, 12),
    (0xC2, 9, 12),
    (0xBE, 9, 12),
    (0xB4, 16, 8),
    (0x7E, 4, 6),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Warp {
    pub group: u8,
    pub map: u8,
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Whereabouts {
    pub x: i16,
    pub y: i16,
    pub location: Warp,
    pub dynamic_warp: Warp,
    pub escape_warp: Warp,
}

impl Whereabouts {
    pub(crate) fn read(block: &[u8]) -> Self {
        let warp = |at: usize| Warp {
            group: block[at],
            map: block[at + 1],
            x: u16_at(block, at + 4) as i16,
            y: u16_at(block, at + 6) as i16,
        };
        Self {
            x: u16_at(block, 0) as i16,
            y: u16_at(block, 2) as i16,
            location: warp(0x04),
            dynamic_warp: warp(0x14),
            escape_warp: warp(0x24),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    section: u8,
    map_type: u8,
    escaping: bool,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    name: String,
    picture: usize,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionMaps {
    game: Game,
    pictures: Vec<RegionMap>,
    sections: Vec<Option<Section>>,
    groups: Vec<Vec<Header>>,
}

impl RegionMaps {
    pub fn load(rom: &[u8]) -> Option<Self> {
        let game = identify(rom)?;
        let groups = find_groups(rom)?;
        let (pictures, sections) = match game.version {
            Version::FireRed | Version::LeafGreen => {
                (kanto_pictures(rom)?, kanto_sections(rom, game)?)
            }
            _ => (vec![hoenn_picture(rom)?], hoenn_sections(rom, game)?),
        };
        Some(Self {
            game,
            pictures,
            sections,
            groups,
        })
    }

    pub fn pictures(&self) -> &[RegionMap] {
        &self.pictures
    }

    pub fn maps_in_groups(&self) -> Vec<usize> {
        self.groups.iter().map(Vec::len).collect()
    }

    pub fn section_name(&self, section: u8) -> Option<&str> {
        self.section(section).map(|s| s.name.as_str())
    }

    fn kanto(&self) -> bool {
        matches!(self.game.version, Version::FireRed | Version::LeafGreen)
    }

    fn section(&self, section: u8) -> Option<&Section> {
        self.sections.get(usize::from(section))?.as_ref()
    }

    fn header(&self, warp: Warp) -> Option<Header> {
        self.groups
            .get(usize::from(warp.group))?
            .get(usize::from(warp.map))
            .copied()
    }

    pub fn marker(&self, at: &Whereabouts) -> Option<Marker<'_>> {
        let here = self.header(at.location)?;
        let dynamic = if self.kanto() {
            DYNAMIC_KANTO
        } else {
            DYNAMIC_HOENN
        };
        let from_warp = |warp: Warp, own: Option<u8>| -> Option<(u8, Header, i16, i16)> {
            let header = self.header(warp)?;
            Some((own.unwrap_or(header.section), header, warp.x, warp.y))
        };
        let (section, size, x, y) = match here.map_type {
            MAP_TYPE_UNDERGROUND | MAP_TYPE_UNKNOWN
                if self.game.version == Version::Emerald && !here.escaping =>
            {
                let one = Header {
                    width: 1,
                    height: 1,
                    ..here
                };
                (here.section, one, 1, 1)
            }
            MAP_TYPE_UNDERGROUND | MAP_TYPE_UNKNOWN => from_warp(at.escape_warp, None)?,
            MAP_TYPE_SECRET_BASE => from_warp(at.dynamic_warp, None)?,
            MAP_TYPE_INDOOR if here.section == dynamic => from_warp(at.dynamic_warp, None)?,
            MAP_TYPE_INDOOR => from_warp(at.escape_warp, Some(here.section))?,
            _ => (here.section, here, at.x, at.y),
        };
        let overridden = KANTO_OVERRIDES
            .iter()
            .find(|(own, _, _)| self.kanto() && *own == here.section);
        let (section, cell) = match overridden {
            Some(&(own, x, y)) => (self.section(own)?, (x, y)),
            None => {
                let entry = self.section(section)?;
                if entry.width == 0 || entry.height == 0 || (entry.x, entry.y) == (0, 0) {
                    return None;
                }
                let step = |size: u32, span: u16, at: i16| -> u16 {
                    let scale = (size / u32::from(span)).max(1);
                    ((at.max(0) as u32 / scale) as u16).min(span - 1)
                };
                (
                    entry,
                    (
                        entry.x + step(size.width, entry.width, x),
                        entry.y + step(size.height, entry.height, y),
                    ),
                )
            }
        };
        let (x, y) = if self.kanto() {
            (8 * cell.0 + 12, 8 * cell.1 + 20)
        } else {
            (8 * (cell.0 + 1) + 4, 8 * (cell.1 + 2) + 4)
        };
        let map = self.pictures.get(section.picture)?;
        (usize::from(x) < map.width && usize::from(y) < map.height).then_some(Marker {
            map,
            name: &section.name,
            x,
            y,
        })
    }
}

pub(super) fn rom_pointer(rom: &[u8], at: usize) -> Option<usize> {
    let value = u32_at(rom.get(at..at + 4)?, 0);
    let offset = (ROM_BASE..ROM_END)
        .contains(&value)
        .then(|| (value - ROM_BASE) as usize)?;
    (offset < rom.len()).then_some(offset)
}

fn read_name(rom: &[u8], at: usize, game: Game) -> Option<String> {
    let bytes = rom.get(at..(at + NAME_MAX).min(rom.len()))?;
    let end = bytes.iter().position(|&b| b == TERMINATOR)?;
    let mut name = Vec::with_capacity(end + 1);
    let mut rest = &bytes[..=end];
    while let Some((&b, tail)) = rest.split_first() {
        match tail.split_first() {
            Some((&0, after)) if b == NAME_END => rest = after,
            _ => {
                name.push(b);
                rest = tail;
            }
        }
    }
    (name.len() > 1).then(|| text::decode(&name, game.language))
}

fn read_header(rom: &[u8], at: usize) -> Option<Header> {
    let record = rom.get(at..at + HEADER)?;
    let layout = rom_pointer(rom, at)?;
    rom_pointer(rom, at + 4)?;
    rom_pointer(rom, at + 8)?;
    let connections = u32_at(record, 0x0C);
    if connections != 0 {
        rom_pointer(rom, at + 0x0C)?;
    }
    let map_type = record[0x17];
    let width = u32_at(rom.get(layout..layout + 8)?, 0);
    let height = u32_at(rom.get(layout..layout + 8)?, 4);
    let plausible = map_type <= MAP_TYPE_SECRET_BASE
        && (1..=0x400).contains(&width)
        && (1..=0x400).contains(&height);
    plausible.then_some(Header {
        section: record[0x14],
        map_type,
        escaping: record[0x1A] & 2 != 0,
        width,
        height,
    })
}

fn find_groups(rom: &[u8]) -> Option<Vec<Vec<Header>>> {
    (0..rom.len().saturating_sub(8))
        .step_by(4)
        .find_map(|table| {
            let first = rom_pointer(rom, table)?;
            let second = rom_pointer(rom, table + 4)?;
            if second <= first || (second - first) % 4 != 0 || second - first > 4 * MAX_GROUP_MAPS {
                return None;
            }
            read_header(rom, rom_pointer(rom, first)?)?;
            let mut starts = vec![first];
            while let Some(next) = rom_pointer(rom, table + 4 * starts.len()) {
                let last = *starts.last()?;
                if next <= last || next > table || (next - last) % 4 != 0 {
                    break;
                }
                starts.push(next);
            }
            if starts.len() < MIN_GROUPS {
                return None;
            }
            starts
                .iter()
                .enumerate()
                .map(|(i, &start)| {
                    let end = starts.get(i + 1).copied().unwrap_or(table);
                    (start..end)
                        .step_by(4)
                        .map(|at| read_header(rom, rom_pointer(rom, at)?))
                        .collect::<Option<Vec<Header>>>()
                })
                .collect()
        })
}

pub(crate) fn decompress_lz77(rom: &[u8], at: usize) -> Option<(Vec<u8>, usize)> {
    let header = rom.get(at..at + 4)?;
    if header[0] != 0x10 {
        return None;
    }
    let size = usize::from(header[1]) | usize::from(header[2]) << 8 | usize::from(header[3]) << 16;
    if size == 0 || size > 0x40000 {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    let mut read = at + 4;
    while out.len() < size {
        let flags = *rom.get(read)?;
        read += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags >> bit & 1 == 0 {
                out.push(*rom.get(read)?);
                read += 1;
            } else {
                let pair = rom.get(read..read + 2)?;
                read += 2;
                let length = usize::from(pair[0] >> 4) + 3;
                let distance = (usize::from(pair[0] & 0x0F) << 8 | usize::from(pair[1])) + 1;
                let start = out.len().checked_sub(distance)?;
                for i in 0..length {
                    let byte = out[start + i];
                    out.push(byte);
                }
            }
        }
    }
    out.truncate(size);
    Some((out, read))
}

fn lz_header(size: usize) -> [u8; 4] {
    [0x10, size as u8, (size >> 8) as u8, (size >> 16) as u8]
}

fn aligned(at: usize) -> usize {
    at.next_multiple_of(4)
}

fn find_lz(rom: &[u8], size: usize) -> impl Iterator<Item = usize> + '_ {
    let header = lz_header(size);
    (0..rom.len().saturating_sub(4))
        .step_by(4)
        .filter(move |&at| rom[at..at + 4] == header)
}

fn bgr555(color: u16) -> [u8; 4] {
    let channel = |shift: u16| {
        let value = ((color >> shift) & 0x1F) as u8;
        (value << 3) | (value >> 2)
    };
    [channel(0), channel(5), channel(10), 0xFF]
}

fn colors(rom: &[u8], at: usize, count: usize) -> Option<Vec<[u8; 4]>> {
    let bytes = rom.get(at..at + count * 2)?;
    Some(
        (0..count)
            .map(|i| bgr555(u16_at(bytes, 2 * i) & 0x7FFF))
            .collect(),
    )
}

fn hoenn_picture(rom: &[u8]) -> Option<RegionMap> {
    let tile_bytes = HOENN_TILES * 64;
    find_lz(rom, tile_bytes).find_map(|at| {
        let (tiles, end) = decompress_lz77(rom, at)?;
        let (tilemap, _) = decompress_lz77(rom, aligned(end))?;
        if tilemap.len() != HOENN_TILEMAP * HOENN_TILEMAP {
            return None;
        }
        let palette = colors(rom, at.checked_sub(HOENN_COLORS * 2)?, HOENN_COLORS)?;
        let mut map = RegionMap::blank(SCREEN_COLUMNS, SCREEN_ROWS);
        for row in 0..SCREEN_ROWS {
            for column in 0..SCREEN_COLUMNS {
                let tile = usize::from(tilemap[row * HOENN_TILEMAP + column]);
                let pixels = tiles.get(tile * 64..tile * 64 + 64)?;
                map.put_tile(column, row, (false, false), |x, y| {
                    let index = pixels[y * 8 + x].checked_sub(HOENN_FIRST_COLOR)?;
                    palette.get(usize::from(index)).copied()
                });
            }
        }
        Some(map)
    })
}

fn hoenn_sections(rom: &[u8], game: Game) -> Option<Vec<Option<Section>>> {
    let entry = |at: usize| -> Option<Option<Section>> {
        let record = rom.get(at..at + 8)?;
        if record.iter().all(|&b| b == 0) {
            return Some(None);
        }
        let (x, y, width, height) = (record[0], record[1], record[2], record[3]);
        if x > 28 || y > 15 || width > 28 || height > 15 {
            return None;
        }
        let pointer = rom_pointer(rom, at + 4)?;
        if rom.get(pointer) == Some(&TERMINATOR) {
            return Some(None);
        }
        let name = read_name(rom, pointer, game)?;
        Some(Some(Section {
            name,
            picture: 0,
            x: u16::from(x),
            y: u16::from(y),
            width: u16::from(width),
            height: u16::from(height),
        }))
    };
    (0..rom.len().saturating_sub(8))
        .step_by(4)
        .filter(|&at| rom[at + 2] == 1 && rom[at + 3] == 1 && rom[at + 7] == 0x08)
        .filter_map(|at| {
            let mut sections = Vec::new();
            while let Some(section) = entry(at + 8 * sections.len()) {
                sections.push(section);
                if sections.len() > 0x100 {
                    break;
                }
            }
            while sections.last().is_some_and(Option::is_none) {
                sections.pop();
            }
            let named = sections.iter().flatten().count();
            (named >= usize::from(DYNAMIC_HOENN) - 4 && sections.first()?.is_some())
                .then_some(sections)
        })
        .max_by_key(Vec::len)
}

fn kanto_pictures(rom: &[u8]) -> Option<Vec<RegionMap>> {
    let size = KANTO_TILEMAP * 2;
    find_lz(rom, size)
        .filter(|&at| {
            let mut next = at;
            (0..KANTO_MAPS).all(|_| {
                let Some((data, end)) = decompress_lz77(rom, next) else {
                    return false;
                };
                next = aligned(end);
                data.len() == size
            })
        })
        .find_map(|kanto| kanto_pictures_at(rom, kanto))
}

fn kanto_pictures_at(rom: &[u8], kanto: usize) -> Option<Vec<RegionMap>> {
    let back = |target: usize, hops: usize| -> Option<usize> {
        (target.saturating_sub(0x10000)..target)
            .step_by(4)
            .rev()
            .find(|&start| {
                let mut at = start;
                for _ in 0..hops {
                    match decompress_lz77(rom, at) {
                        Some((_, end)) => at = aligned(end),
                        None => return false,
                    }
                    if at > target {
                        return false;
                    }
                }
                at == target
            })
    };
    let graphics = back(kanto, 3)?;
    let first_lz = back(graphics, 5)?;
    let palette = colors(
        rom,
        first_lz.checked_sub(KANTO_PALETTES_BEFORE_GFX)?,
        KANTO_COLORS,
    )?;
    let (tiles, _) = decompress_lz77(rom, graphics)?;
    let mut at = kanto;
    let mut pictures = Vec::new();
    for _ in 0..KANTO_MAPS {
        let (tilemap, end) = decompress_lz77(rom, at)?;
        at = aligned(end);
        let mut map = RegionMap::blank(KANTO_COLUMNS, KANTO_ROWS);
        for row in KANTO_EDGE..KANTO_EDGE + KANTO_ROWS {
            for column in KANTO_LEFT..KANTO_LEFT + KANTO_COLUMNS {
                let entry = u16_at(&tilemap, 2 * (row * SCREEN_COLUMNS + column));
                let tile = usize::from(entry & 0x3FF);
                let flip = (entry & 0x400 != 0, entry & 0x800 != 0);
                let bank = usize::from(entry >> 12) * 16;
                let Some(pixels) = tiles.get(tile * 32..tile * 32 + 32) else {
                    continue;
                };
                map.put_tile(column - KANTO_LEFT, row - KANTO_EDGE, flip, |x, y| {
                    let pair = pixels[y * 4 + x / 2];
                    let index = if x % 2 == 0 { pair & 0x0F } else { pair >> 4 };
                    if index == 0 {
                        return None;
                    }
                    palette.get(bank + usize::from(index)).copied()
                });
            }
        }
        pictures.push(map);
    }
    Some(pictures)
}

fn kanto_sections(rom: &[u8], game: Game) -> Option<Vec<Option<Section>>> {
    let names = (0..rom.len().saturating_sub(4 * KANTO_SECTIONS))
        .step_by(4)
        .find(|&at| {
            (0..KANTO_SECTIONS).all(|i| {
                rom_pointer(rom, at + 4 * i)
                    .is_some_and(|name| read_name(rom, name, game).is_some())
            }) && (0..KANTO_SECTIONS)
                .all(|i| rom.get(at + 4 * KANTO_SECTIONS + 4 * i + 1) == Some(&0))
                && rom
                    .get(at + 4 * KANTO_SECTIONS..at + 4 * KANTO_SECTIONS + 4)
                    .is_some_and(|corner| corner.iter().any(|&b| b != 0))
        })?;
    let corners = names + 4 * KANTO_SECTIONS;
    let sizes = (corners + 4 * KANTO_SECTIONS..corners + 0x1000)
        .step_by(4)
        .find(|&at| {
            rom.get(at..at + 4)
                .is_some_and(|w| w.iter().any(|&b| b != 0))
        })?;
    let sevii = find_sevii(rom)?;
    let mut sections = vec![None; usize::from(KANTO_FIRST_SECTION)];
    for i in 0..KANTO_SECTIONS {
        let id = KANTO_FIRST_SECTION + i as u8;
        let picture = sevii
            .iter()
            .position(|row| row.contains(&id))
            .map_or(0, |row| row + 1);
        let pair = |at: usize| (u16_at(rom, at + 4 * i), u16_at(rom, at + 4 * i + 2));
        let (x, y) = pair(corners);
        let (width, height) = pair(sizes);
        sections.push(Some(Section {
            name: read_name(rom, rom_pointer(rom, names + 4 * i)?, game)?,
            picture,
            x,
            y,
            width,
            height,
        }));
    }
    Some(sections)
}

fn find_sevii(rom: &[u8]) -> Option<Vec<Vec<u8>>> {
    let start = [
        SEVII_FIRST_SECTION,
        SEVII_FIRST_SECTION + 1,
        SEVII_FIRST_SECTION + 2,
    ];
    (0..rom.len().saturating_sub(3 * SEVII_ROW)).find_map(|at| {
        if rom[at..at + 3] != start {
            return None;
        }
        let rows: Vec<Vec<u8>> = (0..3)
            .map(|row| {
                let bytes = &rom[at + row * SEVII_ROW..at + (row + 1) * SEVII_ROW];
                let end = bytes.iter().position(|&b| b == SECTION_NONE_KANTO)?;
                let ids = &bytes[..end];
                ids.iter()
                    .all(|&id| (SEVII_FIRST_SECTION..SECTION_NONE_KANTO).contains(&id))
                    .then(|| ids.to_vec())
                    .filter(|ids| !ids.is_empty())
            })
            .collect::<Option<_>>()?;
        Some(rows)
    })
}
