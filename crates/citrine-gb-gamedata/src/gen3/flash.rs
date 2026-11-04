use super::layout::{GAME_CODE, Offsets};
use super::{Certainty, Guess, Layout, u16_at, u32_at};
use crate::ReadError;
use crate::state::SaveCopy;

pub(crate) const SECTION: usize = 0x1000;
pub(crate) const DATA: usize = 0xF80;
pub(crate) const SECTIONS: usize = 14;
pub(crate) const SLOT: usize = SECTION * SECTIONS;
pub(crate) const SIGNATURE: u32 = 0x0801_2025;
pub(crate) const ID: usize = 0xFF4;
pub(crate) const CHECKSUM: usize = 0xFF6;
pub(crate) const SIGNED: usize = 0xFF8;
pub(crate) const INDEX: usize = 0xFFC;
const PC_LAST: usize = 0x7D0;
const LAYOUTS: [Layout; 3] = [
    Layout::RubySapphire,
    Layout::Emerald,
    Layout::FireRedLeafGreen,
];

pub(crate) struct Buffers {
    pub small: Vec<u8>,
    pub large: Vec<u8>,
    pub pc: Vec<u8>,
    pub index: u32,
}

struct Slot<'a> {
    index: u32,
    sections: Vec<&'a [u8]>,
    whole: bool,
}

pub(crate) fn checksum(data: &[u8]) -> u16 {
    let sum = data.as_chunks::<4>().0.iter().fold(0u32, |sum, word| {
        sum.wrapping_add(u32::from_le_bytes(*word))
    });
    ((sum >> 16) as u16).wrapping_add(sum as u16)
}

pub(crate) fn data_size(layout: Layout, id: usize) -> usize {
    let offsets = Offsets::of(layout);
    match id {
        0 => offsets.small_size,
        4 => offsets.rival_size,
        13 => PC_LAST,
        _ => DATA,
    }
}

fn slot(flash: &[u8], start: usize) -> Option<Slot<'_>> {
    let signed: Vec<&[u8]> = flash[start..start + SLOT]
        .as_chunks::<SECTION>()
        .0
        .iter()
        .map(|section| section.as_slice())
        .filter(|section| u32_at(section, SIGNED) == SIGNATURE)
        .collect();
    let index = signed.iter().map(|s| u32_at(s, INDEX)).max()?;
    let mut sections: Vec<&[u8]> = signed
        .into_iter()
        .filter(|s| u32_at(s, INDEX) == index)
        .collect();
    sections.sort_by_key(|s| u16_at(s, ID));
    let whole = sections.len() == SECTIONS
        && sections
            .iter()
            .enumerate()
            .all(|(id, s)| usize::from(u16_at(s, ID)) == id);
    Some(Slot {
        index,
        sections,
        whole,
    })
}

fn valid(slot: &Slot, layout: Layout) -> bool {
    slot.whole
        && slot.sections.iter().enumerate().all(|(id, section)| {
            checksum(&section[..data_size(layout, id)]) == u16_at(section, CHECKSUM)
        })
}

fn newest_first(flash: &[u8]) -> Vec<Slot<'_>> {
    let mut slots: Vec<Slot> = [0, SLOT]
        .into_iter()
        .filter_map(|start| slot(flash, start))
        .collect();
    if let [a, b] = slots.as_slice()
        && (b.index.wrapping_sub(a.index) as i32) > 0
    {
        slots.swap(0, 1);
    }
    slots
}

fn concat(sections: &[&[u8]]) -> Vec<u8> {
    sections
        .iter()
        .flat_map(|s| s[..DATA].iter().copied())
        .collect()
}

pub(crate) fn read(flash: &[u8], layout: Layout) -> Result<(Buffers, SaveCopy), ReadError> {
    if flash.len() < 2 * SLOT {
        return Err(ReadError::TooShort);
    }
    let slots = newest_first(flash);
    let (n, slot) = slots
        .iter()
        .enumerate()
        .find(|(_, slot)| valid(slot, layout))
        .ok_or(ReadError::NoValidCopy)?;
    let buffers = Buffers {
        small: concat(&slot.sections[..1]),
        large: concat(&slot.sections[1..5]),
        pc: concat(&slot.sections[5..]),
        index: slot.index,
    };
    let copy = if n == 0 {
        SaveCopy::Main
    } else {
        SaveCopy::Backup
    };
    Ok((buffers, copy))
}

pub(crate) fn guess(flash: &[u8]) -> Option<Guess> {
    if flash.len() < 2 * SLOT {
        return None;
    }
    newest_first(flash).iter().find_map(|slot| {
        let passing: Vec<Layout> = LAYOUTS
            .into_iter()
            .filter(|&layout| valid(slot, layout))
            .collect();
        let by_code = match u32_at(slot.sections[0], GAME_CODE) {
            0 => Layout::RubySapphire,
            1 => Layout::FireRedLeafGreen,
            _ => Layout::Emerald,
        };
        if passing.contains(&by_code) {
            Some(Guess {
                layout: by_code,
                certainty: Certainty::Sure,
            })
        } else {
            passing.first().map(|&layout| Guess {
                layout,
                certainty: Certainty::Likely,
            })
        }
    })
}
