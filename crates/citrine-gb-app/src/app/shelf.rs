use super::Citrine;
use super::events::AppEvent;
use crate::homebrew::HomebrewGame;
use crate::homebrew::loader::Done;

const SHELF_LOAD: &str = "shelf";

impl Citrine {
    pub(super) fn play_shelf_game(&mut self, game: &'static HomebrewGame) {
        match self.shelf.play(game) {
            Some(data) => self.handle_load_rom_data(data),
            None => self.try_start_audio(),
        }
    }

    pub(super) fn watch_shelf(&mut self) {
        match self.shelf.poll() {
            Some(Done::Play(data)) => self.handle_load_rom_data(data),
            Some(Done::Failed(game)) => {
                self.toasts
                    .error(format!("Couldn't download {}", game.title))
                    .button("Try again", AppEvent::PlayShelfGame { game });
            }
            None => {}
        }
        match self.shelf.waiting() {
            Some((game, progress)) => self.toasts.load(
                SHELF_LOAD,
                format!("Downloading {}", game.title),
                progress,
                web_time::Instant::now(),
            ),
            None => self.toasts.load_done(SHELF_LOAD),
        }
    }
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::tests::phone_app;
    use crate::homebrew::loader::ShelfLoader;
    use crate::homebrew::loader::tests::{Fake, fetched, stored};
    use citrine_gb::console::ConsoleKind;

    #[test]
    fn a_failed_download_offers_try_again_and_the_next_one_starts_the_game() {
        let mut app = phone_app("shelf-download");
        let fake = Fake::default();
        app.shelf = ShelfLoader::new(Box::new(fake.clone()));
        let game = fetched("pentris");
        app.play_shelf_game(game);
        app.watch_shelf();
        assert!(!app.emulator.has_game());
        fake.last().finish(Err("offline".to_owned()));
        app.watch_shelf();
        assert_eq!(app.toasts.texts(), ["Couldn't download Pentris"]);
        assert_eq!(app.toasts.button_labels(), ["Try again"]);
        app.play_shelf_game(game);
        assert_eq!(fake.count(), 2);
        fake.last().finish(Ok(stored(game)));
        app.watch_shelf();
        assert!(app.emulator.has_game());
        assert_eq!(app.emulator.kind(), ConsoleKind::Gba);
        assert_eq!(app.emulator.rom_key(), Some(game.rom_key));
    }
}
