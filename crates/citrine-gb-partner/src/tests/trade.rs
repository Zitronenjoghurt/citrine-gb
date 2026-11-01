use super::side::{Party, Side, game_block, read_party};
use super::{EGG_CYCLES, PSYCHIC, game, partner};
use crate::{Happening, Partner, Stage, event};
use citrine_gb_gamedata::{Language, Version};

const GEN1: u8 = 0x60;
const GEN2: u8 = 0x70;
const TRADE_CENTER: u8 = 0xD4;
const ROOM_TRADE_CENTER: u8 = 0xD1;
const PRESSED_B: u8 = 0xD8;

struct Sizes {
    name: usize,
    party: usize,
    patched: usize,
    mail: Option<usize>,
}

const GEN1_EN: Sizes = Sizes {
    name: 11,
    party: 424,
    patched: 264,
    mail: None,
};
const GEN1_JP: Sizes = Sizes {
    name: 6,
    party: 359,
    patched: 264,
    mail: None,
};
const GEN2_EN: Sizes = Sizes {
    name: 11,
    party: 450,
    patched: 290,
    mail: Some(390),
};
const GEN2_JP: Sizes = Sizes {
    name: 6,
    party: 385,
    patched: 290,
    mail: Some(330),
};

struct Exchanged {
    party: Party,
    mail: Option<Vec<u8>>,
    sent_party: Vec<u8>,
}

fn blocks(side: &mut Side, sizes: &Sizes) -> Exchanged {
    side.byte(0x00);
    side.byte(0x00);
    let rn = side.exchange_bytes(&game_block(17, 7, 0x10));
    assert!(rn[7..].iter().all(|&b| b < 0xFD));
    let sent_party = game_block(sizes.party, 6, 0x22);
    let party = side.exchange_bytes(&sent_party);
    let patches = side.exchange_bytes(&game_block(200, 3, 0));
    let mail = sizes
        .mail
        .map(|len| side.exchange_mail(&game_block(len, 0, 0x20)));
    Exchanged {
        party: read_party(&party, &patches, sizes.name, sizes.patched),
        mail,
        sent_party,
    }
}

fn assert_offer(partner: &Partner, party: &Party, id_prefix: bool) {
    let offered = partner.offered();
    assert_eq!(party.count, 1);
    assert_eq!(&party.species[..2], &[offered.species, 0xFF]);
    let mon = if id_prefix {
        assert_eq!(&party.patched[..2], &offered.ot_id.to_be_bytes());
        &party.patched[2..2 + offered.mon.len()]
    } else {
        &party.patched[..offered.mon.len()]
    };
    assert_eq!(mon, &offered.mon[..]);
    let name = offered.ot_name.len();
    assert_eq!(party.name, offered.ot_name);
    assert_eq!(&party.ots[..name], &offered.ot_name[..]);
    assert_eq!(&party.nicknames[..name], &offered.nickname[..]);
}

fn trade(side: &mut Side, high: u8, slot: u8) {
    assert_eq!(side.nybble(high, slot), 0);
    assert_eq!(side.nybble(high, 2), 2);
}

#[test]
fn red_receives_the_event_mew_and_can_trade_again() {
    let red = game(Version::Red, Language::English);
    let mut partner = partner(red, event("mew-training-tour-1999").unwrap(), 7);
    let first = partner.offered().clone();
    let mut side = Side { peer: &mut partner };
    side.handshake();
    assert_eq!(side.sync_bytes(0xD0), TRADE_CENTER);
    assert_eq!(side.nybble(GEN1, 0), 0);
    let exchanged = blocks(&mut side, &GEN1_EN);
    assert_offer(&partner, &exchanged.party, false);
    assert_eq!(first.species, 0x15);
    assert_eq!(first.mon[0], 0x15);
    assert_eq!(&first.mon[5..7], &[PSYCHIC, PSYCHIC]);
    assert_eq!(first.mon[7], 45);
    assert_eq!(partner.received()[1], exchanged.sent_party);
    assert_eq!(partner.stage(), Stage::Choosing);

    let mut side = Side { peer: &mut partner };
    trade(&mut side, GEN1, 3);
    assert_eq!(partner.traded(), 1);
    assert_eq!(partner.stage(), Stage::Linked);
    let happened = partner.take_happenings();
    assert!(happened.contains(&Happening::Traded { slot: 3 }));
    assert!(happened.contains(&Happening::Exchanged));

    let mut side = Side { peer: &mut partner };
    assert_eq!(side.nybble(GEN1, 2), 0);
    assert_eq!(side.nybble(GEN1, 0), 0);
    let again = blocks(&mut side, &GEN1_EN);
    trade(&mut side, GEN1, 0);
    assert_eq!(partner.traded(), 2);
    assert_eq!(again.party.species[0], 0x15);
}

#[test]
fn a_game_that_picks_cancel_at_the_link_menu_wins_and_can_link_again() {
    let red = game(Version::Red, Language::English);
    let mut partner = partner(red, event("mew-uk-tour-2000").unwrap(), 1);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    assert_eq!(side.sync_bytes(PRESSED_B), TRADE_CENTER);
    side.handshake();
    assert_eq!(side.sync_bytes(0xD0), TRADE_CENTER);
    assert_eq!(side.nybble(GEN1, 0), 0);
    let exchanged = blocks(&mut side, &GEN1_EN);
    trade(&mut side, GEN1, 0);
    assert_eq!(exchanged.party.count, 1);
    assert_eq!(partner.traded(), 1);
}

#[test]
fn cancelling_the_selection_returns_to_the_room() {
    let red = game(Version::Blue, Language::English);
    let mut partner = partner(red, event("mew-stadium-tour-2000").unwrap(), 2);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    side.sync_bytes(0xD0);
    side.nybble(GEN1, 0);
    blocks(&mut side, &GEN1_EN);
    assert_eq!(side.nybble(GEN1, 0x0F), 0x0F);
    assert_eq!(partner.stage(), Stage::Linked);
    assert!(partner.take_happenings().contains(&Happening::Canceled));

    let mut side = Side { peer: &mut partner };
    assert_eq!(side.nybble(GEN1, 0), 0);
    blocks(&mut side, &GEN1_EN);
    trade(&mut side, GEN1, 1);
    assert_eq!(partner.traded(), 1);
}

#[test]
fn declining_at_the_confirm_goes_back_to_the_selection() {
    let yellow = game(Version::Yellow, Language::English);
    let mut partner = partner(yellow, event("bulbasaur-stadium").unwrap(), 3);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    side.sync_bytes(0xD0);
    side.nybble(GEN1, 0);
    let exchanged = blocks(&mut side, &GEN1_EN);
    assert_eq!(side.nybble(GEN1, 2), 0);
    assert_eq!(side.nybble(GEN1, 1), 1);
    assert_eq!(partner.stage(), Stage::Choosing);
    assert_eq!(partner.traded(), 0);
    assert_eq!(partner.offered().mon[7], 168);
    assert_eq!(exchanged.party.patched[7], 168);

    let mut side = Side { peer: &mut partner };
    trade(&mut side, GEN1, 2);
    assert_eq!(partner.traded(), 1);
}

#[test]
fn japanese_green_uses_short_names() {
    let green = game(Version::Green, Language::Japanese);
    let mut partner = partner(green, event("mew-corocoro-1996").unwrap(), 4);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    side.sync_bytes(0xD0);
    side.nybble(GEN1, 0);
    let exchanged = blocks(&mut side, &GEN1_JP);
    assert_offer(&partner, &exchanged.party, false);
    assert_eq!(
        partner.offered().ot_name,
        vec![0x89, 0xA8, 0x89, 0xA8, 0x50, 0x50]
    );
    assert!((1..=20).contains(&partner.offered().ot_id));
}

fn gen2_reception(side: &mut Side) {
    side.handshake();
    assert_eq!(side.nybble(GEN1, 1), 1);
    assert_eq!(side.sync_bytes(ROOM_TRADE_CENTER), ROOM_TRADE_CENTER);
    assert_eq!(side.nybble(GEN2, 5), 5);
}

#[test]
fn silver_receives_celebi_with_mail_and_the_check_byte() {
    let silver = game(Version::Silver, Language::English);
    let mut partner = partner(silver, event("celebi-pcny").unwrap(), 5);
    let mut side = Side { peer: &mut partner };
    gen2_reception(&mut side);
    let exchanged = blocks(&mut side, &GEN2_EN);
    assert_offer(&partner, &exchanged.party, true);
    let mail = exchanged.mail.expect("mail");
    assert_eq!(&mail[..5], &[0x20; 5]);
    assert_eq!(mail[5 + 33 * 6 + 14 * 6], 0xFF);
    let offered = partner.offered().clone();
    assert_eq!(offered.species, 251);
    assert_eq!(&offered.mon[2..6], &[73, 93, 215, 105]);
    assert!(offered.ot_name.starts_with(&[0x8F, 0x82, 0x8D, 0x98]));
    assert!((32..=1053).contains(&offered.ot_id));
    assert_eq!(offered.mon[27], 70);

    let mut side = Side { peer: &mut partner };
    trade(&mut side, GEN2, 0);
    assert_eq!(side.nybble(GEN2, 1), 1);
    assert_eq!(partner.traded(), 1);
    assert_eq!(partner.stage(), Stage::Linked);
}

#[test]
fn the_colosseum_receptionist_sees_another_room() {
    let crystal = game(Version::Crystal, Language::English);
    let mut partner = partner(crystal, event("celebi-pcny").unwrap(), 6);
    let mut side = Side { peer: &mut partner };
    side.handshake();
    assert_eq!(side.nybble(GEN1, 1), 1);
    assert_eq!(side.sync_bytes(0xD2), ROOM_TRADE_CENTER);
    assert_eq!(partner.stage(), Stage::Linked);
}

#[test]
fn gen2_cancels_at_selection_and_confirm() {
    let gold = game(Version::Gold, Language::German);
    let mut partner = partner(gold, event("gligar-stadium-2").unwrap(), 8);
    let mut side = Side { peer: &mut partner };
    gen2_reception(&mut side);
    blocks(&mut side, &GEN2_EN);
    assert_eq!(side.nybble(GEN2, 0x0F), 0x0F);
    assert_eq!(side.nybble(GEN2, 5), 5);
    blocks(&mut side, &GEN2_EN);
    assert_eq!(side.nybble(GEN2, 4), 0);
    assert_eq!(side.nybble(GEN2, 1), 1);
    trade(&mut side, GEN2, 4);
    assert_eq!(partner.traded(), 1);
    let offered = partner.offered();
    assert_eq!(offered.ot_id, 2001);
    assert_eq!(offered.mon[1], 0x96);
}

#[test]
fn a_gen1_event_reaches_silver_through_the_time_capsule() {
    let silver = game(Version::Silver, Language::English);
    let mut partner = partner(silver, event("mew-poke-tour-1999").unwrap(), 9);
    assert!(partner.through_time_capsule());
    let mut side = Side { peer: &mut partner };
    side.handshake();
    assert_eq!(side.nybble(GEN1, 1), 0);
    assert_eq!(side.nybble(GEN1, 5), 0);
    let exchanged = blocks(&mut side, &GEN1_EN);
    assert_offer(&partner, &exchanged.party, false);
    assert_eq!(partner.offered().species, 0x15);
    assert_eq!(partner.offered().mon.len(), 44);
    let mut side = Side { peer: &mut partner };
    trade(&mut side, GEN1, 0);
    assert_eq!(partner.traded(), 1);
}

#[test]
fn eggs_hatch_from_the_species_list_with_no_hp() {
    let silver = game(Version::Silver, Language::English);
    let mut partner = partner(silver, event("pichu-egg-pcny").unwrap(), 10);
    let mut side = Side { peer: &mut partner };
    gen2_reception(&mut side);
    let exchanged = blocks(&mut side, &GEN2_EN);
    assert_offer(&partner, &exchanged.party, true);
    let offered = partner.offered();
    assert_eq!(offered.species, 0xFD);
    assert_eq!(offered.mon[0], 172);
    assert_eq!(offered.mon[27], EGG_CYCLES);
    assert_eq!(offered.mon[31], 5);
    assert_eq!(&offered.mon[34..36], &[0, 0]);
    assert_ne!(&offered.mon[36..38], &[0, 0]);
    assert_eq!(&offered.nickname[..4], &[0x84, 0x86, 0x86, 0x50]);
}

#[test]
fn japanese_gold_gets_short_names_and_short_mail() {
    let gold = game(Version::Gold, Language::Japanese);
    let mut partner = partner(gold, event("celebi-hobby-fair-2001").unwrap(), 11);
    let mut side = Side { peer: &mut partner };
    gen2_reception(&mut side);
    let exchanged = blocks(&mut side, &GEN2_JP);
    assert_offer(&partner, &exchanged.party, true);
    assert_eq!(exchanged.mail.map(|m| m.len()), Some(330));
}
