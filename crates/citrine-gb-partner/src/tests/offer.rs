use super::side::{Side, game_block, read_party};
use super::{custom, game, mew, partner};
use crate::events::{Genes, Trainer, TrainerId};
use crate::moves::pp_table;
use crate::{Dvs, Partner};
use citrine_gb_gamedata::{Language, Version};

const ID_WITH_NO_DATA: &[Trainer] = &[Trainer {
    languages: &[Language::English],
    name: "PATCH",
    id: TrainerId::Fixed(0x00FE),
}];

#[test]
fn no_data_bytes_in_the_party_are_patched_and_restored() {
    let red = game(Version::Red, Language::English);
    let event = custom(crate::Event {
        genes: Genes::Fixed(Dvs {
            attack: 15,
            defense: 14,
            speed: 15,
            special: 14,
        }),
        ..mew(ID_WITH_NO_DATA)
    });
    let mut partner = partner(red, event, 1);
    let offered = partner.offered().clone();
    assert_eq!(&offered.mon[12..14], &[0x00, 0xFE]);
    assert_eq!(&offered.mon[27..29], &[0xFE, 0xFE]);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    side.sync_bytes(0xD0);
    side.nybble(0x60, 0);
    side.exchange_bytes(&game_block(17, 7, 0));
    let party = side.exchange_bytes(&game_block(424, 6, 0));
    let patches = side.exchange_bytes(&game_block(200, 3, 0));
    assert!(!party.contains(&0xFE));
    assert_eq!(&patches[10..15], &[14, 28, 29, 0xFF, 0xFF]);
    let party = read_party(&party, &patches, 11, 264);
    assert_eq!(&party.patched[..44], &offered.mon[..]);
}

#[test]
fn the_offer_follows_the_event() {
    let spanish = game(Version::Blue, Language::Spanish);
    let partner = partner(spanish, crate::event("mew-madrid-2000").unwrap(), 1);
    let offered = partner.offered();
    assert_eq!(offered.ot_id, 60130);
    assert_eq!(
        offered.dvs,
        Dvs {
            attack: 6,
            defense: 4,
            speed: 3,
            special: 8
        }
    );
    assert_eq!(&offered.mon[8..12], &[1, 0, 0, 0]);
    assert_eq!(offered.mon[29], 35);
    assert_eq!(offered.mon[3], 5);
    assert_eq!(&offered.mon[27..29], &[0x64, 0x38]);
}

#[test]
fn pcny_legends_are_shiny() {
    let silver = game(Version::Silver, Language::English);
    for seed in 0..20 {
        let partner = partner(silver, crate::event("raikou-pcny").unwrap(), seed);
        let dvs = partner.offered().dvs;
        assert_eq!((dvs.defense, dvs.speed, dvs.special), (10, 10, 10));
        assert!([2, 3, 6, 7, 10, 11, 14, 15].contains(&dvs.attack));
        assert_eq!(partner.offered().mon[31], 40);
    }
}

#[test]
fn a_new_seed_gives_a_new_trainer_id() {
    let silver = game(Version::Silver, Language::English);
    let event = crate::event("celebi-fun-fest-2001").unwrap();
    let ids: std::collections::HashSet<u16> = (0..10)
        .map(|seed| partner(silver, event, seed).offered().ot_id)
        .collect();
    assert!(ids.len() > 5);
}

#[test]
fn the_offer_names_the_trainer_it_wrote() {
    let silver = game(Version::Silver, Language::English);
    let event = crate::event("celebi-pcny").unwrap();
    for seed in 0..10 {
        let offered = partner(silver, event, seed).offered().clone();
        assert!(event.trainers.iter().any(|t| t.name == offered.trainer));
        assert_eq!(
            Some(offered.ot_name),
            citrine_gb_gamedata::encode_name(silver, offered.trainer, 11)
        );
    }
}

#[test]
fn partners_refuse_roms_without_tables() {
    let red = game(Version::Red, Language::English);
    let event = crate::event("mew-training-tour-1999").unwrap();
    assert_eq!(
        Partner::new(red, &[0; 0x8000], event, 1).err(),
        Some(crate::OfferError::Rom)
    );
}

#[test]
fn the_move_table_is_found_by_its_first_records() {
    let mut rom = vec![0u8; 0x4000];
    for (record, generation) in [(6, Version::Red), (7, Version::Gold)] {
        let at = 0x1000;
        let count = if record == 6 { 165 } else { 251 };
        for i in 0..count {
            rom[at + i * record] = i as u8 + 1;
            rom[at + i * record + 5] = [35, 25, 10][i % 3];
        }
        let pp = pp_table(game(generation, Language::English), &rom).expect("a table");
        assert_eq!(pp.len(), count + 1);
        assert_eq!(&pp[..4], &[0, 35, 25, 10]);
        rom.fill(0);
    }
}
