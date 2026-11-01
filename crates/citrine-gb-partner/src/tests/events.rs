use super::{dex, game, pp};
use crate::{EVENTS, Partner, events_for};
use citrine_gb_gamedata::{Generation, Language, Version, encode_name};
use std::collections::HashSet;

const GAMES: [(Version, Language); 14] = [
    (Version::Red, Language::English),
    (Version::Blue, Language::French),
    (Version::Red, Language::German),
    (Version::Blue, Language::Italian),
    (Version::Red, Language::Spanish),
    (Version::Green, Language::Japanese),
    (Version::Yellow, Language::English),
    (Version::Silver, Language::English),
    (Version::Gold, Language::French),
    (Version::Silver, Language::German),
    (Version::Gold, Language::Italian),
    (Version::Crystal, Language::Spanish),
    (Version::Crystal, Language::Japanese),
    (Version::Gold, Language::Korean),
];

#[test]
fn every_event_has_a_key_a_source_and_sane_facts() {
    let mut keys = HashSet::new();
    for event in EVENTS {
        assert!(keys.insert(event.key), "{} twice", event.key);
        assert!(event.source.starts_with("https://"), "{}", event.key);
        assert!(!event.trainers.is_empty(), "{}", event.key);
        assert!((1..=4).contains(&event.moves.len()), "{}", event.key);
        let (species, moves) = match event.generation {
            Generation::One => (151, 165),
            Generation::Two => (251, 251),
        };
        assert!((1..=species).contains(&event.dex), "{}", event.key);
        assert!(
            event.moves.iter().all(|&m| (1..=moves).contains(&m)),
            "{}",
            event.key
        );
        assert!((1..=100).contains(&event.level), "{}", event.key);
        assert!(!event.egg || event.generation == Generation::Two);
    }
}

#[test]
fn every_event_builds_for_every_game_it_fits() {
    let mut offered = 0;
    for (version, language) in GAMES {
        let game = game(version, language);
        for event in events_for(game) {
            let partner = Partner::with_dex(game, dex(game), pp(), event, 1)
                .unwrap_or_else(|e| panic!("{} on {version:?} {language:?}: {e}", event.key));
            assert_eq!(partner.offered().ot_name.len(), game.name_length());
            offered += 1;
        }
    }
    assert!(offered > EVENTS.len());
}

#[test]
fn every_trainer_name_has_glyphs_in_its_games() {
    for event in EVENTS {
        for trainer in event.trainers {
            for &language in trainer.languages {
                let versions: &[Version] = match event.generation {
                    Generation::One => &[Version::Red, Version::Silver],
                    Generation::Two => &[Version::Silver],
                };
                for &version in versions {
                    let game = game(version, language);
                    assert!(
                        encode_name(game, trainer.name, game.name_length()).is_some(),
                        "{} for {version:?} {language:?}",
                        trainer.name
                    );
                }
            }
        }
    }
}

#[test]
fn games_only_see_events_they_can_receive() {
    let red = game(Version::Red, Language::English);
    assert!(events_for(red).all(|e| e.generation == Generation::One && !e.egg));
    assert!(events_for(red).any(|e| e.key == "mew-training-tour-1999"));

    let silver = game(Version::Silver, Language::English);
    assert!(events_for(silver).any(|e| e.generation == Generation::One));
    assert!(events_for(silver).any(|e| e.key == "celebi-pcny"));
    assert!(events_for(silver).all(|e| e.trainers_for(Language::English).next().is_some()));

    let crystal = game(Version::Crystal, Language::Japanese);
    assert!(events_for(crystal).all(|e| e.trainers_for(Language::Japanese).next().is_some()));
    assert!(!events_for(crystal).any(|e| e.key == "celebi-pcny"));

    let korean = game(Version::Gold, Language::Korean);
    assert_eq!(events_for(korean).count(), 0);
}

#[test]
fn a_gen2_event_is_refused_for_a_gen1_game() {
    let red = game(Version::Red, Language::English);
    let celebi = crate::event("celebi-pcny").unwrap();
    assert!(!celebi.fits(red));
    assert_eq!(
        Partner::with_dex(red, dex(red), pp(), celebi, 1).err(),
        Some(crate::OfferError::Generation)
    );
    let german = game(Version::Red, Language::German);
    let training = crate::event("mew-training-tour-1999").unwrap();
    assert_eq!(
        Partner::with_dex(german, dex(german), pp(), training, 1).err(),
        Some(crate::OfferError::Language)
    );
}
