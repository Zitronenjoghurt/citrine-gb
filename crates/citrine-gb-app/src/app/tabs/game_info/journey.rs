use super::Pages;
use super::parts::{badge_color, card};
use crate::icons;
use citrine_gb_gamedata::{Game, GameState, Language, Milestone, Step, Version};
use egui::{Align, Layout, RichText, Ui};

const GEN1_LEADERS: [(&str, &str); 8] = [
    ("Brock", "Pewter City"),
    ("Misty", "Cerulean City"),
    ("Lt. Surge", "Vermilion City"),
    ("Erika", "Celadon City"),
    ("Koga", "Fuchsia City"),
    ("Sabrina", "Saffron City"),
    ("Blaine", "Cinnabar Island"),
    ("Giovanni", "Viridian City"),
];

const GEN2_LEADERS: [(&str, &str); 16] = [
    ("Falkner", "Violet City"),
    ("Bugsy", "Azalea Town"),
    ("Whitney", "Goldenrod City"),
    ("Morty", "Ecruteak City"),
    ("Chuck", "Cianwood City"),
    ("Jasmine", "Olivine City"),
    ("Pryce", "Mahogany Town"),
    ("Clair", "Blackthorn City"),
    ("Brock", "Pewter City"),
    ("Misty", "Cerulean City"),
    ("Lt. Surge", "Vermilion City"),
    ("Erika", "Celadon City"),
    ("Janine", "Fuchsia City"),
    ("Sabrina", "Saffron City"),
    ("Blaine", "Seafoam Islands"),
    ("Blue", "Viridian City"),
];

const HOENN_LEADERS: [(&str, &str); 8] = [
    ("Roxanne", "Rustboro City"),
    ("Brawly", "Dewford Town"),
    ("Wattson", "Mauville City"),
    ("Flannery", "Lavaridge Town"),
    ("Norman", "Petalburg City"),
    ("Winona", "Fortree City"),
    ("Tate & Liza", "Mossdeep City"),
    ("Wallace", "Sootopolis City"),
];

const EMERALD_LEADERS: [(&str, &str); 8] = [
    ("Roxanne", "Rustboro City"),
    ("Brawly", "Dewford Town"),
    ("Wattson", "Mauville City"),
    ("Flannery", "Lavaridge Town"),
    ("Norman", "Petalburg City"),
    ("Winona", "Fortree City"),
    ("Tate & Liza", "Mossdeep City"),
    ("Juan", "Sootopolis City"),
];

const SINNOH_LEADERS: [(&str, &str); 8] = [
    ("Roark", "Oreburgh City"),
    ("Gardenia", "Eterna City"),
    ("Maylene", "Veilstone City"),
    ("Crasher Wake", "Pastoria City"),
    ("Fantina", "Hearthome City"),
    ("Byron", "Canalave City"),
    ("Candice", "Snowpoint City"),
    ("Volkner", "Sunyshore City"),
];

const BLACK_LEADERS: [(&str, &str); 8] = [
    ("Cilan, Chili or Cress", "Striaton City"),
    ("Lenora", "Nacrene City"),
    ("Burgh", "Castelia City"),
    ("Elesa", "Nimbasa City"),
    ("Clay", "Driftveil City"),
    ("Skyla", "Mistralton City"),
    ("Brycen", "Icirrus City"),
    ("Drayden", "Opelucid City"),
];

const WHITE_LEADERS: [(&str, &str); 8] = [
    ("Cilan, Chili or Cress", "Striaton City"),
    ("Lenora", "Nacrene City"),
    ("Burgh", "Castelia City"),
    ("Elesa", "Nimbasa City"),
    ("Clay", "Driftveil City"),
    ("Skyla", "Mistralton City"),
    ("Brycen", "Icirrus City"),
    ("Iris", "Opelucid City"),
];

const UNOVA_2_LEADERS: [(&str, &str); 8] = [
    ("Cheren", "Aspertia City"),
    ("Roxie", "Virbank City"),
    ("Burgh", "Castelia City"),
    ("Elesa", "Nimbasa City"),
    ("Clay", "Driftveil City"),
    ("Skyla", "Mistralton City"),
    ("Drayden", "Opelucid City"),
    ("Marlon", "Humilau City"),
];

struct Region {
    name: &'static str,
    professor: &'static str,
    league: &'static str,
    leaders: &'static [(&'static str, &'static str)],
}

fn region(version: Version) -> Region {
    match version {
        Version::Red
        | Version::Green
        | Version::Blue
        | Version::Yellow
        | Version::FireRed
        | Version::LeafGreen => Region {
            name: "Kanto",
            professor: "Prof. Oak",
            league: "Indigo Plateau",
            leaders: &GEN1_LEADERS,
        },
        Version::Gold | Version::Silver | Version::Crystal => Region {
            name: "Johto",
            professor: "Prof. Elm",
            league: "Indigo Plateau",
            leaders: &GEN2_LEADERS,
        },
        Version::Ruby | Version::Sapphire => Region {
            name: "Hoenn",
            professor: "Prof. Birch",
            league: "Ever Grande City",
            leaders: &HOENN_LEADERS,
        },
        Version::Emerald => Region {
            name: "Hoenn",
            professor: "Prof. Birch",
            league: "Ever Grande City",
            leaders: &EMERALD_LEADERS,
        },
        Version::Diamond | Version::Pearl | Version::Platinum => Region {
            name: "Sinnoh",
            professor: "Prof. Rowan",
            league: "Pokémon League",
            leaders: &SINNOH_LEADERS,
        },
        Version::HeartGold | Version::SoulSilver => Region {
            name: "Johto",
            professor: "Prof. Elm",
            league: "Indigo Plateau",
            leaders: &GEN2_LEADERS,
        },
        Version::Black => Region {
            name: "Unova",
            professor: "Prof. Juniper",
            league: "Pokémon League",
            leaders: &BLACK_LEADERS,
        },
        Version::White => Region {
            name: "Unova",
            professor: "Prof. Juniper",
            league: "Pokémon League",
            leaders: &WHITE_LEADERS,
        },
        Version::Black2 | Version::White2 => Region {
            name: "Unova",
            professor: "Prof. Juniper",
            league: "Pokémon League",
            leaders: &UNOVA_2_LEADERS,
        },
    }
}

pub(super) fn step_name(game: Game, journey: &[Step], at: usize) -> (String, Option<&'static str>) {
    let step = journey[at];
    if !step.done {
        let name = match step.milestone {
            Milestone::Pokedex => "Get the Pokédex".to_owned(),
            Milestone::EliteFour => "Elite Four".to_owned(),
            Milestone::Badge(_) => format!("Badge {}", badge_number(journey, at)),
        };
        return (name, None);
    }
    let english = game.language == Language::English;
    let region = region(game.version);
    match step.milestone {
        Milestone::Pokedex if english => ("Get the Pokédex".to_owned(), Some(region.professor)),
        Milestone::Pokedex => ("Get the Pokédex".to_owned(), None),
        Milestone::EliteFour => ("Elite Four".to_owned(), Some(region.league)),
        Milestone::Badge(index) => match region.leaders.get(index).filter(|_| english) {
            Some((leader, town)) => ((*leader).to_owned(), Some(town)),
            None => (format!("Badge {}", badge_number(journey, at)), None),
        },
    }
}

fn badge_number(journey: &[Step], at: usize) -> usize {
    journey[..=at]
        .iter()
        .filter(|step| matches!(step.milestone, Milestone::Badge(_)))
        .count()
}

fn kanto_from(version: Version, journey: &[Step]) -> Option<usize> {
    let two_regions = matches!(
        version,
        Version::Gold
            | Version::Silver
            | Version::Crystal
            | Version::HeartGold
            | Version::SoulSilver
    );
    journey
        .iter()
        .position(|step| matches!(step.milestone, Milestone::Badge(8..)))
        .filter(|_| two_regions)
}

pub(super) fn reached(version: Version, journey: &[Step], in_kanto: bool) -> &[Step] {
    let Some(at) = kanto_from(version, journey) else {
        return journey;
    };
    let gated = matches!(version, Version::Gold | Version::Silver | Version::Crystal);
    let there = gated
        || in_kanto
        || journey.iter().any(|step| {
            step.done && matches!(step.milestone, Milestone::EliteFour | Milestone::Badge(8..))
        });
    if there { journey } else { &journey[..at] }
}

fn headings(version: Version, journey: &[Step]) -> Vec<(usize, &'static str)> {
    let mut headings = vec![(0, region(version).name)];
    if let Some(at) = kanto_from(version, journey) {
        headings.push((at, "Kanto"));
    }
    headings
}

impl Pages<'_> {
    pub(super) fn reached_steps<'a>(&self, state: &'a GameState) -> &'a [Step] {
        let in_kanto = state
            .location
            .as_ref()
            .and_then(|position| position.gb())
            .zip(self.game.tables.gb())
            .and_then(|(location, tables)| tables.spot(location))
            .is_some_and(|spot| spot.region == citrine_gb_gamedata::Region::Kanto);
        reached(self.game.tables.game().version, &state.journey, in_kanto)
    }

    pub(super) fn shown_badges<'a>(&self, state: &'a GameState) -> &'a [bool] {
        let badges = &state.trainer.badges;
        if self.reached_steps(state).len() < state.journey.len() {
            &badges[..badges.len().min(8)]
        } else {
            badges
        }
    }

    pub(super) fn journey(&self, ui: &mut Ui, state: &GameState) {
        let game = self.game.tables.game();
        let journey = self.reached_steps(state);
        let next = journey.iter().position(|step| !step.done);
        let headings = headings(game.version, journey);
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for i in 0..journey.len() {
                if let Some((_, name)) = headings.iter().find(|(at, _)| *at == i) {
                    if i > 0 {
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(*name).weak());
                }
                self.step_row(ui, state, journey, i, Some(i) == next);
            }
        });
    }

    fn step_row(&self, ui: &mut Ui, state: &GameState, journey: &[Step], at: usize, next: bool) {
        let step = journey[at];
        let (name, place) = step_name(self.game.tables.game(), journey, at);
        ui.horizontal(|ui| {
            let (icon, color) = if step.done {
                (icons::CHECK_CIRCLE, badge_color(ui))
            } else if next {
                (icons::ARROW_CIRCLE_RIGHT, ui.visuals().strong_text_color())
            } else {
                (icons::LOCK_SIMPLE, ui.visuals().weak_text_color())
            };
            ui.label(RichText::new(icon).color(color));
            let name = RichText::new(name);
            ui.label(if step.done || next {
                name.strong()
            } else {
                name.weak()
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let wins = state
                    .hall_of_fame
                    .filter(|wins| *wins > 0 && step.milestone == Milestone::EliteFour);
                if let Some(wins) = wins {
                    let times = if wins == 1 { "time" } else { "times" };
                    ui.label(RichText::new(format!("Won {wins} {times}")).weak());
                } else if let Some(place) = place {
                    ui.label(RichText::new(place).weak());
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(badges: usize) -> Vec<Step> {
        let step = |milestone| Step {
            milestone,
            done: true,
        };
        let mut steps = vec![step(Milestone::Pokedex)];
        steps.extend((0..8).map(|bit| step(Milestone::Badge(bit))));
        steps.push(step(Milestone::EliteFour));
        steps.extend((8..badges).map(|bit| step(Milestone::Badge(bit))));
        steps
    }

    #[test]
    fn a_gen2_journey_past_the_league_heads_johto_then_kanto() {
        assert_eq!(
            headings(Version::Silver, &steps(16)),
            vec![(0, "Johto"), (10, "Kanto")]
        );
    }

    #[test]
    fn other_journeys_head_their_one_region() {
        assert_eq!(headings(Version::Emerald, &steps(8)), vec![(0, "Hoenn")]);
        assert_eq!(headings(Version::FireRed, &steps(8)), vec![(0, "Kanto")]);
        assert_eq!(headings(Version::Red, &steps(8)), vec![(0, "Kanto")]);
    }

    fn early() -> Vec<Step> {
        let mut steps = steps(16);
        for (i, step) in steps.iter_mut().enumerate() {
            step.done = i < 3;
        }
        steps
    }

    fn english(version: Version) -> Game {
        Game {
            version,
            language: Language::English,
            revision: 0,
        }
    }

    #[test]
    fn a_gen4_johto_journey_before_kanto_shows_no_kanto() {
        for version in [Version::HeartGold, Version::SoulSilver] {
            let journey = early();
            let shown = reached(version, &journey, false);
            assert_eq!(shown.len(), 10, "{version:?}");
            assert_eq!(headings(version, shown), vec![(0, "Johto")]);
            assert_eq!(reached(version, &journey, true).len(), 18);
        }
    }

    #[test]
    fn a_gen4_johto_journey_shows_kanto_once_the_league_falls() {
        let mut journey = early();
        journey[9].done = true;
        assert_eq!(reached(Version::HeartGold, &journey, false).len(), 18);
    }

    #[test]
    fn a_gb_johto_journey_trusts_the_save_to_leave_kanto_out() {
        let journey = steps(16);
        assert_eq!(reached(Version::Crystal, &journey, false).len(), 18);
        assert_eq!(reached(Version::Crystal, &journey[..10], false).len(), 10);
    }

    #[test]
    fn only_reached_steps_carry_a_name_or_a_place() {
        let names: Vec<String> = [
            Version::Red,
            Version::Crystal,
            Version::Emerald,
            Version::HeartGold,
            Version::Platinum,
            Version::Black2,
        ]
        .into_iter()
        .flat_map(|version| {
            let journey = early();
            let game = english(version);
            (0..journey.len())
                .map(|at| {
                    let (name, place) = step_name(game, &journey, at);
                    format!("{name}|{}", place.unwrap_or(""))
                })
                .collect::<Vec<_>>()
        })
        .collect();
        assert_eq!(
            &names[..4],
            [
                "Get the Pokédex|Prof. Oak",
                "Brock|Pewter City",
                "Misty|Cerulean City",
                "Badge 3|"
            ]
        );
        assert_eq!(names[9], "Elite Four|");
        assert_eq!(names[10], "Badge 9|");
        for (at, name) in names.iter().enumerate() {
            if at % 18 >= 3 {
                assert!(name.ends_with('|'), "{name}");
                assert!(
                    name.starts_with("Badge ") || name == "Elite Four|",
                    "{name}"
                );
            }
        }
    }
}
