use crate::online::Online;
use crate::walker::{Hold, Home, KeptCard, Place, StepsFrom, Summary, talks_to_walker};
use citrine_gb::machine::Machine;
use citrine_gb_walker::{App, Button, Event, Link, Screen, Walker};

const OPEN_PRESSES: usize = 16;
const BORROW_WAIT_SECS: u64 = 30;

fn game_name(game_code: [u8; 4]) -> &'static str {
    if game_code.starts_with(b"IPG") {
        "SoulSilver"
    } else {
        "HeartGold"
    }
}

impl super::Emulator {
    pub fn attach_ir(&mut self, game_code: [u8; 4]) -> bool {
        if !talks_to_walker(game_code) {
            return false;
        }
        if self.seat.core().walker.is_some() {
            return true;
        }
        let Some(link) = self.pokewalker.lend() else {
            return false;
        };
        self.seat.core_mut().walker = Some(link);
        self.walker_linked = true;
        self.walker_game = Some(game_name(game_code));
        true
    }

    pub fn walker_game(&self) -> Option<&'static str> {
        self.walker_game
    }

    pub fn walker_place(&self) -> Place {
        self.pokewalker.place()
    }

    pub fn borrow_walker(&mut self, online: &Online) {
        let Some(game) = self.walker_game else {
            return;
        };
        let Hold::Elsewhere(holder) = self.pokewalker.hold().clone() else {
            return;
        };
        let home = self
            .pokewalker
            .summary()
            .and_then(|summary| summary.home)
            .or_else(|| {
                let (devices, _) = online.devices();
                devices
                    .iter()
                    .find(|device| device.name == holder)
                    .map(|device| Home {
                        id: device.id,
                        name: device.name.clone(),
                    })
            });
        match home {
            Some(home) => {
                self.pokewalker.borrow(game, home);
            }
            None => self.pokewalker.take(),
        }
        self.walker_connect = Some((self.walker_clock)() + BORROW_WAIT_SECS);
    }

    pub fn open_walker(&mut self, app: App) {
        for _ in 0..OPEN_PRESSES {
            let Some(walker) = self.walker_mut() else {
                return;
            };
            if walker.asleep() {
                self.press_walker(Button::Middle);
                continue;
            }
            let press = match (walker.screen(), app) {
                (Screen::Collected { .. }, _) => Button::Middle,
                (Screen::Home, App::Connect) => Button::Middle,
                (Screen::Home, _) => Button::Right,
                (Screen::Menu { app: shown, .. }, wanted) if shown == wanted => {
                    self.press_walker(Button::Middle);
                    return;
                }
                (Screen::Menu { .. }, _) => Button::Right,
                _ => return,
            };
            self.press_walker(press);
        }
    }

    fn follow_borrow(&mut self) {
        let Some(deadline) = self.walker_connect else {
            return;
        };
        let held = matches!(self.pokewalker.hold(), Hold::Here | Hold::Local);
        let idle = self
            .seat
            .core()
            .walker
            .as_ref()
            .is_some_and(|link| link.walker().link() == Link::Idle);
        if held && idle {
            self.walker_connect = None;
            self.open_walker(App::Connect);
        } else if (self.walker_clock)() >= deadline {
            self.walker_connect = None;
            self.pokewalker.cancel_borrow();
        }
    }

    pub fn take_walker_linked(&mut self) -> bool {
        std::mem::take(&mut self.walker_linked)
    }

    pub fn walker_mut(&mut self) -> Option<&mut Walker> {
        if self.seat.core().walker.is_some() {
            return self
                .seat
                .core_mut()
                .walker
                .as_mut()
                .map(|link| link.walker_mut());
        }
        self.pokewalker.walker()
    }

    pub fn shown_walker(&mut self) -> Option<&mut Walker> {
        if !self.seat.facts().walker {
            return self.pokewalker.walker();
        }
        self.walker_watched = true;
        if self.seat.is_home() || self.walker_copy.is_none() {
            return self.walker_mut();
        }
        self.walker_copy.as_mut()
    }

    pub(super) fn copy_walker(&mut self) {
        self.walker_copy = std::mem::take(&mut self.walker_watched)
            .then(|| self.seat.core().walker.as_ref())
            .flatten()
            .map(|link| link.walker().clone());
    }

    pub fn walker_summary(&self) -> Option<Summary> {
        self.pokewalker.summary()
    }

    pub fn walker_kept(&mut self) -> Option<KeptCard> {
        self.pokewalker.kept_card()
    }

    pub fn use_kept_walker(&mut self) {
        self.pokewalker.use_kept();
    }

    pub fn close_kept_walker(&mut self) {
        self.pokewalker.close_kept();
    }

    pub fn walker_steps_from(&self) -> StepsFrom {
        self.pokewalker.steps_from()
    }

    pub fn take_walker(&mut self) {
        self.pokewalker.take();
    }

    pub fn sync_walker(&mut self, online: &mut Online, on: bool) {
        if on {
            let (_, this) = online.devices();
            let device = this.map(|id| Home {
                id,
                name: online.device_name(),
            });
            self.pokewalker.set_device(device);
        }
        let route = self.pokewalker.follow_route(on);
        let active = on || route.uses_cloud();
        online.walker_sync(active);
        if active {
            for reply in online.walker_replies() {
                if let Some(walker) = self.pokewalker.cloud_reply(reply) {
                    self.replace_lent_walker(walker);
                }
            }
            if online.walker_ready()
                && let Some(request) = self.pokewalker.cloud_request()
            {
                online.walker_send(request);
            }
        }
        if let Some(walker) = self.pokewalker.settle_here() {
            self.replace_lent_walker(walker);
        }
    }

    pub(crate) fn replace_lent_walker(&mut self, walker: Walker) {
        if self.seat.facts().walker
            && let Some(link) = self.seat.core_mut().walker.as_mut()
            && link.walker().link() == Link::Idle
        {
            *link.walker_mut() = walker;
        }
    }

    pub fn press_walker(&mut self, button: Button) {
        if self.pokewalker.read_only() {
            return;
        }
        if let Some(link) = self.seat.core_mut().walker.as_mut() {
            link.walker_mut().press(button);
            let walker = link.walker();
            if walker.screen() == (Screen::Connect { started: false })
                && walker.link() == Link::Idle
            {
                link.request_connect();
            }
            return;
        }
        self.pokewalker.press(button);
    }

    pub fn tick_walker(&mut self, now_ms: u64) {
        if self.seat.facts().walker {
            self.walker_ms = Some(now_ms);
        } else {
            if let Some(credit) = self.pokewalker.credit((self.walker_clock)()) {
                log::info!("walker: {} steps credited", credit.steps);
            }
            self.pokewalker.tick(now_ms);
        }
    }

    pub fn connect_walker(&mut self) {
        if let Some(link) = self.seat.core_mut().walker.as_mut() {
            link.request_connect();
        }
    }

    pub fn set_steps_in_game(&mut self, on: bool) {
        if on != self.steps_in_game {
            self.game_steps.forget();
        }
        self.steps_in_game = on;
        self.pokewalker.set_game_steps(on);
    }

    pub fn detach_ir(&mut self) {
        if let Some(mut link) = self.seat.core_mut().walker.take() {
            let steps = self.game_steps.take();
            self.pokewalker.credit_game_steps(link.walker_mut(), steps);
            self.pokewalker.give_back(link);
        }
        self.walker_game = None;
        if self.walker_connect.take().is_some() {
            self.pokewalker.cancel_borrow();
        }
        self.pokewalker.send_home();
        self.game_steps.forget();
    }

    fn count_game_steps(&mut self) {
        let core = self.seat.core();
        if !self.steps_in_game || core.walker.is_none() {
            self.game_steps.forget();
            return;
        }
        let counter = match &core.machine {
            Machine::Nds(nds) => citrine_gb_pkm::info::steps(nds.buses().main_ram()),
            _ => None,
        };
        self.game_steps.read(counter);
    }

    pub(crate) fn follow_walker(&mut self) {
        let now_ms = self.walker_ms.take();
        self.count_game_steps();
        self.follow_borrow();
        let Some(link) = self.seat.core_mut().walker.as_mut() else {
            return;
        };
        if let Some(now_ms) = now_ms {
            link.walker_mut().tick(now_ms);
        }
        if link.walker().link() == Link::Idle {
            let walker = link.walker_mut();
            if self
                .pokewalker
                .credit_game_steps(walker, self.game_steps.owed())
            {
                self.game_steps.take();
            }
            self.pokewalker.credit_lent(walker, (self.walker_clock)());
            if let Some(now_ms) = now_ms {
                self.pokewalker.save_stepped(link.walker(), now_ms);
            }
        }
        let events = link.take_events();
        for event in &events {
            log::debug!("walker: {event:?}");
        }
        if events.iter().any(|event| matches!(event, Event::Connected)) {
            self.pokewalker.take();
        }
        if events.iter().any(|event| matches!(event, Event::Ended(_))) {
            self.pokewalker.save(link.walker());
            self.pokewalker.send_home();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Emulator;
    use crate::storage::SaveStore;
    use crate::walker::{Counter, FileStore, Pokewalker, StepSource, Taken, decode};
    use citrine_gb::console::ConsoleKind;
    use citrine_gb::gba::wait::Width;
    use citrine_gb::machine::Machine;
    use citrine_gb::nds::card::AUX_CONTROL;
    use citrine_gb::nds::machine::Nds;
    use citrine_gb::rom::Rom;
    use citrine_gb_walker::{
        Block, Button, CYCLES_PER_SECOND, Eeprom, FLAG_HAS_POKEMON, FLAG_INIT, Link,
        PokemonSummary, Repair, Screen, Walker,
    };
    use std::path::{Path, PathBuf};

    const HEARTGOLD: [u8; 4] = *b"IPKE";
    const MASTER_ID: [u8; 4] = [0x3C, 0x5A, 0x96, 0xF0];
    const ARM9_RAM: u32 = 0x0200_0000;
    const ARM7_RAM: u32 = 0x0380_0000;
    const BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;
    const SPI_HOLD: u32 = 0xA040;
    const SPI_LAST: u32 = 0xA000;
    const SPI_OFF: u32 = 0x8000;
    const SPI_BUSY: u32 = 0x80;
    const SPI_BYTE_CYCLES: u32 = 64;
    const ROUTE_STAGING: usize = 0xD700;
    const ROUTE_SIZE: usize = 0x2900;
    const TEAM_STAGING: usize = 0xD480;
    const TEAM_SIZE: usize = 0x280;
    const TID: u16 = 31_337;
    const SID: u16 = 0x1234;
    const SPECIES: u16 = 25;
    const LEVEL: u8 = 15;

    fn root(name: &str) -> PathBuf {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-walker-link-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn app(root: &Path) -> Emulator {
        Emulator {
            store: SaveStore::at(root.join("saves")),
            pokewalker: Pokewalker::new(Box::new(FileStore::at(root.to_path_buf()))),
            ..Emulator::default()
        }
    }

    fn saved(root: &Path) -> Walker {
        decode(&std::fs::read(root.join("walker").join("walker.bin")).expect("walker file"))
            .expect("readable walker file")
            .0
    }

    fn assert_sound(eeprom: &Eeprom) {
        let mut eeprom = eeprom.clone();
        for block in [Block::Info, Block::Health] {
            assert_eq!(eeprom.repair(block), Repair::Intact, "{block:?}");
        }
    }

    fn checksum(plain: &[u8]) -> u16 {
        let mut sum: u32 = 2;
        for pair in plain.chunks(2) {
            sum += (u32::from(pair[0]) << 8) | u32::from(*pair.get(1).unwrap_or(&0));
        }
        while sum > 0xFFFF {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        sum as u16
    }

    fn encode(cmd: u8, extra: u8, session: [u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut plain = vec![cmd, extra, 0, 0];
        plain.extend_from_slice(&session);
        plain.extend_from_slice(payload);
        let sum = checksum(&plain);
        plain[2..4].copy_from_slice(&sum.to_le_bytes());
        plain.into_iter().map(|b| b ^ 0xAA).collect()
    }

    fn decode_reply(wire: &[u8]) -> (u8, [u8; 4], Vec<u8>) {
        let mut plain: Vec<u8> = wire.iter().map(|b| b ^ 0xAA).collect();
        let stored = u16::from_le_bytes([plain[2], plain[3]]);
        plain[2] = 0;
        plain[3] = 0;
        assert_eq!(checksum(&plain), stored, "reply checksum");
        (
            plain[0],
            [plain[4], plain[5], plain[6], plain[7]],
            plain[8..].to_vec(),
        )
    }

    struct Game {
        session: [u8; 4],
    }

    fn ds_rom() -> Rom {
        let mut rom = vec![0; 0x1000];
        rom[..4].copy_from_slice(b"TEST");
        rom[0x0C..0x10].copy_from_slice(&HEARTGOLD);
        for (at, offset, ram) in [(0x20, 0x200u32, ARM9_RAM), (0x30, 0x800, ARM7_RAM)] {
            for (index, value) in [offset, ram, ram, 4].into_iter().enumerate() {
                rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
            }
            rom[offset as usize..offset as usize + 4]
                .copy_from_slice(&BRANCH_TO_SELF.to_le_bytes());
        }
        Rom::from(rom)
    }

    fn load_heartgold(emulator: &mut Emulator) {
        emulator
            .load_game(&ds_rom(), ConsoleKind::Nds, None)
            .unwrap();
        assert!(emulator.seat.core().walker.is_some());
    }

    fn nds(emulator: &mut Emulator) -> &mut Nds {
        match &mut emulator.seat.core_mut().machine {
            Machine::Nds(nds) => nds,
            _ => panic!("not a DS game"),
        }
    }

    fn spi_byte(emulator: &mut Emulator, byte: u8, last: bool) -> u8 {
        let cart = nds(emulator).buses_mut().cart_mut();
        let control = if last { SPI_LAST } else { SPI_HOLD };
        cart.write(AUX_CONTROL, control, 0xFFFF);
        cart.write(AUX_CONTROL, u32::from(byte) << 16, 0xFF << 16);
        while cart.read(AUX_CONTROL) & SPI_BUSY != 0 {
            cart.step(SPI_BYTE_CYCLES);
        }
        (cart.read(AUX_CONTROL) >> 16) as u8
    }

    fn release(emulator: &mut Emulator) {
        let cart = nds(emulator).buses_mut().cart_mut();
        cart.write(AUX_CONTROL, SPI_OFF, 0xFFFF);
    }

    impl Game {
        fn new() -> Self {
            Self { session: [0; 4] }
        }

        fn send(&mut self, emulator: &mut Emulator, wire: &[u8]) {
            let bytes = [&[0x02][..], wire].concat();
            for (i, &byte) in bytes.iter().enumerate() {
                spi_byte(emulator, byte, i + 1 == bytes.len());
            }
        }

        fn receive(&mut self, emulator: &mut Emulator) -> Vec<u8> {
            spi_byte(emulator, 0x01, false);
            let len = usize::from(spi_byte(emulator, 0, false));
            let data = (0..len)
                .map(|i| spi_byte(emulator, 0, i + 1 == len))
                .collect();
            if len == 0 {
                release(emulator);
            }
            data
        }

        fn wait(&mut self, emulator: &mut Emulator, limit: u64) -> Option<Vec<u8>> {
            let until = nds(emulator).cycles() + limit;
            while nds(emulator).cycles() < until {
                emulator
                    .seat
                    .core_mut()
                    .run_plain_frame(&mut citrine_gb_threads::Split::inline());
                let data = self.receive(emulator);
                if !data.is_empty() {
                    return Some(data);
                }
            }
            None
        }

        fn handshake(&mut self, emulator: &mut Emulator) {
            emulator.connect_walker();
            let advert = self
                .wait(emulator, CYCLES_PER_SECOND)
                .expect("advertisement");
            assert_eq!(advert, vec![0x56]);
            self.send(emulator, &encode(0xFA, 0x01, MASTER_ID, &[]));
            let ack = self
                .wait(emulator, CYCLES_PER_SECOND / 10)
                .expect("slave ack");
            let (cmd, session, _) = decode_reply(&ack);
            assert_eq!(cmd, 0xF8);
            self.session = std::array::from_fn(|i| session[i] ^ MASTER_ID[i]);
        }

        fn request(&mut self, emulator: &mut Emulator, cmd: u8, payload: &[u8]) -> Option<u8> {
            self.ask(emulator, cmd, 0x01, payload)
                .map(|(answer, _)| answer)
        }

        fn ask(
            &mut self,
            emulator: &mut Emulator,
            cmd: u8,
            extra: u8,
            payload: &[u8],
        ) -> Option<(u8, Vec<u8>)> {
            self.send(emulator, &encode(cmd, extra, self.session, payload));
            let wire = self.wait(emulator, CYCLES_PER_SECOND / 10)?;
            let (answer, session, payload) = decode_reply(&wire);
            assert_eq!(session, self.session);
            Some((answer, payload))
        }

        fn write(&mut self, emulator: &mut Emulator, address: usize, data: &[u8]) {
            for (i, block) in data.chunks(128).enumerate() {
                let at = address + i * 128;
                let low = (at & 0x80) as u8 | 0x02;
                let answer = self.ask(emulator, low, (at >> 8) as u8, block);
                assert_eq!(answer.map(|(cmd, _)| cmd), Some(0x04), "write at {at:#06x}");
            }
        }

        fn read(&mut self, emulator: &mut Emulator, address: u16, len: u8) -> Vec<u8> {
            let [high, low] = address.to_be_bytes();
            let (answer, data) = self
                .ask(emulator, 0x0C, 0x01, &[high, low, len])
                .expect("read answer");
            assert_eq!(answer, 0x0E);
            data
        }
    }

    fn trainer_info() -> Vec<u8> {
        let mut info = vec![0; 0x68];
        info[0x00..0x04].copy_from_slice(&1u32.to_le_bytes());
        info[0x0C..0x0E].copy_from_slice(&TID.to_le_bytes());
        info[0x0E..0x10].copy_from_slice(&SID.to_le_bytes());
        info[0x5C] = 0x02;
        info[0x5E] = 0x01;
        info
    }

    fn summary() -> [u8; 16] {
        PokemonSummary {
            species: SPECIES,
            held_item: 0,
            moves: [84, 39, 98, 0],
            level: LEVEL,
            flags: [0, 0],
        }
        .to_bytes()
    }

    fn route_upload() -> Vec<u8> {
        let mut route: Vec<u8> = (0..ROUTE_SIZE).map(|i| (i * 13) as u8).collect();
        route[..16].copy_from_slice(&summary());
        route
    }

    fn start_sending(game: &mut Game, emulator: &mut Emulator) {
        game.handshake(emulator);
        assert_eq!(game.request(emulator, 0x20, &[]), Some(0x22));
        assert_eq!(game.request(emulator, 0x32, &trainer_info()), Some(0x34));
    }

    fn send_pokemon(game: &mut Game, emulator: &mut Emulator) {
        start_sending(game, emulator);
        game.write(emulator, ROUTE_STAGING, &route_upload());
        game.write(emulator, TEAM_STAGING, &vec![0x3C; TEAM_SIZE]);
        assert_eq!(game.request(emulator, 0x38, &[]), Some(0x38));
        assert_eq!(game.request(emulator, 0x5A, &[]), Some(0x5A));
    }

    fn return_pokemon(game: &mut Game, emulator: &mut Emulator) -> (Vec<u8>, Vec<u8>) {
        game.handshake(emulator);
        let (answer, info) = game.ask(emulator, 0x20, 0x01, &[]).expect("identity");
        assert_eq!(answer, 0x22);
        let watts = game.read(emulator, 0xCE8A, 2);
        assert_eq!(game.read(emulator, 0x8F00, 16), summary());
        assert_eq!(game.request(emulator, 0x4E, &[]), Some(0x50));
        assert_eq!(game.request(emulator, 0xF4, &[]), None);
        (info, watts)
    }

    #[test]
    fn heartgold_sends_a_pokemon_and_gets_it_back_after_a_restart() {
        let root = root("round-trip");
        let mut emulator = app(&root);
        assert!(!emulator.attach_ir(*b"CPUE"));
        load_heartgold(&mut emulator);
        let mut game = Game::new();
        send_pokemon(&mut game, &mut emulator);
        emulator.follow_walker();

        let mut sent = saved(&root);
        assert_eq!(sent.info().flags() & FLAG_HAS_POKEMON, FLAG_HAS_POKEMON);
        assert_eq!(sent.info().tid(), TID);
        assert_eq!(sent.eeprom().read(0x8F00, 16), summary());
        assert_sound(sent.eeprom());
        drop(emulator);

        let mut emulator = app(&root);
        emulator.pokewalker.walker().expect("home").steps(2_345);
        load_heartgold(&mut emulator);
        let mut game = Game::new();
        let (info, watts) = return_pokemon(&mut game, &mut emulator);
        assert_eq!(info[0x64..0x68], 2_345u32.to_be_bytes());
        assert_eq!(watts, 117u16.to_be_bytes());
        emulator.follow_walker();

        let mut back = saved(&root);
        let live = emulator.seat.core().walker.as_ref().unwrap().walker();
        assert_eq!(back.eeprom(), live.eeprom());
        assert_eq!(back.rng_state(), live.rng_state());
        assert_eq!(
            back.info().flags() & (FLAG_INIT | FLAG_HAS_POKEMON),
            FLAG_INIT
        );
        assert_eq!(back.walking_pokemon(), None);
        assert_eq!(
            (back.health().total_steps(), back.health().watts()),
            (2_345, 0)
        );
        assert_sound(back.eeprom());

        emulator.unload();
        assert!(!emulator.pokewalker.is_lent());
        assert_eq!(saved(&root).eeprom(), back.eeprom());
    }

    #[test]
    fn unloading_mid_session_keeps_the_walker_sound_and_usable() {
        let root = root("mid-session");
        let mut emulator = app(&root);
        load_heartgold(&mut emulator);
        let mut game = Game::new();
        start_sending(&mut game, &mut emulator);
        game.write(&mut emulator, ROUTE_STAGING, &route_upload()[..0x1000]);
        emulator.unload();

        assert!(emulator.seat.core().walker.is_none());
        let mut kept = saved(&root);
        assert_sound(kept.eeprom());
        assert_eq!(kept.info().flags() & FLAG_HAS_POKEMON, 0);
        let walker = emulator.pokewalker.walker().expect("home again");
        assert_eq!(walker.link(), Link::Idle);

        let mut emulator = app(&root);
        load_heartgold(&mut emulator);
        let mut game = Game::new();
        send_pokemon(&mut game, &mut emulator);
        emulator.follow_walker();
        let mut sent = saved(&root);
        assert_eq!(sent.walking_pokemon().map(|p| p.species), Some(SPECIES));
        assert_sound(sent.eeprom());
    }

    #[cfg(threaded)]
    #[test]
    fn ticking_the_walker_leaves_a_lent_console_on_its_thread() {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let mut emulator = Emulator::default();
        emulator.run_on(&threads);
        let plan = super::super::runner::Plan {
            frames: 1,
            held: emulator.held.held(),
            owed: emulator.held.take_owed(),
            effects: emulator.effects(),
            sound: emulator.sound,
            fast_forward: false,
            take: false,
            rewind: crate::emulator::rewind::RewindPlan::OFF,
            budget: None,
            defer_3d: false,
            pace: 1.0,
        };
        emulator.seat.lend(plan);
        emulator.tick_walker(0);
        assert!(!emulator.seat.is_home());
        assert!(emulator.seat.wait(std::time::Duration::from_secs(5)));
    }

    #[test]
    fn pressing_connect_on_the_walker_advertises_to_the_game() {
        let root = root("press-connect");
        let mut emulator = app(&root);
        load_heartgold(&mut emulator);
        if emulator.walker_mut().expect("walker").asleep() {
            emulator.press_walker(Button::Middle);
        }
        emulator.press_walker(Button::Middle);
        assert_eq!(
            emulator.walker_mut().expect("walker").screen(),
            Screen::Connect { started: false }
        );
        let mut game = Game::new();
        let advert = game
            .wait(&mut emulator, CYCLES_PER_SECOND)
            .expect("advertisement");
        assert_eq!(advert, vec![0x56]);
    }

    #[test]
    fn an_unanswered_connect_goes_home_while_the_game_holds_the_walker() {
        let root = root("unanswered-connect");
        let mut emulator = app(&root);
        load_heartgold(&mut emulator);
        if emulator.walker_mut().expect("walker").asleep() {
            emulator.press_walker(Button::Middle);
        }
        emulator.press_walker(Button::Middle);
        let mut game = Game::new();
        while game.wait(&mut emulator, CYCLES_PER_SECOND).is_some() {}
        assert_eq!(emulator.walker_mut().expect("walker").link(), Link::Idle);
        emulator.tick_walker(0);
        emulator.follow_walker();
        emulator.tick_walker(1_000);
        emulator.follow_walker();
        assert_eq!(
            emulator.walker_mut().expect("walker").screen(),
            Screen::Home
        );
    }

    const GAME_CODE: u32 = 0x027F_FE0C;
    const HEARTGOLD_SAVE_POINTER: u32 = 0x021D_2228;
    const GAME_SAVE: u32 = 0x0230_0000;
    const RECORDS: u32 = GAME_SAVE + 0x10 + 0x4B3C;

    thread_local! {
        static NOW: std::cell::Cell<u64> = const { std::cell::Cell::new(1_000) };
    }

    fn clock() -> u64 {
        NOW.with(|now| now.get())
    }

    fn at(unix: u64) {
        NOW.with(|now| now.set(unix));
    }

    struct Phone(std::rc::Rc<std::cell::Cell<u32>>);

    impl StepSource for Phone {
        fn counter(&self) -> Counter {
            Counter::StepSensor
        }

        fn steps(&mut self, _from: u64, to: u64) -> Option<Vec<Taken>> {
            Some(vec![Taken {
                at: to,
                steps: self.0.take(),
            }])
        }
    }

    fn put(emulator: &mut Emulator, address: u32, value: u32) {
        nds(emulator)
            .buses_mut()
            .arm9()
            .write(address, Width::Word, value);
    }

    fn on_the_road(root: &Path, in_game: bool) -> Emulator {
        let mut emulator = app(root);
        emulator.walker_clock = clock;
        emulator.set_steps_in_game(in_game);
        load_heartgold(&mut emulator);
        put(&mut emulator, GAME_CODE, u32::from_le_bytes(HEARTGOLD));
        put(&mut emulator, HEARTGOLD_SAVE_POINTER, GAME_SAVE);
        emulator
    }

    fn walk_to(emulator: &mut Emulator, walked: u32) {
        put(emulator, RECORDS, walked);
        emulator
            .seat
            .core_mut()
            .run_plain_frame(&mut citrine_gb_threads::Split::inline());
        emulator.follow_walker();
    }

    fn today(emulator: &mut Emulator) -> u32 {
        emulator.walker_mut().expect("walker").today_steps()
    }

    #[test]
    fn a_walk_in_heartgold_counts_each_step_once() {
        let root = root("in-game");
        let mut emulator = on_the_road(&root, true);
        walk_to(&mut emulator, 1_000);
        assert_eq!(today(&mut emulator), 0);
        for walked in [1_001, 1_002, 1_002, 1_005] {
            walk_to(&mut emulator, walked);
        }
        assert_eq!(today(&mut emulator), 5);
        put(&mut emulator, RECORDS + 4, 3);
        walk_to(&mut emulator, 1_005);
        assert_eq!(today(&mut emulator), 8);
        walk_to(&mut emulator, 990);
        walk_to(&mut emulator, 992);
        assert_eq!(today(&mut emulator), 10);
        emulator.unload();
        assert_eq!(saved(&root).today_steps(), 10);
    }

    #[test]
    fn in_game_steps_count_only_while_turned_on() {
        let root = root("in-game-off");
        let mut emulator = on_the_road(&root, false);
        walk_to(&mut emulator, 1_000);
        walk_to(&mut emulator, 1_040);
        assert_eq!(today(&mut emulator), 0);
        emulator.set_steps_in_game(true);
        walk_to(&mut emulator, 1_050);
        assert_eq!(today(&mut emulator), 0);
        walk_to(&mut emulator, 1_052);
        assert_eq!(today(&mut emulator), 2);
        emulator.set_steps_in_game(false);
        walk_to(&mut emulator, 1_060);
        assert_eq!(today(&mut emulator), 2);
    }

    #[test]
    fn phone_and_game_steps_each_count_once() {
        let root = root("both");
        let phone = std::rc::Rc::new(std::cell::Cell::new(0));
        at(1_000);
        let mut emulator = on_the_road(&root, true);
        emulator
            .pokewalker
            .set_step_source(Some(Box::new(Phone(phone.clone()))));
        walk_to(&mut emulator, 500);
        phone.set(300);
        walk_to(&mut emulator, 510);
        assert_eq!(today(&mut emulator), 10);
        at(1_020);
        walk_to(&mut emulator, 512);
        assert_eq!(today(&mut emulator), 312);
        for _ in 0..3 {
            walk_to(&mut emulator, 512);
        }
        at(1_040);
        walk_to(&mut emulator, 512);
        assert_eq!(today(&mut emulator), 312);
        emulator.unload();
        assert_eq!(emulator.pokewalker.credited().until, Some(1_040));
        at(1_060);
        phone.set(25);
        emulator.tick_walker(0);
        assert_eq!(today(&mut emulator), 337);
    }

    #[test]
    fn credited_steps_stop_at_the_day_and_watt_caps() {
        let root = root("caps");
        let phone = std::rc::Rc::new(std::cell::Cell::new(0));
        at(2_000);
        let mut emulator = on_the_road(&root, true);
        emulator
            .pokewalker
            .set_step_source(Some(Box::new(Phone(phone.clone()))));
        walk_to(&mut emulator, 7_000);
        at(2_020);
        phone.set(250_000);
        walk_to(&mut emulator, 7_064);
        let walker = emulator.walker_mut().expect("walker");
        assert_eq!(walker.today_steps(), 99_999);
        assert_eq!(walker.health().watts(), 9_999);
    }
}
