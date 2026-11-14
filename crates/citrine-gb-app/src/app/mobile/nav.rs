use crate::app::tabs::Tab;
use crate::app::ui_state::settings::SettingsTab;
use crate::icons;

const MAX_STEPS: usize = 32;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    #[default]
    Games,
    Friends,
    Media,
    Settings,
    You,
}

impl Place {
    pub const ALL: [Place; 5] = [
        Place::Games,
        Place::Friends,
        Place::Media,
        Place::Settings,
        Place::You,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Place::Games => "Games",
            Place::Friends => "Friends",
            Place::Media => "Media",
            Place::Settings => "Settings",
            Place::You => "You",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Place::Games => icons::GAME_CONTROLLER,
            Place::Friends => icons::USERS,
            Place::Media => icons::IMAGES,
            Place::Settings => icons::GEAR,
            Place::You => icons::USER_CIRCLE,
        }
    }

    pub fn of(tab: Tab) -> Place {
        match tab {
            Tab::Friends => Place::Friends,
            Tab::Media => Place::Media,
            Tab::Settings | Tab::Info => Place::Settings,
            Tab::Stats => Place::You,
            _ => Place::Games,
        }
    }

    fn index(self) -> usize {
        match self {
            Place::Games => 0,
            Place::Friends => 1,
            Place::Media => 2,
            Place::Settings => 3,
            Place::You => 4,
        }
    }

    fn root(self) -> Option<Tab> {
        match self {
            Place::Friends => Some(Tab::Friends),
            Place::Media => Some(Tab::Media),
            Place::Settings => Some(Tab::Settings),
            Place::Games | Place::You => None,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Nav {
    place: Place,
    pages: [Vec<Tab>; 5],
    settings_page: Option<SettingsTab>,
    on_game: bool,
    visits: Vec<Place>,
    more: bool,
}

impl Nav {
    pub fn place(&self) -> Place {
        self.place
    }

    pub fn page(&self) -> Option<Tab> {
        self.pages[self.place.index()].last().copied()
    }

    pub fn settings_page(&self) -> Option<SettingsTab> {
        self.settings_page
            .filter(|_| self.place == Place::Settings && self.page().is_none())
    }

    pub fn more_open(&self) -> bool {
        self.more
    }

    pub fn on_game(&self) -> bool {
        self.place == Place::Games && self.on_game && self.page().is_none()
    }

    pub fn game_in_view(&self) -> bool {
        self.on_game() && !self.more
    }

    pub fn showing(&self, tab: Tab) -> bool {
        match self.page() {
            Some(page) => page == tab,
            None if tab == Tab::GameBoy => self.on_game(),
            None => self.place.root() == Some(tab) && self.settings_page().is_none(),
        }
    }

    pub fn go(&mut self, place: Place) {
        self.more = false;
        if place == self.place {
            self.back_to_root();
            return;
        }
        self.visit(place);
    }

    fn back_to_root(&mut self) {
        self.more = false;
        self.pages[self.place.index()].clear();
        match self.place {
            Place::Settings => self.settings_page = None,
            Place::Games => self.on_game = false,
            _ => {}
        }
    }

    pub fn open(&mut self, tab: Tab) {
        self.more = false;
        if tab == Tab::GameBoy {
            self.show_game();
            return;
        }
        let place = Place::of(tab);
        if place != self.place {
            self.visit(place);
        }
        let pages = &mut self.pages[place.index()];
        if place.root() == Some(tab) {
            pages.clear();
            if place == Place::Settings {
                self.settings_page = None;
            }
        } else if pages.last() != Some(&tab) {
            pages.retain(|page| *page != tab);
            pages.push(tab);
        }
    }

    pub fn open_settings(&mut self, page: SettingsTab) {
        self.open(Tab::Settings);
        self.settings_page = Some(page);
    }

    pub fn show_game(&mut self) {
        self.more = false;
        if self.place != Place::Games {
            self.visit(Place::Games);
        }
        self.pages[Place::Games.index()].clear();
        self.on_game = true;
    }

    pub fn leave_game(&mut self) {
        self.on_game = false;
        self.more = false;
    }

    pub fn set_more(&mut self, open: bool) {
        self.more = open && self.on_game();
    }

    pub fn back(&mut self) -> bool {
        if std::mem::take(&mut self.more) || self.pages[self.place.index()].pop().is_some() {
            return true;
        }
        if self.place == Place::Settings && self.settings_page.take().is_some() {
            return true;
        }
        if self.place == Place::Games && std::mem::take(&mut self.on_game) {
            return true;
        }
        if let Some(previous) = self.visits.pop() {
            self.place = previous;
            return true;
        }
        if self.place == Place::Games {
            return false;
        }
        self.place = Place::Games;
        true
    }

    pub fn depth(&self) -> usize {
        let mut nav = self.clone();
        let mut steps = 0;
        while steps < MAX_STEPS && nav.back() {
            steps += 1;
        }
        steps
    }

    fn visit(&mut self, place: Place) {
        self.visits
            .retain(|visited| *visited != place && *visited != self.place);
        self.visits.push(self.place);
        self.place = place;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing() -> Nav {
        let mut nav = Nav::default();
        nav.show_game();
        nav
    }

    #[test]
    fn back_on_a_game_goes_to_the_games_list_and_never_further_in_one_step() {
        let mut nav = playing();
        assert!(nav.on_game());
        assert!(nav.back());
        assert_eq!(nav.place(), Place::Games);
        assert!(!nav.on_game());
        assert!(!nav.back(), "the games list is the root");
    }

    #[test]
    fn back_closes_the_more_sheet_before_anything_else() {
        let mut nav = playing();
        nav.set_more(true);
        assert!(nav.more_open());
        assert!(!nav.game_in_view());
        assert!(nav.back());
        assert!(nav.on_game());
        assert!(nav.game_in_view());
    }

    #[test]
    fn the_more_sheet_only_opens_over_a_game() {
        let mut nav = Nav::default();
        nav.set_more(true);
        assert!(!nav.more_open());
    }

    #[test]
    fn back_returns_to_the_place_the_user_came_from() {
        let mut nav = playing();
        nav.go(Place::Media);
        nav.go(Place::Settings);
        assert!(nav.back());
        assert_eq!(nav.place(), Place::Media);
        assert!(nav.back());
        assert_eq!(nav.place(), Place::Games);
        assert!(nav.on_game(), "the game is where the user was");
    }

    #[test]
    fn a_place_shows_as_it_was_left() {
        let mut nav = Nav::default();
        nav.open_settings(SettingsTab::Sound);
        nav.go(Place::Friends);
        nav.go(Place::Settings);
        assert_eq!(nav.settings_page(), Some(SettingsTab::Sound));
    }

    #[test]
    fn tapping_the_shown_place_goes_to_its_root() {
        let mut nav = Nav::default();
        nav.open_settings(SettingsTab::Controls);
        nav.go(Place::Settings);
        assert_eq!(nav.settings_page(), None);
        assert!(nav.showing(Tab::Settings));

        let mut nav = playing();
        nav.go(Place::Games);
        assert!(!nav.on_game());
    }

    #[test]
    fn a_page_opens_in_its_place_and_back_pops_it() {
        let mut nav = playing();
        nav.open(Tab::Saves);
        assert_eq!(nav.place(), Place::Games);
        assert!(nav.showing(Tab::Saves));
        assert!(!nav.game_in_view());
        assert!(nav.back());
        assert!(nav.on_game());

        nav.open(Tab::Stats);
        assert_eq!(nav.place(), Place::You);
        assert!(nav.back());
        assert_eq!(nav.place(), Place::You, "stats closes to the profile");
        assert!(nav.back());
        assert!(nav.on_game());
    }

    #[test]
    fn about_is_a_settings_page() {
        let mut nav = Nav::default();
        nav.open(Tab::Info);
        assert_eq!(nav.place(), Place::Settings);
        assert!(nav.showing(Tab::Info));
    }

    #[test]
    fn depth_counts_every_back_step_to_the_root() {
        let mut nav = playing();
        assert_eq!(nav.depth(), 1);
        nav.go(Place::Settings);
        nav.open_settings(SettingsTab::Sound);
        nav.go(Place::Friends);
        let steps = nav.depth();
        let mut walk = nav.clone();
        let mut taken = 0;
        while walk.back() {
            taken += 1;
        }
        assert_eq!(steps, taken);
        assert_eq!(walk.place(), Place::Games);
    }

    #[test]
    fn a_visit_is_listed_once() {
        let mut nav = Nav::default();
        for place in [Place::Media, Place::Settings, Place::Media, Place::Settings] {
            nav.go(place);
        }
        assert!(nav.back());
        assert_eq!(nav.place(), Place::Media);
        assert!(nav.back());
        assert_eq!(nav.place(), Place::Games);
        assert!(!nav.back());
    }
}
