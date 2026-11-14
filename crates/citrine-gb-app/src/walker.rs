mod credit;
mod game_steps;
mod link;
mod phone_steps;
mod store;
mod sync;

pub use credit::{Counter, Credit, Credited, StepSource};
pub use game_steps::GameSteps;
pub use link::WalkerLink;
pub use phone_steps::{PhoneSteps, Reading};
pub use store::{FileStore, MAIN, WalkerStore, decode, encode, is_walker_file};
pub use sync::Hold;

use crate::online::cloud::{WalkerReply, WalkerRequest};
use citrine_gb_walker::Walker;
use sync::{Effect, Sync};

const SAVE_EVERY_MS: u64 = 60_000;
const CREDIT_EVERY_SECS: u64 = 10;
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

pub struct Pokewalker {
    store: Box<dyn WalkerStore>,
    held: Held,
    saved_at: Option<u64>,
    credited: Credited,
    credit_due: u64,
    stepped: bool,
    copy: Vec<u8>,
    sync: Sync,
    source: Option<Box<dyn StepSource>>,
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
            sync: Sync::default(),
            source: None,
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
            && let Some((_, mine)) = self.files().into_iter().find(|(file, _)| file == MAIN)
            && mine != bytes
        {
            self.store.keep(&mine);
        }
        self.store.put(name, bytes)?;
        if name == MAIN {
            self.store.set_last_stored(None);
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
        self.sync.take();
    }

    pub fn set_step_source(&mut self, source: Option<Box<dyn StepSource>>) {
        self.source = source;
    }

    pub fn credited(&self) -> Credited {
        self.credited
    }

    pub fn has_step_source(&self) -> bool {
        self.source.is_some()
    }

    pub fn credit(&mut self, now: u64) -> Option<Credit> {
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
        let steps = credit.map_or(0, |credit| credit.steps);
        if steps > 0 {
            walker.steps(steps);
        }
        let changed = steps > 0 || before.until.is_none() || before.counter != credited.counter;
        (credit, changed && *credited != before)
    }

    pub(crate) fn credit_game_steps(&mut self, walker: &mut Walker, steps: u32) -> bool {
        if steps == 0 {
            return true;
        }
        if self.sync.hold() == &Hold::Idle {
            self.sync.take();
        }
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
            self.store.keep(&bytes);
        }
    }

    pub(crate) fn cloud_off(&mut self) {
        self.sync.off();
    }

    pub(crate) fn cloud_on(&mut self) {
        self.load();
        self.sync.on();
    }

    pub(crate) fn cloud_request(&mut self) -> Option<WalkerRequest> {
        self.load();
        self.sync.request(&self.copy)
    }

    pub(crate) fn cloud_reply(&mut self, reply: WalkerReply) -> Option<Walker> {
        self.load();
        let last_stored = self.sync.last_stored();
        let effect = self.sync.reply(reply, &self.copy);
        if self.sync.last_stored() != last_stored {
            self.store.set_last_stored(self.sync.last_stored());
        }
        match effect? {
            Effect::Adopt(data) => self.adopt(data),
            Effect::KeepAndAdopt { kept, adopt } => {
                self.store.keep(&kept);
                self.adopt(adopt)
            }
        }
    }

    fn adopt(&mut self, data: Vec<u8>) -> Option<Walker> {
        let Some((walker, credited)) = decode(&data) else {
            log::warn!("the cloud walker is not a walker this version reads");
            return None;
        };
        self.store.save(&data);
        self.copy = data;
        self.credited = credited;
        match &mut self.held {
            Held::Lent => Some(walker),
            held => {
                *held = Held::Home(Box::new(walker));
                None
            }
        }
    }

    fn persist_home(&mut self) {
        if let Held::Home(walker) = &self.held {
            self.copy = encode(walker, self.credited);
            self.store.save(&self.copy);
            self.sync.changed();
        }
    }

    fn persist(&mut self, walker: &Walker) {
        self.copy = encode(walker, self.credited);
        self.store.save(&self.copy);
        self.sync.changed();
    }

    fn load(&mut self) {
        if !matches!(self.held, Held::Unloaded) {
            return;
        }
        self.sync.set_last_stored(self.store.last_stored());
        let walker = match self.store.load() {
            Some((walker, credited)) => {
                self.credited = credited;
                self.sync.changed();
                walker
            }
            None => Walker::new(seed()),
        };
        self.copy = encode(&walker, self.credited);
        self.held = Held::Home(Box::new(walker));
    }
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
    struct Memory {
        saved: Option<Vec<u8>>,
        saves: usize,
        kept: Vec<Vec<u8>>,
        last_stored: Option<sync::LastStored>,
    }

    type Shared = Rc<RefCell<Memory>>;

    impl WalkerStore for Shared {
        fn load(&mut self) -> Option<(Walker, Credited)> {
            decode(self.borrow().saved.as_ref()?)
        }

        fn save(&mut self, bytes: &[u8]) {
            let mut memory = self.borrow_mut();
            memory.saved = Some(bytes.to_vec());
            memory.saves += 1;
        }

        fn keep(&mut self, bytes: &[u8]) {
            self.borrow_mut().kept.push(bytes.to_vec());
        }

        fn files(&self) -> Vec<(String, Vec<u8>)> {
            let memory = self.borrow();
            let main = memory
                .saved
                .iter()
                .map(|bytes| (MAIN.to_owned(), bytes.clone()));
            let kept = memory
                .kept
                .iter()
                .enumerate()
                .map(|(n, bytes)| (format!("walker-kept-{n}.bin"), bytes.clone()));
            main.chain(kept).collect()
        }

        fn put(&mut self, name: &str, bytes: &[u8]) -> std::io::Result<()> {
            let mut memory = self.borrow_mut();
            if name == MAIN {
                memory.saved = Some(bytes.to_vec());
            } else {
                memory.kept.push(bytes.to_vec());
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
    }

    #[derive(Default)]
    struct Cloud {
        version: u64,
        device: String,
        data: Vec<u8>,
    }

    impl Cloud {
        fn remote(&self) -> WalkerRemote {
            WalkerRemote {
                version: self.version,
                device: self.device.clone(),
                hash: sync::tests::digest(&self.data),
            }
        }

        fn sync(&mut self, device: &str, pokewalker: &mut Pokewalker) {
            pokewalker.cloud_on();
            let listed = (self.version > 0).then(|| self.remote());
            pokewalker.cloud_reply(WalkerReply::Listed(listed));
            while let Some(request) = pokewalker.cloud_request() {
                let reply = match request {
                    WalkerRequest::Fetch => WalkerReply::Fetched {
                        remote: self.remote(),
                        data: self.data.clone(),
                    },
                    WalkerRequest::Put { base, .. } if base != self.version => {
                        WalkerReply::Refused { changed: true }
                    }
                    WalkerRequest::Put { data, .. } => {
                        self.version += 1;
                        self.device = device.to_string();
                        self.data = data;
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
                steps: 1_040
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
    }

    fn pokewalker(memory: &Shared) -> Pokewalker {
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
        let dir = std::env::temp_dir().join(format!("citrine-gaps-{}-{name}", std::process::id()));
        let reading = Rc::clone(reading);
        Box::new(PhoneSteps::new(
            Box::new(move || reading.get()),
            crate::storage::Backend::at(dir),
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
}
