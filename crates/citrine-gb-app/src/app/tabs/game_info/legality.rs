use crate::icons;
use citrine_gb_gamedata::{Generation, Legality, Mon, Reason, Tables};
use egui::{RichText, Ui};

const HGSS_FIRST_BALL: u8 = 17;
const HGSS_BALL_ITEMS: u16 = 475;

pub(super) fn verdict(tables: &Tables, mon: &Mon) -> Option<Legality> {
    (tables.generation() == Generation::Four)
        .then(|| mon.ds().map(|genes| genes.legality))
        .flatten()
}

pub(super) fn mark(ui: &mut Ui, tables: &Tables, mon: &Mon) {
    let Some(legality) = verdict(tables, mon) else {
        return;
    };
    let (icon, color) = match legality {
        Legality::Passes => (icons::SEAL_CHECK, ui.visuals().weak_text_color()),
        Legality::Breaks(_) => (icons::SEAL_WARNING, ui.visuals().warn_fg_color),
        Legality::NotChecked(_) => return,
    };
    ui.label(RichText::new(icon).color(color))
        .on_hover_text(sentence(tables, mon, legality));
}

pub(super) fn sentence(tables: &Tables, mon: &Mon, legality: Legality) -> String {
    match legality {
        Legality::Passes => "Passes every check".to_owned(),
        Legality::Breaks(reason) => format!("Breaks a rule: {}", broken(tables, mon, reason)),
        Legality::NotChecked(reason) => unchecked(tables, reason),
    }
}

fn broken(tables: &Tables, mon: &Mon, reason: Reason) -> String {
    let species = tables.species_name(mon.species).unwrap_or("this Pokémon");
    match reason {
        Reason::Experience => "its experience doesn't fit its level".to_owned(),
        Reason::Gender => "its gender doesn't fit its personality value".to_owned(),
        Reason::Ability => "its ability doesn't fit its personality value".to_owned(),
        Reason::Evs => "its effort values add up to more than 510".to_owned(),
        Reason::BallBytes => "its two Poké Ball records disagree".to_owned(),
        Reason::MetLocation => "its two records of where it was met disagree".to_owned(),
        Reason::EggLocation => "its records of where it hatched are wrong".to_owned(),
        Reason::Date => "its met or hatch date is impossible".to_owned(),
        Reason::Moves => "its moves are empty or repeat".to_owned(),
        Reason::Pp(slot) => match mon
            .moves
            .get(slot)
            .and_then(|slot| tables.move_name(slot.id))
        {
            Some(name) => format!("{name} has the wrong PP"),
            None => "a move has the wrong PP".to_owned(),
        },
        Reason::NicknameTooLong { max, .. } => format!("its nickname is longer than {max}"),
        Reason::TrainerNameTooLong { max, .. } => {
            format!("its trainer's name is longer than {max}")
        }
        Reason::TrainerNameEmpty => "its trainer's name is empty".to_owned(),
        Reason::Language => "its language record names no language".to_owned(),
        Reason::Species(_) => format!("{species} isn't in this game"),
        Reason::Form(_) => format!("this form of {species} isn't found here"),
        Reason::MetLevel(level) => format!("it can't be met at level {level}"),
        Reason::Ball(ball) => match ball_name(tables, ball) {
            Some(name) => format!("a {name} can't catch {species} where it was met"),
            None => format!("its Poké Ball can't catch {species} where it was met"),
        },
        Reason::GroundTile(_) => "the ground it was met on doesn't fit the place".to_owned(),
        Reason::Correlation => {
            "its personality value and IVs don't fit how the game makes wild Pokémon".to_owned()
        }
        Reason::NotBreedable => format!("{species} can't hatch from an egg"),
        Reason::Generation
        | Reason::Version(_)
        | Reason::Egg
        | Reason::Unchecked(_)
        | Reason::NoEncounter(_)
        | Reason::Move(_) => reason.to_string(),
    }
}

fn unchecked(tables: &Tables, reason: Reason) -> String {
    match reason {
        Reason::Generation => {
            "Not checked, since only Pokémon from Gen 4 games are checked".to_owned()
        }
        Reason::Version(version) => match origin_name(version) {
            Some(name) => format!("Not checked yet for Pokémon from {name}"),
            None => "Not checked yet for Pokémon from other games".to_owned(),
        },
        Reason::Egg => "Not checked yet for eggs".to_owned(),
        Reason::Unchecked(what) => format!("Not checked yet for {what}"),
        Reason::NoEncounter(place) => match tables.ds().and_then(|ds| ds.place_name(place)) {
            Some(name) => format!(
                "Not found in the wild at {name} at this level. Gifts and one-time encounters aren't checked yet"
            ),
            None => "Gifts and one-time encounters aren't checked yet".to_owned(),
        },
        Reason::Move(id) => match tables.move_name(id) {
            Some(name) => format!(
                "Can't learn {name} by levelling up. Moves from TMs, tutors and eggs aren't checked yet"
            ),
            None => "Moves from TMs, tutors and eggs aren't checked yet".to_owned(),
        },
        other => format!("Not checked yet, because {other}"),
    }
}

fn ball_name(tables: &Tables, ball: u8) -> Option<String> {
    let item = if ball >= HGSS_FIRST_BALL {
        HGSS_BALL_ITEMS + u16::from(ball)
    } else {
        u16::from(ball)
    };
    tables.item_name(item)
}

fn origin_name(version: u8) -> Option<&'static str> {
    Some(match version {
        1 => "Sapphire",
        2 => "Ruby",
        3 => "Emerald",
        4 => "FireRed",
        5 => "LeafGreen",
        7 => "HeartGold",
        8 => "SoulSilver",
        10 => "Diamond",
        11 => "Pearl",
        12 => "Platinum",
        15 => "Colosseum or XD",
        _ => return None,
    })
}
