use super::{Live, Pokedex, Ram, Spot, Storage, Trainer, badges};
use crate::pkm::{Gen, Language};
use crate::tables::Game;

const NAME_UNITS: usize = 8;
const DEX_FLAGS: usize = 0x40;
const JOHTO_BADGES: usize = 8;
const BOXES: usize = 18;
const STEPS_WALKED: u32 = 0x00;
const STEPS_BIKED: u32 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    DiamondPearl,
    Platinum,
    HeartGoldSoulSilver,
}

struct Layout {
    header: u32,
    name: u32,
    party_count: u32,
    pokedex: u32,
    spot: u32,
    storage: u32,
    records: Option<u32>,
}

impl Family {
    fn of(game: Game) -> Option<Self> {
        match game {
            Game::Diamond | Game::Pearl => Some(Self::DiamondPearl),
            Game::Platinum => Some(Self::Platinum),
            Game::HeartGold | Game::SoulSilver => Some(Self::HeartGoldSoulSilver),
            _ => None,
        }
    }

    fn layout(self) -> Layout {
        match self {
            Self::DiamondPearl => Layout {
                header: 0x214,
                name: 0x64,
                party_count: 0x94,
                pokedex: 0x12DC,
                spot: 0x1238,
                storage: 0xC100,
                records: None,
            },
            Self::Platinum => Layout {
                header: 0x14,
                name: 0x68,
                party_count: 0x9C,
                pokedex: 0x1328,
                spot: 0x1280,
                storage: 0xCF2C,
                records: None,
            },
            Self::HeartGoldSoulSilver => Layout {
                header: 0x10,
                name: 0x64,
                party_count: 0x94,
                pokedex: 0x12B8,
                spot: 0x1234,
                storage: 0xF700,
                records: Some(0x4B3C),
            },
        }
    }

    fn storage(self, start: u32) -> Storage {
        match self {
            Self::DiamondPearl | Self::Platinum => Storage {
                count: BOXES,
                current: start,
                names: start + 0x1_1EE4,
                first: start + 4,
                stride: 0xFF0,
            },
            Self::HeartGoldSoulSilver => Storage {
                count: BOXES,
                current: start + 0x1_2000,
                names: start + 0x1_2008,
                first: start,
                stride: 0x1000,
            },
        }
    }

    fn save_pointer(self, game: Game, language: Language) -> Option<u32> {
        use Language::*;
        Some(match (self, language) {
            (Self::DiamondPearl, German) => 0x021C_5B08,
            (Self::DiamondPearl, English) => 0x021C_59C8,
            (Self::DiamondPearl, French) => 0x021C_5B48,
            (Self::DiamondPearl, Italian) => 0x021C_5AA8,
            (Self::DiamondPearl, Japanese) => 0x021C_7234,
            (Self::DiamondPearl, Korean) => 0x021C_2FC8,
            (Self::DiamondPearl, Spanish) => 0x021C_5B68,
            (Self::Platinum, German) => 0x021C_0934,
            (Self::Platinum, English) => 0x021C_0794,
            (Self::Platinum, French) => 0x021C_0974,
            (Self::Platinum, Italian) => 0x021C_08F4,
            (Self::Platinum, Japanese) => 0x021B_FB94,
            (Self::Platinum, Korean) => 0x021C_1694,
            (Self::Platinum, Spanish) => 0x021C_0994,
            (Self::HeartGoldSoulSilver, German) => 0x021D_2208,
            (Self::HeartGoldSoulSilver, English) => 0x021D_2228,
            (Self::HeartGoldSoulSilver, French) => 0x021D_2248,
            (Self::HeartGoldSoulSilver, Italian) => 0x021D_21C8,
            (Self::HeartGoldSoulSilver, Japanese) => 0x021D_1768,
            (Self::HeartGoldSoulSilver, Korean) => 0x021D_2C28,
            (Self::HeartGoldSoulSilver, Spanish) if game == Game::HeartGold => 0x021D_2248,
            (Self::HeartGoldSoulSilver, Spanish) => 0x021D_2268,
            (_, Other(_)) => return None,
        })
    }

    fn badge_bits(self, ram: Ram, name: u32) -> Option<(u16, usize)> {
        let johto = u16::from(ram.u8(name + 0x1A)?);
        Some(match self {
            Self::HeartGoldSoulSilver => (johto | u16::from(ram.u8(name + 0x1F)?) << 8, 16),
            Self::DiamondPearl | Self::Platinum => (johto, JOHTO_BADGES),
        })
    }
}

pub(super) fn steps(ram: Ram, game: Game, language: Language) -> Option<u32> {
    let family = Family::of(game)?;
    let layout = family.layout();
    let save = ram.u32(family.save_pointer(game, language)?)?;
    let records = save
        .checked_add(layout.header)?
        .checked_add(layout.records?)?;
    let walked = ram.u32(records + STEPS_WALKED)?;
    let biked = ram.u32(records + STEPS_BIKED)?;
    Some(walked.saturating_add(biked))
}

pub(super) fn read(ram: Ram, game: Game, language: Language) -> Option<Live> {
    let family = Family::of(game)?;
    let layout = family.layout();
    let save = ram.u32(family.save_pointer(game, language)?)?;
    let general = save.checked_add(layout.header)?;
    let name = general + layout.name;
    let (bits, count) = family.badge_bits(ram, name)?;
    let trainer = Trainer {
        name: ram.text(Gen::Four, name, NAME_UNITS)?,
        tid: ram.u16(name + 0x10)?,
        sid: ram.u16(name + 0x12)?,
        money: ram.u32(name + 0x14)?,
        female: ram.u8(name + 0x18)? == 1,
        badges: badges(bits, count),
        hours: ram.u16(name + 0x22)?,
        minutes: ram.u8(name + 0x24)?,
        seconds: ram.u8(name + 0x25)?,
    };
    let count = general + layout.party_count;
    let party = ram.party(Gen::Four, count, count + 4)?;
    let dex = general + layout.pokedex;
    let pokedex = Pokedex {
        caught: ram.flags(dex + 0x04, DEX_FLAGS, Pokedex::size(game))?,
        seen: ram.flags(dex + 0x44, DEX_FLAGS, Pokedex::size(game))?,
    };
    let spot = general + layout.spot;
    let spot = Some(Spot {
        map: ram.u32(spot)? as u16,
        x: ram.i32(spot + 0x08)?,
        z: ram.i32(spot + 0x0C)?,
    });
    let (boxes, current_box) = ram
        .boxes(Gen::Four, family.storage(general + layout.storage))
        .unwrap_or_default();
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
    use super::super::fixture::{RamImage, mon};
    use super::super::{read, steps};
    use crate::pkm::{Gen, Language};
    use crate::tables::Game;

    const SAVE: u32 = 0x0222_0000;

    fn soulsilver() -> RamImage {
        let mut ram = RamImage::new(b"IPGE");
        ram.put_u32(0x021D_2228, SAVE);
        let general = SAVE + 0x10;
        let name = general + 0x64;
        ram.put_text(name, Gen::Four, "LYRA");
        ram.put_u16(name + 0x10, 12345);
        ram.put_u16(name + 0x12, 54321);
        ram.put_u32(name + 0x14, 3000);
        ram.put(name + 0x18, &[1]);
        ram.put(name + 0x1A, &[0b101]);
        ram.put(name + 0x1F, &[0b1]);
        ram.put_u16(name + 0x22, 12);
        ram.put(name + 0x24, &[34, 56]);
        ram.put_party(
            general + 0x94,
            general + 0x98,
            &[mon(Gen::Four, 155, 5), mon(Gen::Four, 16, 3)],
        );
        ram.put(general + 0x12B8 + 0x04, &[0b11]);
        ram.put(general + 0x12B8 + 0x44, &[0b111]);
        ram.put_u32(general + 0x1234, 58);
        ram.put_u32(general + 0x1234 + 0x08, 0xFFFF_FFFE);
        ram.put_u32(general + 0x1234 + 0x0C, 700);
        ram
    }

    #[test]
    fn soulsilver_reads_trainer_party_dex_and_spot() {
        let live = read(&soulsilver().0).expect("live");
        assert_eq!(live.game, Game::SoulSilver);
        assert_eq!(live.language, Language::English);
        let trainer = &live.trainer;
        assert_eq!(trainer.name, "LYRA");
        assert_eq!((trainer.tid, trainer.sid), (12345, 54321));
        assert_eq!(trainer.money, 3000);
        assert!(trainer.female);
        assert_eq!(trainer.badges.len(), 16);
        assert_eq!(trainer.badges.iter().filter(|b| **b).count(), 3);
        assert!(trainer.badges[8]);
        assert_eq!(
            (trainer.hours, trainer.minutes, trainer.seconds),
            (12, 34, 56)
        );
        let species: Vec<u16> = live.party.iter().map(|m| m.species).collect();
        assert_eq!(species, vec![155, 16]);
        assert_eq!(live.pokedex.caught, vec![1, 2]);
        assert_eq!(live.pokedex.seen, vec![1, 2, 3]);
        let spot = live.spot.expect("spot");
        assert_eq!((spot.map, spot.x, spot.z), (58, -2, 700));
    }

    #[test]
    fn heartgold_boxes_are_padded_to_0x1000() {
        let mut ram = soulsilver();
        let storage = SAVE + 0x10 + 0xF700;
        ram.put(storage + 0x1_2000, &[1]);
        for number in 0..18 {
            ram.put_box_name(
                storage + 0x1_2008 + number * 40,
                &format!("BOX {}", number + 1),
            );
        }
        ram.put_box_name(storage + 0x1_2008 + 40, "FIRE");
        ram.put_boxed(storage, &mon(Gen::Four, 155, 5));
        ram.put_boxed(storage + 0x1000 + 136, &mon(Gen::Four, 158, 7));
        ram.put_boxed(storage + 0x1000 + 29 * 136, &mon(Gen::Four, 152, 9));
        ram.put(storage + 0x1000 + 2 * 136 + 0x10, &[0xAA; 4]);
        let live = read(&ram.0).expect("live");
        assert_eq!(live.boxes.len(), 18);
        assert_eq!(live.current_box, 1);
        assert_eq!(live.boxes[0].name, "BOX 1");
        assert_eq!(live.boxes[1].name, "FIRE");
        let species =
            |i: usize| -> Vec<u16> { live.boxes[i].pokemon.iter().map(|m| m.species).collect() };
        assert_eq!(species(0), vec![155]);
        assert_eq!(species(1), vec![158, 152]);
        assert!(live.boxes[2].pokemon.is_empty());
        assert!(live.boxes[0].pokemon[0].party.is_none());
    }

    #[test]
    fn platinum_boxes_follow_the_current_box_word() {
        const PT_SAVE: u32 = 0x0223_0000;
        let mut ram = RamImage::new(b"CPUE");
        ram.put_u32(0x021C_0794, PT_SAVE);
        let general = PT_SAVE + 0x14;
        ram.put_text(general + 0x68, Gen::Four, "DAWN");
        let storage = general + 0xCF2C;
        ram.put(storage, &[17]);
        for number in 0..18 {
            ram.put_box_name(
                storage + 0x1_1EE4 + number * 40,
                &format!("BOX {}", number + 1),
            );
        }
        ram.put_boxed(storage + 4 + 17 * 0xFF0 + 3 * 136, &mon(Gen::Four, 387, 5));
        let live = read(&ram.0).expect("live");
        assert_eq!(live.game, Game::Platinum);
        assert_eq!(live.current_box, 17);
        assert_eq!(live.boxes[17].name, "BOX 18");
        assert_eq!(live.boxes[17].pokemon[0].species, 387);
        assert_eq!(live.boxes.iter().map(|b| b.pokemon.len()).sum::<usize>(), 1);
    }

    #[test]
    fn diamond_shares_the_sinnoh_box_layout() {
        const DP_SAVE: u32 = 0x0224_0000;
        let mut ram = RamImage::new(b"ADAE");
        ram.put_u32(0x021C_59C8, DP_SAVE);
        let storage = DP_SAVE + 0x214 + 0xC100;
        ram.put_box_name(storage + 0x1_1EE4, "BOX 1");
        ram.put_boxed(storage + 4, &mon(Gen::Four, 393, 5));
        let live = read(&ram.0).expect("live");
        assert_eq!(live.game, Game::Diamond);
        assert_eq!(live.boxes[0].pokemon[0].species, 393);
        assert_eq!(live.boxes[0].name, "BOX 1");
    }

    #[test]
    fn a_wrong_current_box_reads_no_boxes() {
        let mut ram = soulsilver();
        ram.put(SAVE + 0x10 + 0xF700 + 0x1_2000, &[18]);
        let live = read(&ram.0).expect("live");
        assert!(live.boxes.is_empty());
        assert_eq!(live.party.len(), 2);
    }

    #[test]
    fn heartgold_steps_add_walked_and_biked_records() {
        let mut ram = soulsilver();
        let records = SAVE + 0x10 + 0x4B3C;
        ram.put_u32(records, 1_204);
        ram.put_u32(records + 4, 96);
        ram.put_u32(records + 8, 0xDEAD_BEEF);
        assert_eq!(steps(&ram.0), Some(1_300));
        ram.put_u32(records, 1_205);
        assert_eq!(steps(&ram.0), Some(1_301));
    }

    #[test]
    fn steps_read_nothing_without_a_save_or_outside_hgss() {
        let mut ram = soulsilver();
        ram.put_u32(0x021D_2228, 0);
        assert_eq!(steps(&ram.0), None);
        let mut platinum = RamImage::new(b"CPUE");
        platinum.put_u32(0x021C_0794, SAVE);
        assert_eq!(steps(&platinum.0), None);
    }

    #[test]
    fn a_broken_party_mon_reads_nothing() {
        let mut ram = soulsilver();
        ram.put(SAVE + 0x10 + 0x98 + 0x10, &[0xAA; 4]);
        assert_eq!(read(&ram.0), None);
    }

    #[test]
    fn a_party_count_over_six_reads_nothing() {
        let mut ram = soulsilver();
        ram.put(SAVE + 0x10 + 0x94, &[7]);
        assert_eq!(read(&ram.0), None);
    }

    #[test]
    fn a_null_save_pointer_reads_nothing() {
        let mut ram = soulsilver();
        ram.put_u32(0x021D_2228, 0);
        assert_eq!(read(&ram.0), None);
    }
}
