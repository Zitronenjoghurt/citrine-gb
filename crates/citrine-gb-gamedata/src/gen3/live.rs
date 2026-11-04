use super::flash::{Buffers, DATA};
use super::layout::{BOX_NAMES, NAME_LEN, Offsets};
use super::mon::{self, PARTY_SIZE};
use super::text::{self, TERMINATOR};
use super::{BOXES, Layout, PARTY_SLOTS, Pokemon, u16_at, u32_at};
use crate::ReadError;
use crate::game::Language;

const EWRAM: u32 = 0x0200_0000;
const SMALL: usize = DATA;
const LARGE: usize = DATA * 4;
const PC: usize = DATA * 9;
const BOX_NAME: usize = 9;
const WALLPAPERS: usize = BOX_NAMES + BOXES * BOX_NAME;
const LEVEL: usize = 0x54;
const HP: usize = 0x56;
const MAX_HP: usize = 0x58;
const SPACE: u8 = 0x00;
const TRAINER_ID: usize = 0x0A;
const RS_SMALL: usize = 0x2_4EA4;
const RS_PC: usize = 0x3_00A0;
const UNKNOWN: char = '\u{FFFD}';

#[derive(Debug, Clone, Copy)]
pub struct Memory<'a> {
    pub ewram: &'a [u8],
    pub iwram: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Blocks {
    small: usize,
    large: usize,
    pc: usize,
}

pub(crate) fn read(
    memory: Memory,
    layout: Layout,
    language: Language,
) -> Result<Buffers, ReadError> {
    let blocks = find_blocks(memory, layout, language).ok_or(ReadError::NotRunning)?;
    let ewram = memory.ewram;
    let mut buffers = Buffers {
        small: ewram[blocks.small..][..SMALL].to_vec(),
        large: ewram[blocks.large..][..LARGE].to_vec(),
        pc: ewram[blocks.pc..][..PC].to_vec(),
        index: 0,
    };
    let offsets = Offsets::of(layout);
    let ot = u32_at(&buffers.small, TRAINER_ID);
    if let Some((party, count)) = find_party(memory, &buffers.large, offsets, ot, blocks.large) {
        buffers.large[offsets.team_size] = count as u8;
        buffers.large[offsets.team..][..PARTY_SLOTS * PARTY_SIZE].copy_from_slice(&party);
    }
    Ok(buffers)
}

fn find_blocks(memory: Memory, layout: Layout, language: Language) -> Option<Blocks> {
    let ewram = memory.ewram;
    match layout {
        Layout::RubySapphire => {
            let gap = Offsets::of(layout).small_size;
            let blocks_at = |at: usize| {
                at + gap + LARGE <= ewram.len()
                    && trainer_like(&ewram[at..], language)
                    && team_like(&ewram[at + gap..], layout)
            };
            let small = std::iter::once(RS_SMALL)
                .chain((0..ewram.len().saturating_sub(gap + LARGE)).step_by(4))
                .find(|&at| blocks_at(at))?;
            let pc = std::iter::once(RS_PC)
                .chain((0..ewram.len().saturating_sub(PC)).step_by(4))
                .find(|&at| at + PC <= ewram.len() && storage_like(&ewram[at..]))?;
            Some(Blocks {
                small,
                large: small + gap,
                pc,
            })
        }
        Layout::Emerald | Layout::FireRedLeafGreen => {
            let inside = |word: u32, size: usize| -> Option<usize> {
                let at = usize::try_from(word.checked_sub(EWRAM)?).ok()?;
                (at + size <= ewram.len()).then_some(at)
            };
            memory
                .iwram
                .as_chunks::<4>()
                .0
                .windows(3)
                .find_map(|words| {
                    let [large, small, pc] = [0, 1, 2].map(|i| u32::from_le_bytes(words[i]));
                    let blocks = Blocks {
                        small: inside(small, SMALL)?,
                        large: inside(large, LARGE)?,
                        pc: inside(pc, PC)?,
                    };
                    (trainer_like(&ewram[blocks.small..], language)
                        && team_like(&ewram[blocks.large..], layout)
                        && storage_like(&ewram[blocks.pc..]))
                    .then_some(blocks)
                })
        }
    }
}

fn trainer_like(small: &[u8], language: Language) -> bool {
    let name = &small[..NAME_LEN];
    name[0] != TERMINATOR
        && name[0] != SPACE
        && name.contains(&TERMINATOR)
        && !text::decode(name, language).contains(UNKNOWN)
        && u32_at(small, TRAINER_ID) != 0
        && small[0x08] <= 1
        && small[0x10] < 60
        && small[0x11] < 60
        && small[0x12] < 60
}

fn team_like(large: &[u8], layout: Layout) -> bool {
    usize::from(large[Offsets::of(layout).team_size]) <= PARTY_SLOTS
}

fn storage_like(pc: &[u8]) -> bool {
    usize::from(pc[0]) < BOXES
        && pc[BOX_NAMES..WALLPAPERS]
            .chunks(BOX_NAME)
            .all(|name| name[0] != TERMINATOR && name.contains(&TERMINATOR))
        && pc[WALLPAPERS..WALLPAPERS + BOXES].iter().all(|&w| w < 16)
}

fn find_party(
    memory: Memory,
    large: &[u8],
    offsets: &Offsets,
    ot: u32,
    large_at: usize,
) -> Option<(Vec<u8>, usize)> {
    let saved = &large[offsets.team..][..PARTY_SLOTS * PARTY_SIZE];
    let saved: Vec<u32> = saved
        .chunks(PARTY_SIZE)
        .map(|slot| u32_at(slot, 0))
        .filter(|&p| p != 0)
        .collect();
    let skip = large_at + offsets.team..large_at + offsets.team + PARTY_SLOTS * PARTY_SIZE;
    let mut best: Option<((usize, usize), &[u8], usize)> = None;
    for (region, bytes) in [(0, memory.ewram), (1, memory.iwram)] {
        let mut at = 0;
        while at + PARTY_SLOTS * PARTY_SIZE <= bytes.len() {
            if (region == 0 && skip.contains(&at)) || !party_mon(&bytes[at..][..PARTY_SIZE]) {
                at += 4;
                continue;
            }
            let window = &bytes[at..][..PARTY_SLOTS * PARTY_SIZE];
            let slots: Vec<&[u8]> = window.chunks(PARTY_SIZE).collect();
            let count = slots.iter().take_while(|slot| party_mon(slot)).count();
            let rest_empty = slots[count..]
                .iter()
                .all(|slot| slot.iter().all(|&b| b == 0));
            if rest_empty {
                let shared = slots[..count]
                    .iter()
                    .filter(|slot| saved.contains(&u32_at(slot, 0)))
                    .count();
                let own = slots[..count]
                    .iter()
                    .filter(|slot| u32_at(slot, 4) == ot)
                    .count();
                if best.is_none_or(|(score, _, _)| (shared, own) > score) {
                    best = Some(((shared, own), window, count));
                }
            }
            at += count * PARTY_SIZE;
        }
    }
    best.filter(|((_, own), _, _)| *own > 0)
        .map(|(_, window, count)| (window.to_vec(), count))
}

fn party_mon(slot: &[u8]) -> bool {
    let level = slot[LEVEL];
    let max_hp = u16_at(slot, MAX_HP);
    (1..=100).contains(&level)
        && max_hp > 0
        && u16_at(slot, HP) <= max_hp
        && matches!(mon::read(slot), Some(Pokemon::Hatched(_) | Pokemon::Egg(_)))
}

#[cfg(test)]
mod tests {
    use crate::fixtures::gen3::TestMon3;
    use crate::fixtures::gen3_game::{
        EWRAM, GEN3_OT, IWRAM, gen3_running, gen3_sample_save, gen3_state, gen3_tables, hatched,
        put, torchic, treecko,
    };
    use crate::gen3::{Layout, Memory};
    use crate::{Pokemon, Ram, ReadError};

    #[test]
    fn a_running_game_reads_like_its_save_with_the_live_party() {
        for layout in [
            Layout::RubySapphire,
            Layout::Emerald,
            Layout::FireRedLeafGreen,
        ] {
            let save = gen3_sample_save(layout);
            let hurt = TestMon3 {
                hp: 12,
                ..treecko()
            };
            let (ewram, iwram) = gen3_running(layout, &save, &[hurt.party(), torchic().party()]);
            let live = gen3_tables(layout)
                .read_live(
                    Ram::Gba(Memory {
                        ewram: &ewram,
                        iwram: &iwram,
                    }),
                    &[],
                )
                .expect("live");
            let mut saved = gen3_state(layout, &save);
            assert_eq!(hatched(&live.party[0]).hp, Some(12), "{layout:?}");
            if let Pokemon::Hatched(lead) = &mut saved.party[0] {
                lead.hp = Some(12);
            }
            assert_eq!(live.party, saved.party, "{layout:?}");
            assert_eq!(live.boxes, saved.boxes, "{layout:?}");
            assert_eq!(live.trainer, saved.trainer, "{layout:?}");
            assert_eq!(live.journey, saved.journey, "{layout:?}");
        }
    }

    #[test]
    fn a_new_party_member_shows_before_the_game_saves() {
        let layout = Layout::Emerald;
        let save = gen3_sample_save(layout);
        let (ewram, iwram) = gen3_running(
            layout,
            &save,
            &[
                treecko().party(),
                torchic().party(),
                TestMon3::new(283, 0x0000_7777, GEN3_OT).party(),
            ],
        );
        let live = gen3_tables(layout)
            .read_live(
                Ram::Gba(Memory {
                    ewram: &ewram,
                    iwram: &iwram,
                }),
                &[],
            )
            .expect("live");
        assert_eq!(live.party.len(), 3);
        assert_eq!(hatched(&live.party[2]).species, 283);
    }

    #[test]
    fn memory_without_save_blocks_is_not_gen3_running() {
        for layout in [
            Layout::RubySapphire,
            Layout::Emerald,
            Layout::FireRedLeafGreen,
        ] {
            let (ewram, iwram) = (vec![0; EWRAM], vec![0; IWRAM]);
            let read = gen3_tables(layout).read_live(
                Ram::Gba(Memory {
                    ewram: &ewram,
                    iwram: &iwram,
                }),
                &[],
            );
            assert_eq!(read.err(), Some(ReadError::NotRunning), "{layout:?}");
            assert_eq!(
                gen3_tables(layout)
                    .read_live(Ram::Gb(&[0; 0x2000]), &[])
                    .err(),
                Some(ReadError::NotRunning)
            );
        }
    }

    #[test]
    fn ruby_skips_blocks_whose_name_is_not_text() {
        let layout = Layout::RubySapphire;
        let save = gen3_sample_save(layout);
        let (mut ewram, iwram) = gen3_running(layout, &save, &[treecko().party()]);
        put(&mut ewram, 0x2_0000, &[0, 0, 0, 0xFF, 0xA0, 0, 0x30, 1]);
        put(&mut ewram, 0x2_0100, &[0xBB, 0x0A, 0xFF, 0, 0, 0, 0, 0]);
        put(&mut ewram, 0x2_0200, &[0xF0, 0xFF, 0, 0, 0, 0, 0, 0]);
        let live = gen3_tables(layout)
            .read_live(
                Ram::Gba(Memory {
                    ewram: &ewram,
                    iwram: &iwram,
                }),
                &[],
            )
            .expect("live");
        assert_eq!(live.trainer.name, "BRENDAN");
        assert_eq!(live.trainer.id, 12345);
    }

    #[test]
    fn ruby_looks_at_its_fixed_addresses_first() {
        let layout = Layout::RubySapphire;
        let save = gen3_sample_save(layout);
        let mut decoy = save.clone();
        decoy.trainer("DECOY", false, (1, 2), (1, 2, 3));
        let mut ewram = vec![0; EWRAM];
        put(&mut ewram, 0x1_0000, &decoy.small);
        put(&mut ewram, 0x1_0890, &decoy.large);
        put(&mut ewram, 0x8, &decoy.pc);
        put(&mut ewram, 0x2_4EA4, &save.small);
        put(&mut ewram, 0x2_4EA4 + 0x890, &save.large);
        put(&mut ewram, 0x3_00A0, &save.pc);
        let iwram = vec![0; IWRAM];
        let live = gen3_tables(layout)
            .read_live(
                Ram::Gba(Memory {
                    ewram: &ewram,
                    iwram: &iwram,
                }),
                &[],
            )
            .expect("live");
        assert_eq!(live.trainer.name, "BRENDAN");
        assert_eq!(live.boxes, gen3_state(layout, &save).boxes);
    }
}
