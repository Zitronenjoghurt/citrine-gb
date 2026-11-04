use super::Pages;
use super::parts::{badge_color, card};
use crate::icons;
use citrine_gb_gamedata::{Game, GameState, Generation, Language, Milestone, Step, Version};
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
    }
}

pub(super) fn milestone_name(game: Game, milestone: Milestone) -> (String, Option<&'static str>) {
    let english = game.language == Language::English;
    let region = region(game.version);
    match milestone {
        Milestone::Pokedex if english => ("Get the Pokédex".to_owned(), Some(region.professor)),
        Milestone::Pokedex => ("Get the Pokédex".to_owned(), None),
        Milestone::EliteFour => ("Elite Four".to_owned(), Some(region.league)),
        Milestone::Badge(index) => match region.leaders.get(index).filter(|_| english) {
            Some((leader, town)) => ((*leader).to_owned(), Some(town)),
            None => (format!("Badge {}", index + 1), None),
        },
    }
}

fn headings(version: Version, journey: &[Step]) -> Vec<(usize, &'static str)> {
    let mut headings = vec![(0, region(version).name)];
    let kanto_from = journey
        .iter()
        .position(|step| matches!(step.milestone, Milestone::Badge(8..)));
    if let Some(at) = kanto_from.filter(|_| version.generation() == Generation::Two) {
        headings.push((at, "Kanto"));
    }
    headings
}

impl Pages<'_> {
    pub(super) fn journey(&self, ui: &mut Ui, state: &GameState) {
        let game = self.game.tables.game();
        let next = state.journey.iter().position(|step| !step.done);
        let headings = headings(game.version, &state.journey);
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (i, step) in state.journey.iter().enumerate() {
                if let Some((_, name)) = headings.iter().find(|(at, _)| *at == i) {
                    if i > 0 {
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(*name).weak());
                }
                self.step_row(ui, state, step, Some(i) == next);
            }
        });
    }

    fn step_row(&self, ui: &mut Ui, state: &GameState, step: &Step, next: bool) {
        let (name, place) = milestone_name(self.game.tables.game(), step.milestone);
        ui.horizontal(|ui| {
            let (icon, color) = if step.done {
                (icons::CHECK_CIRCLE, badge_color(ui))
            } else if next {
                (icons::ARROW_CIRCLE_RIGHT, ui.visuals().strong_text_color())
            } else {
                (icons::CIRCLE, ui.visuals().weak_text_color())
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
}
