use super::{Live, Pokedex, Ram, Spot, Storage, Trainer, badges};
use crate::pkm::{Gen, Language};
use crate::tables::Game;

const PARTY: u32 = 0x18E00;
const PLAYER: u32 = 0x19400;
const HEAP_WINDOW: usize = 0x4_0000;
const TAG_TO_BLOCK: u32 = 0x18;
const GAME_DATA_TAG: &[u8] = b"game_data.c\0";
const PLAY_TIME_TAG: &[u8] = b"playtime.c\0";
const GAME_DATA_SPOT: u32 = 0x114;
const NAME_UNITS: usize = 8;
const DEX_FLAGS: usize = 0x54;
const BADGES: usize = 8;
const WHITE_SHIFT: u32 = 0x20;
const BOXES: usize = 24;

struct Layout {
    misc: u32,
    pokedex: u32,
}

fn layout(game: Game) -> Layout {
    match game {
        Game::Black | Game::White => Layout {
            misc: 0x21200,
            pokedex: 0x21600,
        },
        _ => Layout {
            misc: 0x21100,
            pokedex: 0x21400,
        },
    }
}

fn first_slot(game: Game, language: Language) -> Option<u32> {
    use Language::*;
    let black = match (game, language) {
        (Game::Black | Game::White, English) => 0x0223_49B4,
        (Game::Black | Game::White, German) => 0x0223_48F4,
        (Game::Black | Game::White, French) => 0x0223_4934,
        (Game::Black | Game::White, Italian) => 0x0223_48B4,
        (Game::Black | Game::White, Japanese) => 0x0223_4814,
        (Game::Black | Game::White, Korean) => 0x0223_50B4,
        (Game::Black | Game::White, Spanish) => 0x0223_4974,
        (Game::Black2 | Game::White2, English) => 0x0221_E3EC,
        (Game::Black2 | Game::White2, German) => 0x0221_E32C,
        (Game::Black2 | Game::White2, French) => 0x0221_E40C,
        (Game::Black2 | Game::White2, Italian) => 0x0221_E2EC,
        (Game::Black2 | Game::White2, Japanese) => 0x0221_DD8C,
        (Game::Black2 | Game::White2, Korean) => 0x0221_EB2C,
        (Game::Black2 | Game::White2, Spanish) => 0x0221_E3AC,
        _ => return None,
    };
    let shift = match (game, language) {
        (Game::White, _) => WHITE_SHIFT,
        (Game::White2, English | Italian) => 2 * WHITE_SHIFT,
        (Game::White2, _) => WHITE_SHIFT,
        _ => 0,
    };
    Some(black + shift)
}

fn heap_block(ram: Ram, first_slot: u32, tag: &[u8]) -> Option<u32> {
    Some(ram.find(first_slot, HEAP_WINDOW, tag)? + TAG_TO_BLOCK)
}

fn play_time(ram: Ram, first_slot: u32) -> Option<(u16, u8, u8)> {
    let clock = heap_block(ram, first_slot, PLAY_TIME_TAG)?;
    let time = (ram.u16(clock)?, ram.u8(clock + 2)?, ram.u8(clock + 3)?);
    (time.1 < 60 && time.2 < 60).then_some(time)
}

fn spot(ram: Ram, game: Game, first_slot: u32) -> Option<Spot> {
    if !matches!(game, Game::Black | Game::White) {
        return None;
    }
    let spot = heap_block(ram, first_slot, GAME_DATA_TAG)? + GAME_DATA_SPOT;
    Some(Spot {
        map: ram.u16(spot)?,
        x: i32::from(ram.u16(spot + 0x06)?),
        z: i32::from(ram.u16(spot + 0x0E)?),
    })
}

pub(super) fn read(ram: Ram, game: Game, language: Language) -> Option<Live> {
    let first = first_slot(game, language)?;
    let base = first - 8 - PARTY;
    let layout = layout(game);
    let player = base + PLAYER;
    let misc = base + layout.misc;
    let saved_time = (
        ram.u16(player + 0x24)?,
        ram.u8(player + 0x26)?,
        ram.u8(player + 0x27)?,
    );
    let (hours, minutes, seconds) = play_time(ram, first).unwrap_or(saved_time);
    let trainer = Trainer {
        name: ram.text(Gen::Five, player + 0x04, NAME_UNITS)?,
        tid: ram.u16(player + 0x14)?,
        sid: ram.u16(player + 0x16)?,
        female: ram.u8(player + 0x21)? == 1,
        money: ram.u32(misc)?,
        badges: badges(u16::from(ram.u8(misc + 0x04)?), BADGES),
        hours,
        minutes,
        seconds,
    };
    let party = ram.party(Gen::Five, base + PARTY + 4, base + PARTY + 8)?;
    let dex = base + layout.pokedex;
    let pokedex = Pokedex {
        caught: ram.flags(dex + 0x08, DEX_FLAGS, Pokedex::size(game))?,
        seen: ram.flags(
            dex + 0x08 + DEX_FLAGS as u32,
            DEX_FLAGS,
            Pokedex::size(game),
        )?,
    };
    let spot = spot(ram, game, first);
    let storage = Storage {
        count: BOXES,
        current: base,
        names: base + 0x04,
        first: base + 0x400,
        stride: 0x1000,
    };
    let (boxes, current_box) = ram.boxes(Gen::Five, storage).unwrap_or_default();
    Some(Live {
        game,
        language,
        trainer,
        party,
        pokedex,
        spot,
        boxes,
        current_box,
    })
}

#[cfg(test)]
mod tests {
    use super::super::Spot;
    use super::super::fixture::{RamImage, mon};
    use super::super::read;
    use super::{GAME_DATA_TAG, PARTY, PLAY_TIME_TAG, PLAYER, TAG_TO_BLOCK};
    use crate::pkm::{Gen, Language};
    use crate::tables::Game;

    const BASE: u32 = 0x0221_E32C - 8 - PARTY;

    #[test]
    fn black_2_reads_trainer_and_party() {
        let mut ram = RamImage::new(b"IREO");
        ram.0[0x3F_FE0F] = b'D';
        let player = BASE + PLAYER;
        ram.put_text(player + 0x04, Gen::Five, "Nate");
        ram.put_u16(player + 0x14, 4242);
        ram.put_u16(player + 0x24, 3);
        ram.put(player + 0x26, &[4, 5]);
        ram.put_u32(BASE + 0x21100, 1500);
        ram.put(BASE + 0x21104, &[0b1]);
        ram.put_party(
            BASE + PARTY + 4,
            BASE + PARTY + 8,
            &[mon(Gen::Five, 495, 5)],
        );
        ram.put(BASE + 0x21400 + 0x08, &[0b1]);
        let live = read(&ram.0).expect("live");
        assert_eq!(live.game, Game::Black2);
        assert_eq!(live.language, Language::German);
        assert_eq!(live.trainer.name, "Nate");
        assert_eq!(live.trainer.tid, 4242);
        assert_eq!(live.trainer.money, 1500);
        assert_eq!(live.trainer.badges.iter().filter(|b| **b).count(), 1);
        assert_eq!(
            (
                live.trainer.hours,
                live.trainer.minutes,
                live.trainer.seconds
            ),
            (3, 4, 5)
        );
        assert_eq!(live.party[0].species, 495);
        assert_eq!(live.pokedex.caught, vec![1]);
        assert_eq!(live.spot, None);
    }

    #[test]
    fn black_2_boxes_sit_at_the_start_of_the_save() {
        let mut ram = RamImage::new(b"IREO");
        ram.0[0x3F_FE0F] = b'D';
        ram.put_party(
            BASE + PARTY + 4,
            BASE + PARTY + 8,
            &[mon(Gen::Five, 495, 5)],
        );
        ram.put(BASE, &[23]);
        for number in 0..24 {
            ram.put_text(
                BASE + 4 + number * 0x28,
                Gen::Five,
                &format!("Box {}", number + 1),
            );
        }
        ram.put(
            BASE + 0x400 + 23 * 0x1000,
            &mon(Gen::Five, 501, 8).encode_box(),
        );
        let live = read(&ram.0).expect("live");
        assert_eq!(live.boxes.len(), 24);
        assert_eq!(live.current_box, 23);
        assert_eq!(live.boxes[23].name, "Box 24");
        assert_eq!(live.boxes[23].pokemon[0].species, 501);
        assert_eq!(live.boxes[23].pokemon[0].generation(), Gen::Five);
    }

    #[test]
    fn black_reads_spot_and_play_time_from_their_heap_blocks() {
        const FIRST: u32 = 0x0223_48F4;
        let mut ram = RamImage::new(b"IRBO");
        ram.0[0x3F_FE0F] = b'D';
        let base = FIRST - 8 - PARTY;
        ram.put_u16(base + PLAYER + 0x24, 1);
        ram.put_party(
            base + PARTY + 4,
            base + PARTY + 8,
            &[mon(Gen::Five, 498, 6)],
        );
        let game_data = FIRST + 0x1_AE2C;
        ram.put(game_data, GAME_DATA_TAG);
        ram.put_u16(game_data + TAG_TO_BLOCK + 0x114, 391);
        ram.put(game_data + TAG_TO_BLOCK + 0x11A, &[4]);
        ram.put(game_data + TAG_TO_BLOCK + 0x122, &[6]);
        let clock = FIRST + 0x2_2608;
        ram.put(clock, PLAY_TIME_TAG);
        ram.put(clock + TAG_TO_BLOCK, &[0, 0, 9, 35]);
        let live = read(&ram.0).expect("live");
        assert_eq!(live.game, Game::Black);
        assert_eq!(
            live.spot,
            Some(Spot {
                map: 391,
                x: 4,
                z: 6
            })
        );
        assert_eq!(
            (
                live.trainer.hours,
                live.trainer.minutes,
                live.trainer.seconds
            ),
            (0, 9, 35)
        );
    }
}
