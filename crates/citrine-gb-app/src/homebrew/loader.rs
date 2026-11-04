use super::{HomebrewGame, ShelfRom};
use crate::progress::{Counter, Progress};
use citrine_gb::rom::id::RomId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Outcome = Option<Result<Vec<u8>, String>>;

#[derive(Clone, Default)]
pub struct Download {
    counter: Counter,
    result: Arc<Mutex<Outcome>>,
}

impl Download {
    pub fn new(total: u64) -> Self {
        let download = Self::default();
        download.counter.set_total(total);
        download
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    pub fn add(&self, bytes: u64) {
        self.counter.add(bytes);
    }

    pub fn finish(&self, result: Result<Vec<u8>, String>) {
        *self.result.lock().unwrap() = Some(result);
    }

    fn progress(&self) -> Progress {
        self.counter.bytes()
    }

    fn take(&self) -> Outcome {
        self.result.lock().unwrap().take()
    }
}

pub trait Fetcher {
    fn fetch(&mut self, path: &'static str, bytes: u64) -> Download;
}

enum State {
    Loading(&'static HomebrewGame, Download),
    Ready(Vec<u8>),
    Failed,
}

pub enum Done {
    Play(Vec<u8>),
    Failed(&'static HomebrewGame),
}

pub struct ShelfLoader {
    fetcher: Box<dyn Fetcher>,
    states: HashMap<&'static str, State>,
    pending: Option<&'static HomebrewGame>,
}

impl Default for ShelfLoader {
    fn default() -> Self {
        Self::new(Box::new(platform_fetcher()))
    }
}

impl ShelfLoader {
    pub fn new(fetcher: Box<dyn Fetcher>) -> Self {
        Self {
            fetcher,
            states: HashMap::new(),
            pending: None,
        }
    }

    pub fn prefetch(&mut self, game: &'static HomebrewGame) {
        if !self.states.contains_key(game.id) {
            self.start(game);
        }
    }

    pub fn play(&mut self, game: &'static HomebrewGame) -> Option<Vec<u8>> {
        self.pending = None;
        if let Some(data) = game.embedded() {
            return Some(data);
        }
        match self.states.get(game.id) {
            Some(State::Ready(data)) => return Some(data.clone()),
            Some(State::Loading(..)) => {}
            Some(State::Failed) | None => self.start(game),
        }
        self.pending = Some(game);
        None
    }

    pub fn waiting(&self) -> Option<(&'static HomebrewGame, Progress)> {
        let game = self.pending?;
        match self.states.get(game.id) {
            Some(State::Loading(_, download)) => Some((game, download.progress())),
            _ => None,
        }
    }

    pub fn poll(&mut self) -> Option<Done> {
        for state in self.states.values_mut() {
            let State::Loading(game, download) = state else {
                continue;
            };
            let Some(result) = download.take() else {
                continue;
            };
            *state = match result.ok().and_then(|bytes| decode(game, bytes)) {
                Some(data) => State::Ready(data),
                None => State::Failed,
            };
        }
        let game = self.pending?;
        let done = match self.states.get(game.id)? {
            State::Loading(..) => return None,
            State::Ready(data) => Done::Play(data.clone()),
            State::Failed => Done::Failed(game),
        };
        self.pending = None;
        Some(done)
    }

    fn start(&mut self, game: &'static HomebrewGame) {
        if let ShelfRom::Fetch { path, bytes } = game.rom {
            let download = self.fetcher.fetch(path, bytes);
            self.states.insert(game.id, State::Loading(game, download));
        }
    }
}

fn decode(game: &HomebrewGame, bytes: Vec<u8>) -> Option<Vec<u8>> {
    let matches = |data: &[u8]| RomId::new(data).sha1 == game.sha1;
    if matches(&bytes) {
        return Some(bytes);
    }
    let mut raw = Vec::with_capacity(game.size);
    std::io::Read::read_to_end(
        &mut brotli::Decompressor::new(bytes.as_slice(), 4096),
        &mut raw,
    )
    .ok()?;
    matches(&raw).then_some(raw)
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_fetcher() -> impl Fetcher {
    struct Unavailable;
    impl Fetcher for Unavailable {
        fn fetch(&mut self, _path: &'static str, bytes: u64) -> Download {
            let download = Download::new(bytes);
            download.finish(Err("no downloads in this build".to_owned()));
            download
        }
    }
    Unavailable
}

#[cfg(target_arch = "wasm32")]
fn platform_fetcher() -> impl Fetcher {
    web::Web
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{Download, Fetcher};
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    pub struct Web;

    impl Fetcher for Web {
        fn fetch(&mut self, path: &'static str, bytes: u64) -> Download {
            let download = Download::new(bytes);
            let task = download.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = read(path, &task).await.map_err(|err| format!("{err:?}"));
                task.finish(result);
            });
            download
        }
    }

    async fn read(path: &str, download: &Download) -> Result<Vec<u8>, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let response: web_sys::Response = JsFuture::from(window.fetch_with_str(path))
            .await?
            .dyn_into()?;
        if !response.ok() {
            return Err(JsValue::from(response.status()));
        }
        let body = response.body().ok_or("no body")?;
        let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().dyn_into()?;
        let mut bytes = Vec::new();
        loop {
            let chunk = JsFuture::from(reader.read()).await?;
            let done = js_sys::Reflect::get(&chunk, &"done".into())?;
            if done.as_bool().unwrap_or(true) {
                return Ok(bytes);
            }
            let value: js_sys::Uint8Array =
                js_sys::Reflect::get(&chunk, &"value".into())?.dyn_into()?;
            let start = bytes.len();
            bytes.resize(start + value.length() as usize, 0);
            value.copy_to(&mut bytes[start..]);
            download.add(value.length() as u64);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::homebrew::{HOMEBREW_GAMES, ShelfConsole};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    pub(crate) struct Fake {
        started: Rc<RefCell<Vec<(&'static str, Download)>>>,
    }

    impl Fetcher for Fake {
        fn fetch(&mut self, path: &'static str, bytes: u64) -> Download {
            let download = Download::new(bytes);
            self.started.borrow_mut().push((path, download.clone()));
            download
        }
    }

    impl Fake {
        pub(crate) fn count(&self) -> usize {
            self.started.borrow().len()
        }

        pub(crate) fn last(&self) -> Download {
            self.started.borrow().last().unwrap().1.clone()
        }
    }

    fn shelf_game(id: &str) -> &'static HomebrewGame {
        HOMEBREW_GAMES.iter().find(|game| game.id == id).unwrap()
    }

    pub(crate) fn fetched(id: &str) -> &'static HomebrewGame {
        let mut game = *shelf_game(id);
        game.rom = ShelfRom::Fetch {
            path: "homebrew/fetched.br",
            bytes: 1000,
        };
        Box::leak(Box::new(game))
    }

    pub(crate) fn stored(game: &HomebrewGame) -> Vec<u8> {
        let ShelfRom::Embedded(data) = shelf_game(game.id).rom else {
            unreachable!()
        };
        data.to_vec()
    }

    fn fetched_game() -> &'static HomebrewGame {
        fetched("snake")
    }

    fn raw(game: &HomebrewGame) -> Vec<u8> {
        shelf_game(game.id).embedded().unwrap()
    }

    fn loader() -> (ShelfLoader, Fake) {
        let fake = Fake::default();
        (ShelfLoader::new(Box::new(fake.clone())), fake)
    }

    #[test]
    fn an_embedded_game_plays_at_once_without_a_download() {
        let (mut loader, fake) = loader();
        let game = HOMEBREW_GAMES
            .iter()
            .find(|game| game.console == ShelfConsole::GameBoy)
            .unwrap();
        assert_eq!(loader.play(game), game.embedded());
        assert_eq!(fake.count(), 0);
        assert!(loader.poll().is_none());
    }

    #[test]
    fn a_fetched_game_downloads_shows_its_bytes_then_plays() {
        let (mut loader, fake) = loader();
        let game = fetched_game();
        assert!(loader.play(game).is_none());
        assert_eq!(fake.count(), 1);
        fake.last().add(400);
        let (waiting, progress) = loader.waiting().unwrap();
        assert_eq!(waiting.id, game.id);
        assert_eq!(progress, Progress::bytes(400, 1000));
        assert!(loader.poll().is_none());
        fake.last().finish(Ok(stored(game)));
        let Some(Done::Play(data)) = loader.poll() else {
            panic!("the game did not start");
        };
        assert_eq!(data, raw(game));
        assert!(loader.waiting().is_none());
        assert!(loader.poll().is_none());
    }

    #[test]
    fn a_finished_download_plays_again_without_a_second_one() {
        let (mut loader, fake) = loader();
        let game = fetched_game();
        loader.prefetch(game);
        loader.prefetch(game);
        assert_eq!(fake.count(), 1);
        fake.last().finish(Ok(stored(game)));
        assert!(loader.poll().is_none());
        assert_eq!(loader.play(game), Some(raw(game)));
        assert_eq!(loader.play(game), Some(raw(game)));
        assert_eq!(fake.count(), 1);
    }

    #[test]
    fn a_server_that_unpacks_the_file_still_gives_the_game() {
        let (mut loader, fake) = loader();
        let game = fetched_game();
        loader.play(game);
        fake.last().finish(Ok(raw(game)));
        assert!(matches!(loader.poll(), Some(Done::Play(data)) if data == raw(game)));
    }

    #[test]
    fn a_failed_or_damaged_download_reports_once_and_try_again_starts_a_new_one() {
        for result in [Err("offline".to_owned()), Ok(vec![1, 2, 3])] {
            let (mut loader, fake) = loader();
            let game = fetched_game();
            loader.play(game);
            fake.last().finish(result);
            assert!(matches!(loader.poll(), Some(Done::Failed(failed)) if failed.id == game.id));
            assert!(loader.poll().is_none());
            loader.prefetch(game);
            assert_eq!(fake.count(), 1);
            assert!(loader.play(game).is_none());
            assert_eq!(fake.count(), 2);
            fake.last().finish(Ok(stored(game)));
            assert!(matches!(loader.poll(), Some(Done::Play(_))));
        }
    }

    #[test]
    fn a_failed_prefetch_stays_quiet_until_the_game_is_picked() {
        let (mut loader, fake) = loader();
        let game = fetched_game();
        loader.prefetch(game);
        fake.last().finish(Err("offline".to_owned()));
        assert!(loader.poll().is_none());
        assert!(loader.play(game).is_none());
        assert_eq!(fake.count(), 2);
    }

    #[test]
    fn picking_another_game_drops_the_waiting_one() {
        let (mut loader, fake) = loader();
        let game = fetched_game();
        loader.play(game);
        let other = HOMEBREW_GAMES
            .iter()
            .find(|game| game.console == ShelfConsole::GameBoy)
            .unwrap();
        assert!(loader.play(other).is_some());
        assert!(loader.waiting().is_none());
        fake.last().finish(Ok(stored(game)));
        assert!(loader.poll().is_none());
        assert_eq!(loader.play(game), Some(raw(game)));
    }
}
