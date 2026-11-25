mod credit;
mod game_steps;
#[cfg(not(target_arch = "wasm32"))]
pub mod lan;
mod link;
mod phone_steps;
mod referee;
mod route;
mod store;
pub mod summary;
mod sync;

pub use credit::{Counter, Credit, Credited, StepSource, Taken};
pub use game_steps::GameSteps;
pub use link::WalkerLink;
pub use phone_steps::{PhoneSteps, Reading};
pub use referee::{Answer, Peer};
#[cfg(not(target_arch = "wasm32"))]
pub use route::Replaced;
pub use route::{Offer, Route};
pub use store::{FileStore, KeptNote, KeptWhy, MAIN, WalkerStore, decode, encode, is_walker_file};
pub use summary::{Home, StepsFrom, Summary, Trip};
pub use sync::Hold;

use crate::online::cloud::{WalkerReply, WalkerRequest};
use citrine_gb_walker::Walker;
use sync::{Effect, Sync};

const SAVE_EVERY_MS: u64 = 60_000;
const CREDIT_EVERY_SECS: u64 = 10;
const CATCH_UP_AFTER_MS: u64 = 10_000;
const HOLD_CLOCK_MS: u64 = 30_000;
const WALKER_GAMES: [[u8; 3]; 2] = [*b"IPK", *b"IPG"];

pub fn talks_to_walker(game_code: [u8; 4]) -> bool {
    WALKER_GAMES
        .iter()
        .any(|prefix| game_code.starts_with(prefix))
}

enum Held {
    Unloaded,
    Home(Box<Walker>),
    Lent,
}

struct Borrow {
    game: String,
    home: Home,
    steps_from: StepsFrom,
    back: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptCard {
    pub steps: u32,
    pub why: KeptWhy,
    pub can_use: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    Here,
    Borrowed {
        from: String,
    },
    Getting {
        from: String,
    },
    On {
        device: String,
    },
    Away {
        device: String,
    },
    Using {
        device: String,
        game: String,
        home: Option<String>,
    },
    GoingBack {
        to: String,
    },
    ComingBack {
        from: String,
    },
}

pub struct Pokewalker {
    store: Box<dyn WalkerStore>,
    held: Held,
    saved_at: Option<u64>,
    credited: Credited,
    credit_due: u64,
    stepped: bool,
    copy: Vec<u8>,
    walked: bool,
    sync: Sync,
    source: Option<Box<dyn StepSource>>,
    held_clock: Option<u64>,
    copied: bool,
    game_steps: bool,
    device: Option<Home>,
    borrow: Option<Borrow>,
    lan: route::LanState,
    peer: Option<Peer>,
    route: Route,
    cloud_listed: Option<Option<crate::online::cloud::WalkerRemote>>,
    leave_data: Option<Vec<u8>>,
    offers: Vec<Offer>,
    on_cloud: Option<citrine_gb_net::PeerKey>,
    #[cfg(not(target_arch = "wasm32"))]
    taking: Option<(route::LanState, Hold)>,
    cloud_on: bool,
    kept: Option<Option<(String, KeptNote)>>,
}

impl Default for Pokewalker {
    fn default() -> Self {
        Self::new(Box::new(FileStore::default()))
    }
}

impl Pokewalker {
    pub fn new(store: Box<dyn WalkerStore>) -> Self {
        Self {
            store,
            held: Held::Unloaded,
            saved_at: None,
            credited: Credited::default(),
            credit_due: 0,
            stepped: false,
            copy: Vec::new(),
            walked: false,
            sync: Sync::default(),
            source: None,
            held_clock: None,
            copied: false,
            game_steps: false,
            device: None,
            borrow: None,
            lan: route::LanState::default(),
            peer: None,
            route: Route::Local,
            cloud_listed: None,
            leave_data: None,
            offers: Vec::new(),
            on_cloud: None,
            #[cfg(not(target_arch = "wasm32"))]
            taking: None,
            cloud_on: false,
            kept: None,
        }
    }

    pub fn walker(&mut self) -> Option<&mut Walker> {
        self.load();
        match &mut self.held {
            Held::Home(walker) => Some(walker.as_mut()),
            Held::Unloaded | Held::Lent => None,
        }
    }

    pub fn press(&mut self, button: citrine_gb_walker::Button) {
        self.load();
        self.sync.claim();
        if !self.sync.may_change() {
            return;
        }
        if let Held::Home(walker) = &mut self.held {
            walker.press(button);
            self.persist_home();
        }
    }

    pub fn tick(&mut self, now_ms: u64) {
        let Held::Home(walker) = &mut self.held else {
            return;
        };
        let waits = self.source.is_some() && matches!(self.sync.hold(), Hold::Idle | Hold::Taking);
        let jump = walker
            .ticked_at()
            .is_some_and(|last| now_ms > last + CATCH_UP_AFTER_MS);
        if waits && jump {
            let since = *self.held_clock.get_or_insert(now_ms);
            if now_ms < since + HOLD_CLOCK_MS {
                return;
            }
        }
        self.held_clock = None;
        walker.tick(now_ms);
        if !self.sync.may_change() {
            return;
        }
        let saved = *self.saved_at.get_or_insert(now_ms);
        if now_ms.saturating_sub(saved) >= SAVE_EVERY_MS {
            self.persist_home();
            self.saved_at = Some(now_ms);
        }
    }

    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        self.store.files()
    }

    pub fn restore(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
        if name == MAIN
            && let Some(mine) = self.live_copy().or_else(|| {
                self.files()
                    .into_iter()
                    .find(|(file, _)| file == MAIN)
                    .map(|(_, mine)| mine)
            })
            && mine != bytes
        {
            self.keep_copy(&mine, KeptWhy::Imported);
        }
        self.store.put(name, bytes)?;
        if name == MAIN {
            self.store.set_last_stored(None);
            self.store.set_ticked_at(None);
            self.store.set_copied(false);
            self.sync.set_last_stored(None);
        }
        if name == MAIN && !self.is_lent() {
            self.held = Held::Unloaded;
        }
        Ok(())
    }

    pub fn forget(&mut self) -> std::io::Result<()> {
        self.store.clear()?;
        let store = std::mem::take(self).store;
        *self = Self::new(store);
        Ok(())
    }

    pub fn is_lent(&self) -> bool {
        matches!(self.held, Held::Lent)
    }

    pub fn hold(&self) -> &Hold {
        self.sync.hold()
    }

    pub fn read_only(&self) -> bool {
        !self.sync.may_change()
    }

    pub fn take(&mut self) {
        self.load();
        if self.route != Route::Away {
            self.sync.take();
        }
    }

    pub fn set_step_source(&mut self, source: Option<Box<dyn StepSource>>) {
        self.source = source;
    }

    pub fn set_game_steps(&mut self, on: bool) {
        self.game_steps = on;
    }

    pub fn steps_from(&self) -> StepsFrom {
        if self.source.is_some() {
            StepsFrom::Phone
        } else if self.game_steps {
            StepsFrom::Game
        } else {
            StepsFrom::Nothing
        }
    }

    pub fn summary(&self) -> Option<Summary> {
        match self.place() {
            Place::Here | Place::Borrowed { .. } => None,
            _ => self.listed_summary(),
        }
    }

    fn listed_summary(&self) -> Option<Summary> {
        let remote = self.sync.listed()?;
        let saved = crate::stats::local_date(remote.saved_at);
        Summary::from_label(&remote.name, saved, crate::stats::today())
    }

    pub fn set_device(&mut self, device: Option<Home>) {
        self.device = device;
    }

    pub fn borrow(&mut self, game: &str, home: Home) -> bool {
        self.load();
        let free = matches!(self.sync.hold(), Hold::Elsewhere(_) | Hold::Idle);
        if !free || self.borrow.is_some() || self.route == Route::Away {
            return false;
        }
        let steps_from = self
            .listed_summary()
            .map_or(StepsFrom::Nothing, |summary| summary.steps_from);
        self.borrow = Some(Borrow {
            game: game.to_owned(),
            home,
            steps_from,
            back: false,
        });
        self.sync.take();
        true
    }

    pub fn send_home(&mut self) {
        if let Some(borrow) = &mut self.borrow
            && !borrow.back
        {
            borrow.back = true;
            self.sync.release();
        }
    }

    pub fn cancel_borrow(&mut self) {
        if self.borrow.is_none() {
            return;
        }
        if self.sync.untake() {
            self.borrow = None;
        } else {
            self.send_home();
        }
    }

    pub fn place(&self) -> Place {
        if self.route == Route::Away
            && let Hold::Elsewhere(device) = self.sync.hold()
        {
            return Place::Away {
                device: device.clone(),
            };
        }
        let summary = self.listed_summary();
        let me = self.device.as_ref().map(|device| device.id);
        let home_is_me =
            |summary: &Summary| me.is_some() && summary.home.as_ref().map(|home| home.id) == me;
        let home_name = |summary: &Summary| {
            summary
                .home
                .as_ref()
                .map(|home| home.name.clone())
                .unwrap_or_default()
        };
        match (self.sync.hold(), &self.borrow) {
            (Hold::Here | Hold::Local, Some(borrow)) if !borrow.back => Place::Borrowed {
                from: borrow.home.name.clone(),
            },
            (Hold::Taking | Hold::Idle, Some(borrow)) if !borrow.back => Place::Getting {
                from: borrow.home.name.clone(),
            },
            (Hold::Here | Hold::Taking, Some(borrow)) => Place::GoingBack {
                to: borrow.home.name.clone(),
            },
            (Hold::Elsewhere(device), _) => match summary {
                Some(summary) => match &summary.trip {
                    Trip::Borrowed { game } => Place::Using {
                        device: device.clone(),
                        game: game.clone(),
                        home: (!home_is_me(&summary)).then(|| home_name(&summary)),
                    },
                    Trip::Returning if home_is_me(&summary) => Place::ComingBack {
                        from: device.clone(),
                    },
                    Trip::Returning => Place::GoingBack {
                        to: home_name(&summary),
                    },
                    Trip::Home => Place::On {
                        device: device.clone(),
                    },
                },
                None => Place::On {
                    device: device.clone(),
                },
            },
            _ => Place::Here,
        }
    }

    fn follow_trip(&mut self) {
        if let Some(borrow) = &self.borrow
            && borrow.back
            && !self.sync.releasing()
        {
            self.borrow = None;
        }
        let Some(summary) = self.listed_summary() else {
            return;
        };
        let me = self.device.as_ref().map(|device| device.id);
        let home = summary.home.filter(|_| me.is_some());
        match (self.sync.hold().clone(), summary.trip, home) {
            (Hold::Elsewhere(_), Trip::Returning, Some(home)) if Some(home.id) == me => {
                self.sync.take();
            }
            (Hold::Here, Trip::Borrowed { game }, Some(home))
                if Some(home.id) != me && self.borrow.is_none() =>
            {
                self.borrow = Some(Borrow {
                    game,
                    home,
                    steps_from: summary.steps_from,
                    back: true,
                });
                self.sync.release();
            }
            _ => {}
        }
    }

    pub fn credited(&self) -> Credited {
        self.credited
    }

    pub fn has_step_source(&self) -> bool {
        self.source.is_some()
    }

    pub fn walking_here(&mut self) -> bool {
        let walking = match &mut self.held {
            Held::Home(walker) => walker.info().has_pokemon(),
            Held::Lent => true,
            Held::Unloaded => false,
        };
        walking && self.sync.may_credit()
    }

    pub fn credit(&mut self, now: u64) -> Option<Credit> {
        self.source.as_ref()?;
        self.load();
        self.sync.claim();
        let Held::Home(walker) = &mut self.held else {
            return None;
        };
        let (credit, changed) = Self::credit_into(
            &self.sync,
            &mut self.credited,
            &mut self.credit_due,
            self.source.as_deref_mut()?,
            walker,
            now,
        );
        if changed {
            self.persist_home();
        }
        credit
    }

    pub(crate) fn credit_lent(&mut self, walker: &mut Walker, now: u64) -> Option<Credit> {
        let (credit, changed) = Self::credit_into(
            &self.sync,
            &mut self.credited,
            &mut self.credit_due,
            self.source.as_deref_mut()?,
            walker,
            now,
        );
        if changed {
            self.persist(walker);
        }
        credit
    }

    fn credit_into(
        sync: &Sync,
        credited: &mut Credited,
        due: &mut u64,
        source: &mut dyn StepSource,
        walker: &mut Walker,
        now: u64,
    ) -> (Option<Credit>, bool) {
        if !sync.may_credit() || now < *due {
            return (None, false);
        }
        *due = now + CREDIT_EVERY_SECS;
        let before = *credited;
        let credit = credited.next(source, now);
        let steps = credit.as_ref().map_or(0, |credit| credit.steps);
        for taken in credit.iter().flat_map(|credit| &credit.taken) {
            let at = taken.at.saturating_mul(1_000);
            if walker.ticked_at().is_some_and(|last| at > last) {
                walker.tick(at);
            }
            walker.steps(taken.steps);
        }
        let changed = steps > 0 || before.until.is_none() || before.counter != credited.counter;
        (credit, changed && *credited != before)
    }

    pub(crate) fn credit_game_steps(&mut self, walker: &mut Walker, steps: u32) -> bool {
        if steps == 0 {
            return true;
        }
        self.sync.claim();
        if !self.sync.may_credit() {
            return false;
        }
        walker.steps(steps);
        self.stepped = true;
        true
    }

    pub(crate) fn save_stepped(&mut self, walker: &Walker, now_ms: u64) {
        let saved = *self.saved_at.get_or_insert(now_ms);
        if self.stepped && now_ms.saturating_sub(saved) >= SAVE_EVERY_MS {
            self.save(walker);
            self.saved_at = Some(now_ms);
        }
    }

    pub(crate) fn lend(&mut self) -> Option<WalkerLink> {
        self.load();
        self.sync.claim();
        match std::mem::replace(&mut self.held, Held::Lent) {
            Held::Home(walker) => Some(WalkerLink::new(*walker)),
            held => {
                self.held = held;
                None
            }
        }
    }

    pub(crate) fn save(&mut self, walker: &Walker) {
        self.stepped = false;
        if self.sync.may_change() {
            self.persist(walker);
        } else {
            self.keep(walker);
        }
    }

    pub(crate) fn give_back(&mut self, link: WalkerLink) {
        let walker = link.into_walker();
        self.save(&walker);
        self.held = Held::Home(Box::new(walker));
    }

    fn keep(&mut self, walker: &Walker) {
        let bytes = encode(walker, self.credited);
        if bytes != self.copy {
            let holder = self.holder_name();
            self.keep_copy(&bytes, KeptWhy::ChangedHere { holder });
        }
    }

    fn holder_name(&self) -> Option<String> {
        match self.sync.hold() {
            Hold::Elsewhere(device) => Some(device.clone()),
            _ => self.sync.listed().map(|remote| remote.device.clone()),
        }
    }

    fn live_copy(&self) -> Option<Vec<u8>> {
        match &self.held {
            Held::Home(walker) => Some(encode(walker, self.credited)),
            _ => None,
        }
    }

    fn keep_copy(&mut self, bytes: &[u8], why: KeptWhy) {
        let Some(name) = self.store.keep(bytes) else {
            return;
        };
        let Some((walker, _)) = decode(bytes) else {
            return;
        };
        if walker.health().total_steps() == 0 && walker.shelves().is_empty() {
            return;
        }
        for (open, mut note) in self.store.notes() {
            if !note.closed {
                note.closed = true;
                self.store.set_note(&open, &note);
            }
        }
        let note = KeptNote {
            why,
            steps: walker.health().total_steps(),
            closed: false,
        };
        self.store.set_note(&name, &note);
        self.kept = Some(Some((name, note)));
    }

    fn kept(&mut self) -> Option<&(String, KeptNote)> {
        let store = &self.store;
        self.kept
            .get_or_insert_with(|| store.notes().into_iter().rfind(|(_, note)| !note.closed))
            .as_ref()
    }

    fn may_swap(&self) -> bool {
        !matches!(self.held, Held::Lent)
            && matches!(self.sync.hold(), Hold::Here | Hold::Local)
            && self.borrow.is_none()
    }

    pub fn kept_card(&mut self) -> Option<KeptCard> {
        let (_, note) = self.kept()?.clone();
        Some(KeptCard {
            steps: note.steps,
            why: note.why,
            can_use: self.may_swap(),
        })
    }

    pub fn close_kept(&mut self) {
        let Some((name, mut note)) = self.kept().cloned() else {
            return;
        };
        note.closed = true;
        self.store.set_note(&name, &note);
        self.kept = Some(None);
    }

    pub fn use_kept(&mut self) -> bool {
        self.load();
        if !self.may_swap() {
            return false;
        }
        let Some((name, _)) = self.kept().cloned() else {
            return false;
        };
        let Some((walker, credited)) = self
            .store
            .files()
            .into_iter()
            .find(|(file, _)| *file == name)
            .and_then(|(_, bytes)| decode(&bytes))
        else {
            return false;
        };
        let Held::Home(current) = &self.held else {
            return false;
        };
        let out = encode(current, self.credited);
        self.store.drop_kept(&name);
        self.kept = Some(None);
        self.credited = credited;
        self.persist(&walker);
        self.held = Held::Home(Box::new(walker));
        self.keep_copy(&out, KeptWhy::SwappedOut);
        true
    }

    #[cfg(test)]
    pub(crate) fn cloud_on(&mut self) {
        self.follow_route(true);
    }

    pub(crate) fn cloud_request(&mut self) -> Option<WalkerRequest> {
        if !self.route.uses_cloud() {
            return None;
        }
        let request = self.next_request()?;
        self.sent_cloud(&request);
        Some(request)
    }

    fn next_request(&mut self) -> Option<WalkerRequest> {
        self.load();
        let mut request = self.sync.request(&self.copy)?;
        if let WalkerRequest::Put { data, name, .. } = &mut request
            && let Some((mut walker, _)) = decode(data)
        {
            let mut label = self.label(&mut walker);
            self.leaving_label(&mut label);
            *name = label.to_label();
        }
        Some(request)
    }

    fn label(&self, walker: &mut Walker) -> Summary {
        let Some(borrow) = &self.borrow else {
            let mut summary = Summary::of(walker, self.steps_from());
            summary.home = self.device.clone();
            return summary;
        };
        let mut summary = Summary::of(walker, borrow.steps_from);
        summary.home = Some(borrow.home.clone());
        summary.trip = if borrow.back {
            Trip::Returning
        } else {
            Trip::Borrowed {
                game: borrow.game.clone(),
            }
        };
        summary
    }

    pub(crate) fn cloud_reply(&mut self, reply: WalkerReply) -> Option<Walker> {
        self.load();
        if !self.heard_cloud(&reply) {
            return None;
        }
        let stored = match &reply {
            WalkerReply::Stored(remote) => Some(remote.clone()),
            _ => None,
        };
        let walker = self.reply_any(reply);
        if let Some(remote) = stored {
            self.stored_cloud(&remote);
        }
        self.left_cloud_elsewhere();
        walker
    }

    fn reply_any(&mut self, reply: WalkerReply) -> Option<Walker> {
        self.load();
        let last_stored = self.sync.last_stored();
        let (saved_at, from) = match &reply {
            WalkerReply::Fetched { remote, .. } => {
                (Some(remote.saved_at), Some(remote.device.clone()))
            }
            _ => (None, None),
        };
        let effect = self.sync.reply(reply, &self.copy);
        if self.sync.last_stored() != last_stored {
            self.store.set_last_stored(self.sync.last_stored());
        }
        self.follow_trip();
        let Some(effect) = effect else {
            if saved_at.is_some() {
                self.mark_copied(true);
            }
            return None;
        };
        match effect {
            Effect::Adopt(data) => self.adopt(data, saved_at),
            Effect::KeepAndAdopt { kept, adopt } => {
                let kept = match self.live_copy() {
                    Some(live) if kept == self.copy => live,
                    _ => kept,
                };
                self.keep_copy(&kept, KeptWhy::ChangedHere { holder: from });
                self.adopt(adopt, saved_at)
            }
        }
    }

    fn adopt(&mut self, data: Vec<u8>, saved_at: Option<u64>) -> Option<Walker> {
        let Some((mut walker, credited)) = decode(&data) else {
            log::warn!("the cloud walker is not a walker this version reads");
            return None;
        };
        if let Some(saved_at) = saved_at.filter(|&at| at > 0) {
            let at = saved_at.min(crate::storage::now_unix());
            walker.resume_at(at.saturating_mul(1_000));
        }
        self.store.save(&data);
        self.mark_copied(true);
        self.copy = data;
        self.walked = walked(&mut walker);
        self.credited = credited;
        match &mut self.held {
            Held::Lent => Some(walker),
            held => {
                *held = Held::Home(Box::new(walker));
                None
            }
        }
    }

    fn mark_copied(&mut self, copied: bool) {
        if self.copied != copied {
            self.copied = copied;
            self.store.set_copied(copied);
        }
    }

    fn persist_home(&mut self) {
        if let Held::Home(walker) = &self.held {
            if self.copied {
                self.copied = false;
                self.store.set_copied(false);
            }
            self.copy = encode(walker, self.credited);
            self.walked = self.walked || walked(&mut Walker::clone(walker));
            self.store.save(&self.copy);
            self.store.set_ticked_at(walker.ticked_at());
            self.sync.changed();
        }
    }

    fn persist(&mut self, walker: &Walker) {
        self.mark_copied(false);
        self.copy = encode(walker, self.credited);
        self.walked = self.walked || walked(&mut Walker::clone(walker));
        self.store.save(&self.copy);
        self.store.set_ticked_at(walker.ticked_at());
        self.sync.changed();
    }

    fn load(&mut self) {
        if !matches!(self.held, Held::Unloaded) {
            return;
        }
        self.sync.set_last_stored(self.store.last_stored());
        self.load_lan();
        let mut walker = match self.store.load() {
            Some((mut walker, credited)) => {
                self.credited = credited;
                self.copied = self.store.copied();
                if !self.copied {
                    self.sync.changed();
                }
                if let Some(ms) = self.store.ticked_at() {
                    walker.resume_at(ms);
                }
                walker
            }
            None => Walker::new(seed()),
        };
        self.copy = encode(&walker, self.credited);
        self.walked = walked(&mut walker);
        self.held = Held::Home(Box::new(walker));
    }
}

fn walked(walker: &mut Walker) -> bool {
    walker.health().total_steps() > 0 || walker.info().has_pokemon()
}

fn seed() -> u64 {
    let mut bytes = [0; 8];
    if getrandom::getrandom(&mut bytes).is_err() {
        bytes = crate::storage::now_unix().to_le_bytes();
    }
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::online::cloud::WalkerRemote;
    use citrine_gb_walker::Button;
    use credit::tests::Pedometer;

    #[derive(Default)]
    pub(super) struct Memory {
        saved: Option<Vec<u8>>,
        saves: usize,
        pub(super) kept: Vec<Vec<u8>>,
        kept_names: Vec<String>,
        next_kept: u64,
        notes: std::collections::BTreeMap<String, KeptNote>,
        last_stored: Option<sync::LastStored>,
        ticked_at: Option<u64>,
        copied: bool,
        lan: Option<Vec<u8>>,
    }

    pub(super) type Shared = Rc<RefCell<Memory>>;

    impl WalkerStore for Shared {
        fn load(&mut self) -> Option<(Walker, Credited)> {
            decode(self.borrow().saved.as_ref()?)
        }

        fn save(&mut self, bytes: &[u8]) {
            let mut memory = self.borrow_mut();
            memory.saved = Some(bytes.to_vec());
            memory.saves += 1;
        }

        fn keep(&mut self, bytes: &[u8]) -> Option<String> {
            let mut memory = self.borrow_mut();
            let name = format!("walker-kept-{:04}.bin", memory.next_kept);
            memory.next_kept += 1;
            memory.kept.push(bytes.to_vec());
            memory.kept_names.push(name.clone());
            Some(name)
        }

        fn notes(&self) -> Vec<(String, KeptNote)> {
            let memory = self.borrow();
            memory
                .notes
                .iter()
                .map(|(name, note)| (name.clone(), note.clone()))
                .collect()
        }

        fn set_note(&mut self, name: &str, note: &KeptNote) {
            self.borrow_mut()
                .notes
                .insert(name.to_owned(), note.clone());
        }

        fn drop_kept(&mut self, name: &str) {
            let mut memory = self.borrow_mut();
            if let Some(at) = memory.kept_names.iter().position(|kept| kept == name) {
                memory.kept.remove(at);
                memory.kept_names.remove(at);
            }
            memory.notes.remove(name);
        }

        fn files(&self) -> Vec<(String, Vec<u8>)> {
            let memory = self.borrow();
            let main = memory
                .saved
                .iter()
                .map(|bytes| (MAIN.to_owned(), bytes.clone()));
            let kept = memory
                .kept_names
                .iter()
                .zip(&memory.kept)
                .map(|(name, bytes)| (name.clone(), bytes.clone()));
            main.chain(kept).collect()
        }

        fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            let mut memory = self.borrow_mut();
            if name == MAIN {
                memory.saved = Some(bytes.to_vec());
            } else {
                memory.kept.push(bytes.to_vec());
                memory.kept_names.push(name.to_owned());
            }
            Ok(())
        }

        fn clear(&mut self) -> std::io::Result<()> {
            *self.borrow_mut() = Memory::default();
            Ok(())
        }

        fn last_stored(&self) -> Option<sync::LastStored> {
            self.borrow().last_stored
        }

        fn set_last_stored(&mut self, last_stored: Option<sync::LastStored>) {
            self.borrow_mut().last_stored = last_stored;
        }

        fn ticked_at(&self) -> Option<u64> {
            self.borrow().ticked_at
        }

        fn set_ticked_at(&mut self, ms: Option<u64>) {
            self.borrow_mut().ticked_at = ms;
        }

        fn copied(&self) -> bool {
            self.borrow().copied
        }

        fn set_copied(&mut self, copied: bool) {
            self.borrow_mut().copied = copied;
        }

        fn lan(&self) -> Option<Vec<u8>> {
            self.borrow().lan.clone()
        }

        fn set_lan(&mut self, bytes: Option<&[u8]>) {
            self.borrow_mut().lan = bytes.map(<[u8]>::to_vec);
        }
    }

    #[derive(Default)]
    pub(super) struct Cloud {
        pub(super) version: u64,
        pub(super) device: String,
        pub(super) data: Vec<u8>,
        pub(super) name: String,
        saved_at: u64,
        pub(super) fetches: usize,
    }

    impl Cloud {
        fn remote(&self) -> WalkerRemote {
            WalkerRemote {
                version: self.version,
                device: self.device.clone(),
                hash: sync::tests::digest(&self.data),
                saved_at: self.saved_at,
                name: self.name.clone(),
            }
        }

        fn sync(&mut self, device: &str, pokewalker: &mut Pokewalker) {
            pokewalker.cloud_on();
            self.serve(device, pokewalker);
        }

        pub(super) fn serve(&mut self, device: &str, pokewalker: &mut Pokewalker) {
            let listed = (self.version > 0).then(|| self.remote());
            pokewalker.cloud_reply(WalkerReply::Listed(listed));
            while let Some(request) = pokewalker.cloud_request() {
                let reply = match request {
                    WalkerRequest::Fetch => {
                        self.fetches += 1;
                        WalkerReply::Fetched {
                            remote: self.remote(),
                            data: self.data.clone(),
                        }
                    }
                    WalkerRequest::Put { base, .. } if base != self.version => {
                        WalkerReply::Refused { changed: true }
                    }
                    WalkerRequest::Put { data, name, .. } => {
                        self.version += 1;
                        self.device = device.to_string();
                        self.data = data;
                        self.name = name;
                        self.saved_at = crate::storage::now_unix();
                        WalkerReply::Stored(self.remote())
                    }
                };
                pokewalker.cloud_reply(reply);
                pokewalker.cloud_reply(WalkerReply::Listed(Some(self.remote())));
            }
        }
    }

    fn steps_of(pokewalker: &mut Pokewalker) -> u32 {
        pokewalker.walker().unwrap().today_steps()
    }

    #[test]
    fn steps_taken_while_another_device_held_the_walker_are_credited_once() {
        let mut cloud = Cloud::default();
        let mut phone = pokewalker(&Shared::default());
        phone.set_step_source(Some(Box::new(Pedometer {
            taken: vec![(150, 300), (250, 40), (450, 1_000)],
            ..Pedometer::default()
        })));
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.credit(100), None);
        assert_eq!(phone.credit(200).map(|c| c.steps), Some(300));
        cloud.sync("Phone", &mut phone);
        let mut pc = pokewalker(&Shared::default());
        pc.cloud_on();
        pc.take();
        cloud.sync("PC", &mut pc);
        assert_eq!(pc.credit(300), None);
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Elsewhere("PC".into()));
        assert_eq!(phone.credit(400), None);
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        let credit = phone.credit(500).unwrap();
        assert_eq!(
            credit,
            Credit {
                from: 200,
                to: 500,
                steps: 1_040,
                taken: vec![
                    Taken { at: 250, steps: 40 },
                    Taken {
                        at: 450,
                        steps: 1_000
                    },
                ],
            }
        );
        assert_eq!(steps_of(&mut phone), 1_340);
        assert_eq!(phone.credited().until, Some(500));
    }

    #[test]
    fn a_device_without_a_step_counter_credits_nothing() {
        let mut pokewalker = pokewalker(&Shared::default());
        assert_eq!(pokewalker.credit(1_000), None);
        assert_eq!(pokewalker.credited(), Credited::default());
    }

    fn named(id: u64, name: &str) -> Home {
        Home {
            id,
            name: name.into(),
        }
    }

    fn trip_of(cloud: &Cloud) -> Trip {
        serde_json::from_str::<Summary>(&cloud.name).unwrap().trip
    }

    fn phone_with_walker(cloud: &mut Cloud) -> Pokewalker {
        let mut phone = pokewalker(&Shared::default());
        phone.set_device(Some(named(1, "Phone")));
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        phone.walker().unwrap().steps(77);
        phone.press(Button::Middle);
        cloud.sync("Phone", &mut phone);
        phone
    }

    fn pc_beside(memory: &Shared, cloud: &mut Cloud) -> Pokewalker {
        let mut pc = pokewalker(memory);
        pc.set_device(Some(named(2, "PC")));
        pc.cloud_on();
        cloud.sync("PC", &mut pc);
        pc
    }

    #[test]
    fn a_borrowed_walker_goes_home_by_itself_after_the_session() {
        let mut cloud = Cloud::default();
        let mut phone = phone_with_walker(&mut cloud);
        let mut pc = pc_beside(&Shared::default(), &mut cloud);
        assert_eq!(
            pc.place(),
            Place::On {
                device: "Phone".into()
            }
        );
        let home = pc.summary().unwrap().home.unwrap();
        assert_eq!(home, named(1, "Phone"));
        assert!(pc.borrow("HeartGold", home));
        assert_eq!(
            pc.place(),
            Place::Getting {
                from: "Phone".into()
            }
        );
        cloud.sync("PC", &mut pc);
        assert_eq!(
            pc.place(),
            Place::Borrowed {
                from: "Phone".into()
            }
        );
        assert_eq!(steps_of(&mut pc), 77);
        assert_eq!(
            trip_of(&cloud),
            Trip::Borrowed {
                game: "HeartGold".into()
            }
        );
        cloud.sync("Phone", &mut phone);
        assert_eq!(
            phone.place(),
            Place::Using {
                device: "PC".into(),
                game: "HeartGold".into(),
                home: None,
            }
        );
        pc.walker().unwrap().steps(5);
        pc.press(Button::Middle);
        pc.send_home();
        assert_eq!(pc.place(), Place::GoingBack { to: "Phone".into() });
        cloud.sync("PC", &mut pc);
        assert_eq!(trip_of(&cloud), Trip::Returning);
        assert_eq!(pc.place(), Place::GoingBack { to: "Phone".into() });
        assert!(pc.read_only());
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.place(), Place::Here);
        assert_eq!(phone.hold(), &Hold::Here);
        assert_eq!(
            (cloud.device.as_str(), trip_of(&cloud)),
            ("Phone", Trip::Home)
        );
        assert_eq!(steps_of(&mut phone), 82);
        cloud.sync("PC", &mut pc);
        assert_eq!(
            pc.place(),
            Place::On {
                device: "Phone".into()
            }
        );
    }

    #[test]
    fn a_borrower_restarted_mid_borrow_sends_the_walker_home() {
        let mut cloud = Cloud::default();
        let mut phone = phone_with_walker(&mut cloud);
        let memory = Shared::default();
        let mut pc = pc_beside(&memory, &mut cloud);
        pc.borrow("SoulSilver", named(1, "Phone"));
        cloud.sync("PC", &mut pc);
        drop(pc);
        let pc = pc_beside(&memory, &mut cloud);
        assert_eq!(trip_of(&cloud), Trip::Returning);
        assert_eq!(pc.place(), Place::GoingBack { to: "Phone".into() });
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Here);
    }

    #[test]
    fn a_borrow_cancelled_before_anything_was_sent_leaves_the_walker_where_it_is() {
        let mut cloud = Cloud::default();
        let _phone = phone_with_walker(&mut cloud);
        let mut pc = pc_beside(&Shared::default(), &mut cloud);
        let version = cloud.version;
        pc.borrow("HeartGold", named(1, "Phone"));
        pc.cancel_borrow();
        assert_eq!(
            pc.place(),
            Place::On {
                device: "Phone".into()
            }
        );
        cloud.sync("PC", &mut pc);
        assert_eq!((cloud.version, cloud.device.as_str()), (version, "Phone"));
    }

    #[test]
    fn a_device_that_is_not_the_home_only_watches_the_walker_go_back() {
        let mut cloud = Cloud::default();
        let _phone = phone_with_walker(&mut cloud);
        let mut pc = pc_beside(&Shared::default(), &mut cloud);
        let mut laptop = pokewalker(&Shared::default());
        laptop.set_device(Some(named(3, "Laptop")));
        laptop.cloud_on();
        pc.borrow("HeartGold", named(1, "Phone"));
        cloud.sync("PC", &mut pc);
        pc.send_home();
        cloud.sync("PC", &mut pc);
        cloud.sync("Laptop", &mut laptop);
        assert_eq!(laptop.place(), Place::GoingBack { to: "Phone".into() });
        assert_eq!(cloud.device, "PC");
    }

    #[test]
    fn the_previous_holder_shows_the_new_walker_and_ignores_presses() {
        let mut cloud = Cloud::default();
        let pc_memory = Shared::default();
        let mut pc = pokewalker(&pc_memory);
        pc.cloud_on();
        pc.take();
        cloud.sync("PC", &mut pc);
        let mut phone = pokewalker(&Shared::default());
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        phone.walker().unwrap().steps(77);
        phone.press(Button::Middle);
        cloud.sync("Phone", &mut phone);
        cloud.sync("PC", &mut pc);
        assert!(pc.read_only());
        assert_eq!(steps_of(&mut pc), 77);
        let saves = pc_memory.borrow().saves;
        pc.press(Button::Middle);
        pc.tick(0);
        pc.tick(SAVE_EVERY_MS * 2);
        assert_eq!(pc_memory.borrow().saves, saves);
        assert!(pc_memory.borrow().kept.is_empty());
        let saved = decode(pc_memory.borrow().saved.as_ref().unwrap())
            .unwrap()
            .0;
        assert_eq!(saved.today_steps(), 77);
    }

    #[test]
    fn the_other_device_shows_the_holders_summary_without_fetching() {
        let mut cloud = Cloud::default();
        let mut phone = pokewalker(&Shared::default());
        phone.set_step_source(Some(Box::new(Pedometer::default())));
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Idle);
        phone.credit(100);
        cloud.sync("Phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Here);
        phone.walker().unwrap().steps(77);
        phone.press(Button::Middle);
        cloud.sync("Phone", &mut phone);
        let pc_memory = Shared::default();
        let mut pc = pokewalker(&pc_memory);
        cloud.sync("PC", &mut pc);
        assert_eq!(pc.hold(), &Hold::Elsewhere("Phone".into()));
        assert_eq!(cloud.fetches, 0);
        assert!(pc_memory.borrow().saved.is_none());
        let summary = pc.summary().expect("a summary");
        assert_eq!(summary.steps_from, StepsFrom::Phone);
        assert_eq!(summary.week.last(), Some(&77));
        assert_eq!(summary.pokemon, None);
        assert_eq!(phone.summary(), None);
    }

    #[test]
    fn a_restarted_device_without_the_walker_keeps_nothing_aside() {
        let mut cloud = Cloud::default();
        let pc_memory = Shared::default();
        let mut pc = pokewalker(&pc_memory);
        pc.cloud_on();
        pc.take();
        cloud.sync("PC", &mut pc);
        let mut phone = pokewalker(&Shared::default());
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        cloud.sync("PC", &mut pc);
        assert_eq!(pc.hold(), &Hold::Elsewhere("Phone".into()));
        phone.walker().unwrap().steps(5);
        phone.press(Button::Middle);
        cloud.sync("Phone", &mut phone);
        let fetches = cloud.fetches;
        let mut pc = pokewalker(&pc_memory);
        cloud.sync("PC", &mut pc);
        assert_eq!(pc.hold(), &Hold::Elsewhere("Phone".into()));
        assert!(pc_memory.borrow().kept.is_empty());
        assert_eq!(cloud.fetches, fetches);
    }

    #[test]
    fn a_lent_walker_saved_after_a_take_is_kept_and_the_link_gets_the_new_one() {
        let mut cloud = Cloud::default();
        let pc_memory = Shared::default();
        let mut pc = pokewalker(&pc_memory);
        pc.cloud_on();
        pc.take();
        cloud.sync("PC", &mut pc);
        let mut link = pc.lend().unwrap();
        let mut phone = pokewalker(&Shared::default());
        phone.cloud_on();
        phone.take();
        cloud.sync("Phone", &mut phone);
        phone.walker().unwrap().steps(500);
        phone.press(Button::Middle);
        cloud.sync("Phone", &mut phone);
        link.walker_mut().steps(9);
        pc.save(link.walker());
        let request = pc.cloud_request().unwrap();
        assert!(matches!(request, WalkerRequest::Put { base: 1, .. }));
        pc.cloud_reply(WalkerReply::Refused { changed: true });
        pc.cloud_reply(WalkerReply::Listed(Some(cloud.remote())));
        assert_eq!(pc.cloud_request(), Some(WalkerRequest::Fetch));
        let fresh = pc
            .cloud_reply(WalkerReply::Fetched {
                remote: cloud.remote(),
                data: cloud.data.clone(),
            })
            .expect("the link gets the new walker");
        assert_eq!(fresh.today_steps(), 500);
        assert_eq!(pc_memory.borrow().kept.len(), 1);
        assert_eq!(
            decode(&pc_memory.borrow().kept[0]).unwrap().0.today_steps(),
            9
        );
        assert!(pc.read_only());
        let changed_here = KeptWhy::ChangedHere {
            holder: Some("Phone".into()),
        };
        assert_eq!(
            pc.kept_card(),
            Some(KeptCard {
                steps: 9,
                why: changed_here.clone(),
                can_use: false,
            })
        );
        link.walker_mut().steps(4);
        pc.save(link.walker());
        assert_eq!(pc_memory.borrow().kept.len(), 2);
        assert_eq!(
            pc.kept_card(),
            Some(KeptCard {
                steps: 13,
                why: changed_here,
                can_use: false,
            })
        );
    }

    fn walker_with(steps: u32) -> Vec<u8> {
        let mut walker = Walker::new(5);
        walker.steps(steps);
        encode(&walker, Credited::default())
    }

    #[test]
    fn use_it_swaps_the_kept_copy_in_and_a_second_use_it_swaps_back() {
        let memory = Shared::default();
        let mut pc = pokewalker(&memory);
        pc.walker().unwrap().steps(300);
        pc.press(Button::Middle);
        let mine = memory.borrow().saved.clone().unwrap();
        let imported = walker_with(1_032);
        pc.restore(MAIN, &imported).unwrap();
        assert_eq!(steps_of(&mut pc), 1_032);
        assert_eq!(
            pc.kept_card(),
            Some(KeptCard {
                steps: 300,
                why: KeptWhy::Imported,
                can_use: true,
            })
        );

        assert!(pc.use_kept());
        assert_eq!(steps_of(&mut pc), 300);
        assert_eq!(memory.borrow().saved.as_ref(), Some(&mine));
        assert_eq!(memory.borrow().kept, vec![imported.clone()]);
        assert_eq!(
            pc.kept_card(),
            Some(KeptCard {
                steps: 1_032,
                why: KeptWhy::SwappedOut,
                can_use: true,
            })
        );

        assert!(pc.use_kept());
        assert_eq!(steps_of(&mut pc), 1_032);
        assert_eq!(memory.borrow().saved.as_ref(), Some(&imported));
        assert_eq!(memory.borrow().kept, vec![mine]);
        assert_eq!(
            pc.kept_card(),
            Some(KeptCard {
                steps: 300,
                why: KeptWhy::SwappedOut,
                can_use: true,
            })
        );
    }

    #[test]
    fn an_unused_walker_kept_aside_shows_no_card() {
        let memory = Shared::default();
        let mut pc = pokewalker(&memory);
        pc.press(Button::Middle);
        pc.restore(MAIN, &walker_with(50)).unwrap();
        assert_eq!(memory.borrow().kept.len(), 1);
        assert_eq!(pc.kept_card(), None);
        assert!(!pc.use_kept());
    }

    #[test]
    fn a_newer_kept_copy_takes_the_card_and_close_hides_it_for_good() {
        let memory = Shared::default();
        let mut pc = pokewalker(&memory);
        pc.walker().unwrap().steps(300);
        pc.press(Button::Middle);
        pc.restore(MAIN, &walker_with(40)).unwrap();
        assert_eq!(pc.kept_card().map(|kept| kept.steps), Some(300));
        pc.restore(MAIN, &walker_with(41)).unwrap();
        assert_eq!(pc.kept_card().map(|kept| kept.steps), Some(40));
        pc.close_kept();
        assert_eq!(pc.kept_card(), None);
        assert_eq!(memory.borrow().kept.len(), 2);
        assert_eq!(pokewalker(&memory).kept_card(), None);
    }

    #[test]
    fn a_game_using_the_walker_hides_use_it_but_keeps_the_card() {
        let memory = Shared::default();
        let mut pc = pokewalker(&memory);
        pc.walker().unwrap().steps(300);
        pc.press(Button::Middle);
        pc.restore(MAIN, &walker_with(40)).unwrap();
        let link = pc.lend().unwrap();
        assert_eq!(pc.kept_card().map(|kept| kept.can_use), Some(false));
        assert!(!pc.use_kept());
        pc.give_back(link);
        assert_eq!(pc.kept_card().map(|kept| kept.can_use), Some(true));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_kept_card_and_its_reason_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let open = || Pokewalker::new(Box::new(FileStore::at(dir.path().to_path_buf())));
        let mut pc = open();
        pc.walker().unwrap().steps(300);
        pc.press(Button::Middle);
        pc.restore(MAIN, &walker_with(1_032)).unwrap();
        let card = pc.kept_card();
        assert_eq!(
            card.as_ref().map(|kept| &kept.why),
            Some(&KeptWhy::Imported)
        );
        let mut again = open();
        assert_eq!(again.kept_card(), card);
        assert!(again.use_kept());
        let mut third = open();
        assert_eq!(steps_of(&mut third), 300);
        assert_eq!(
            third.kept_card().map(|kept| (kept.steps, kept.why)),
            Some((1_032, KeptWhy::SwappedOut))
        );
        assert!(third.files().iter().all(|(name, _)| is_walker_file(name)));
        third.close_kept();
        assert_eq!(open().kept_card(), None);
    }

    pub(super) fn pokewalker(memory: &Shared) -> Pokewalker {
        Pokewalker::new(Box::new(Rc::clone(memory)))
    }

    #[test]
    fn only_heartgold_and_soulsilver_talk_to_the_walker() {
        for code in [*b"IPKE", *b"IPGJ", *b"IPKD", *b"IPGS"] {
            assert!(talks_to_walker(code));
        }
        for code in [*b"IRBO", *b"IRAO", *b"IMWE", *b"CPUE", *b"APAE"] {
            assert!(!talks_to_walker(code));
        }
    }

    #[test]
    fn a_new_profile_gets_a_fresh_walker_and_keeps_it() {
        let memory = Shared::default();
        let mut first = pokewalker(&memory);
        first.walker().unwrap().steps(40);
        let link = first.lend().unwrap();
        first.give_back(link);
        assert_eq!(pokewalker(&memory).walker().unwrap().health().watts(), 2);
    }

    #[test]
    fn a_lent_walker_is_lent_once_and_saved_when_it_comes_back() {
        let memory = Shared::default();
        let mut pokewalker = pokewalker(&memory);
        let link = pokewalker.lend().unwrap();
        assert!(pokewalker.is_lent());
        assert!(pokewalker.lend().is_none());
        assert!(pokewalker.walker().is_none());
        assert_eq!(memory.borrow().saves, 0);
        pokewalker.give_back(link);
        assert_eq!(memory.borrow().saves, 1);
        assert!(pokewalker.walker().is_some());
    }

    const MONDAY: u64 = 1_793_664_000;

    fn phone_sensor(
        name: &str,
        reading: &Rc<std::cell::Cell<Option<Reading>>>,
    ) -> Box<dyn StepSource> {
        let dir = crate::storage::scratch_dir()
            .join(format!("citrine-gaps-{}-{name}", std::process::id()));
        let reading = Rc::clone(reading);
        Box::new(PhoneSteps::new(
            Box::new(move || reading.get()),
            crate::storage::Folders::at(dir),
        ))
    }

    fn counted(count: u64) -> Option<Reading> {
        Some(Reading { count, boot: 1_000 })
    }

    #[test]
    fn a_restarted_phone_credits_while_the_app_is_open() {
        let mut cloud = Cloud::default();
        let memory = Shared::default();
        let taken = vec![(MONDAY + 150, 300)];
        let mut phone = pokewalker(&memory);
        phone.set_step_source(Some(Box::new(Pedometer {
            taken: taken.clone(),
            ..Pedometer::default()
        })));
        phone.cloud_on();
        phone.take();
        cloud.sync("Android phone", &mut phone);
        assert_eq!(phone.credit(MONDAY + 100), None);
        cloud.sync("Android phone", &mut phone);

        let mut phone = pokewalker(&memory);
        phone.set_step_source(Some(Box::new(Pedometer {
            taken,
            ..Pedometer::default()
        })));
        cloud.sync("Android phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Here);
        assert_eq!(phone.credit(MONDAY + 200).map(|c| c.steps), Some(300));
    }

    #[test]
    fn steps_credited_just_before_the_app_closes_stay() {
        let mut cloud = Cloud::default();
        let memory = Shared::default();
        let sensor = Rc::new(std::cell::Cell::new(counted(1_000)));
        let mut phone = pokewalker(&memory);
        phone.set_step_source(Some(phone_sensor("closes", &sensor)));
        phone.cloud_on();
        phone.take();
        cloud.sync("Android phone", &mut phone);
        assert_eq!(phone.credit(MONDAY + 100), None);
        assert_eq!(phone.credit(MONDAY + 110).map(|c| c.steps), Some(0));
        cloud.sync("Android phone", &mut phone);
        sensor.set(counted(3_000));
        assert_eq!(phone.credit(MONDAY + 200).map(|c| c.steps), Some(2_000));

        let mut phone = pokewalker(&memory);
        phone.set_step_source(Some(phone_sensor("closes", &sensor)));
        cloud.sync("Android phone", &mut phone);
        phone.take();
        cloud.sync("Android phone", &mut phone);
        let _ = phone.credit(MONDAY + 300);
        assert_eq!(steps_of(&mut phone), 2_000);
    }

    const DAY: u64 = 86_400;

    fn pedometer(taken: &[(u64, u32)]) -> Option<Box<dyn StepSource>> {
        Some(Box::new(Pedometer {
            taken: taken.to_vec(),
            ..Pedometer::default()
        }))
    }

    #[test]
    fn steps_land_on_the_days_they_were_walked() {
        let memory = Shared::default();
        let taken = [
            (MONDAY + DAY / 2, 3_000),
            (MONDAY + DAY + DAY / 2, 4_000),
            (MONDAY + 2 * DAY + DAY / 2, 5_000),
        ];
        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&taken));
        phone.walker();
        phone.tick(MONDAY * 1_000);
        assert_eq!(phone.credit(MONDAY + 3_600), None);
        phone.tick((MONDAY + 3_600) * 1_000);

        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&taken));
        let now = MONDAY + 2 * DAY + 20 * 3_600;
        assert_eq!(phone.credit(now).map(|c| c.steps), Some(12_000));
        phone.tick(now * 1_000);
        let walker = phone.walker().unwrap();
        assert_eq!(walker.history()[..2], [4_000, 3_000]);
        assert_eq!(walker.today_steps(), 5_000);
    }

    #[test]
    fn a_restarted_phone_holds_the_walker_clock_until_the_cloud_answers() {
        let mut cloud = Cloud::default();
        let memory = Shared::default();
        let taken = [(MONDAY + DAY / 2, 3_000)];
        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&taken));
        phone.cloud_on();
        phone.take();
        cloud.sync("Android phone", &mut phone);
        phone.tick(MONDAY * 1_000);
        assert_eq!(phone.credit(MONDAY + 60), None);
        phone.tick((MONDAY + 120) * 1_000);
        cloud.sync("Android phone", &mut phone);

        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&taken));
        phone.cloud_on();
        let now = MONDAY + DAY + 3_600;
        phone.tick(now * 1_000);
        let held = phone.walker().unwrap().ticked_at();
        assert_eq!(held, Some((MONDAY + 120) * 1_000));
        cloud.sync("Android phone", &mut phone);
        assert_eq!(phone.hold(), &Hold::Here);
        assert_eq!(phone.credit(now).map(|c| c.steps), Some(3_000));
        phone.tick(now * 1_000);
        let walker = phone.walker().unwrap();
        assert_eq!((walker.history()[0], walker.today_steps()), (3_000, 0));
    }

    #[test]
    fn the_walker_clock_runs_on_when_the_cloud_never_answers() {
        let memory = Shared::default();
        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&[]));
        phone.walker();
        phone.tick(MONDAY * 1_000);
        phone.tick((MONDAY + 120) * 1_000);

        let mut phone = pokewalker(&memory);
        phone.set_step_source(pedometer(&[]));
        phone.cloud_on();
        let now = (MONDAY + DAY) * 1_000;
        phone.tick(now);
        phone.tick(now + HOLD_CLOCK_MS);
        assert_eq!(
            phone.walker().unwrap().ticked_at(),
            Some(now + HOLD_CLOCK_MS)
        );
    }
}
