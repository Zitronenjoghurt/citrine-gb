use citrine_gb_test_roms as test_roms;
use citrine_gb_walker::{self as hle, CYCLES_PER_SECOND, airtime};
use citrine_walker_lab::harness::{self, MASTER_SESSION};
use citrine_walker_lab::ir::{CMD_ASSERT_MASTER, CMD_SLAVE_ACK, Packet};
use citrine_walker_lab::walker::{Button as Key, EEPROM_SIZE, Walker as Real};

const TOTAL_STEPS: u16 = 0xF780;
const GENERATOR: u16 = 0xF7C0;
const BUSH: u16 = 0xF7D3;
const DOWSING_OPEN: u16 = 0x47A2;
const RADAR_OPEN: u16 = 0x9D92;
const SCREEN: u16 = 0xF7B1;
const MENU_CURSOR: u16 = 0xF7AA;
const RADAR_CURSOR: u16 = 0xF7D0;
const RADAR_SHOWN: u16 = 0xF7D5;
const BATTLE_SCREEN: u8 = 4;
const BATTLE_STATE: u16 = 0xF7CF;
const APPEARED: u8 = 1;
const CHOOSING: u8 = 2;
const SOUND: u16 = 0xF7C4;
const HOME_ROLL: u16 = 0x6000;
const CLOCK_SET: u16 = 0xA5D2;
const LINK_START: u16 = 0x0888;
const DAY: u32 = 86_400;
const SYNC: u32 = 3_000 * DAY + 5 * 3_600 + 57 * 60;
const ROLLOVER_HOUR: u8 = 6;
const ROUTE_STAGING: usize = 0xD700;
const EVENT_LOG: usize = 0xCF0C;
const LOG_ENTRY: usize = 0x88;
const LOG_ENTRIES: usize = 10;
const ROUTE_SIZE: usize = 0x2900;
const TEAM_STAGING: usize = 0xD480;
const TEAM_SIZE: usize = 0x280;
const RADAR_BUBBLES: [usize; 4] = [0x1D70, 0x1DB0, 0x1DF0, 0x1E30];
const RADAR_BUSH: usize = 0x1CB0;

fn rom() -> Option<Vec<u8>> {
    let path = test_roms::env_path("walker conformance", "CITRINE_WALKER_ROM")?;
    test_roms::read("walker conformance", path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Reply {
    cmd: u8,
    extra: u8,
    payload: Vec<u8>,
}

impl From<Packet> for Reply {
    fn from(p: Packet) -> Self {
        Self {
            cmd: p.cmd,
            extra: p.extra,
            payload: p.payload,
        }
    }
}

type Request = (u8, u8, Vec<u8>);

struct Pair {
    real: Real,
    ours: hle::Walker,
    ms: u64,
}

impl Pair {
    fn boot(rom: Vec<u8>) -> Self {
        let mut real = Real::new(rom, vec![0xFF; EEPROM_SIZE]);
        real.run_seconds(10.0).unwrap();
        let mut ours = hle::Walker::new(0);
        ours.tick(0);
        let mut pair = Self { real, ours, ms: 0 };
        pair.catch_up();
        pair
    }

    fn boot_from(rom: Vec<u8>, eeprom: Vec<u8>) -> Self {
        let mut real = Real::new(rom, eeprom.clone());
        real.run_seconds(10.0).unwrap();
        let eeprom = hle::Eeprom::from_bytes(&eeprom).unwrap();
        let mut ours = hle::Walker::from_eeprom(eeprom, 0);
        ours.tick(0);
        let mut pair = Self { real, ours, ms: 0 };
        pair.catch_up();
        pair
    }

    fn real_ms(&self) -> u64 {
        self.real.now() * 1000 / citrine_walker_lab::walker::CLOCK_HZ
    }

    fn catch_up(&mut self) {
        let counted = self.total_steps();
        let ours = self.ours.health().total_steps();
        if counted > ours {
            self.ours.steps(counted - ours);
        }
        self.ms = self.real_ms();
        self.ours.tick(self.ms);
    }

    fn wait(&mut self, seconds: f64) {
        self.real.run_seconds(seconds).unwrap();
        self.catch_up();
    }

    fn cycles(&self) -> u64 {
        self.ms * CYCLES_PER_SECOND / 1000
    }

    fn generator(&mut self) -> u32 {
        u32::from_be_bytes([0, 1, 2, 3].map(|i| self.real.read(GENERATOR + i)))
    }

    fn clocks(&mut self, what: &str) {
        let real = u32::from_be_bytes([0, 1, 2, 3].map(|i| self.real.read(0xF788 + i)));
        eprintln!(
            "{what}: real clock {real} ours {} at {} ms",
            self.ours.clock(),
            self.ms
        );
    }

    fn total_steps(&mut self) -> u32 {
        u32::from_be_bytes([0, 1, 2, 3].map(|i| self.real.read(TOTAL_STEPS + i)))
    }

    fn walk(&mut self, steps: u32) {
        self.real.walk(steps, 2.0);
        self.real.run_seconds(steps as f64 / 2.0 + 10.0).unwrap();
        self.catch_up();
    }

    fn press(&mut self, key: Key, button: hle::Button) {
        self.real.press(key).unwrap();
        self.ours.press(button);
        self.catch_up();
    }

    fn wake(&mut self) {
        let start = self.real.now();
        self.real.set_button(Key::Enter, true);
        let rolled = self.real.run_until(HOME_ROLL, 1.0).unwrap();
        let state = rolled.then(|| self.generator());
        let held = (self.real.now() - start) as f64 / citrine_walker_lab::walker::CLOCK_HZ as f64;
        self.real.run_seconds((0.8 - held).max(0.0)).unwrap();
        self.real.set_button(Key::Enter, false);
        if let Some(state) = state {
            self.ours.set_rng_state(state as u64);
        }
        self.ours.press(hle::Button::Middle);
        self.real.run_seconds(0.4).unwrap();
        self.catch_up();
        if self.ours.home_event().is_some() {
            eprintln!("home event offered: {:?}", self.ours.home_event());
            self.press(Key::Enter, hle::Button::Middle);
            while matches!(self.ours.screen(), hle::Screen::Collected { .. }) {
                self.press(Key::Enter, hle::Button::Middle);
            }
        }
    }

    fn open(&mut self, at: u16) {
        let start = self.real.now();
        self.real.set_button(Key::Enter, true);
        let before = (
            self.real.read(SCREEN),
            self.real.read(MENU_CURSOR),
            self.ours.screen(),
        );
        let opened = self.real.run_until(at, 1.0).unwrap();
        assert!(
            opened,
            "the app did not open: before {before:?}, after {:?}",
            (
                self.real.read(SCREEN),
                self.real.read(MENU_CURSOR),
                self.ours.screen()
            )
        );
        let state = self.generator();
        self.ours.set_rng_state(state as u64);
        self.ours.press(hle::Button::Middle);
        let held = (self.real.now() - start) as f64 / citrine_walker_lab::walker::CLOCK_HZ as f64;
        self.real.run_seconds((0.2 - held).max(0.0)).unwrap();
        self.real.set_button(Key::Enter, false);
        self.real.run_seconds(0.4).unwrap();
        self.catch_up();
    }

    fn menu_to(&mut self, app: hle::App) {
        let target = app.index() as u8;
        if self.ours.screen() == hle::Screen::Home && self.real.read(SCREEN) == 0 {
            self.press(Key::Right, hle::Button::Right);
        }
        for _ in 0..8 {
            let real = self.real.read(MENU_CURSOR);
            let hle::Screen::Menu { app: ours, .. } = self.ours.screen() else {
                panic!("ours is not in the menu: {:?}", self.ours.screen());
            };
            let ours = ours.index() as u8;
            if real == target && ours == target {
                return;
            }
            match real.cmp(&target) {
                std::cmp::Ordering::Less => self.tap(Key::Right),
                std::cmp::Ordering::Greater => self.tap(Key::Left),
                std::cmp::Ordering::Equal => {}
            }
            match ours.cmp(&target) {
                std::cmp::Ordering::Less => self.ours.press(hle::Button::Right),
                std::cmp::Ordering::Greater => self.ours.press(hle::Button::Left),
                std::cmp::Ordering::Equal => {}
            }
            self.real.run_seconds(0.2).unwrap();
            self.catch_up();
        }
        panic!("the menus did not reach {app:?}");
    }

    fn until_real(&mut self, done: impl Fn(&mut Real) -> bool) {
        for _ in 0..100 {
            let quiet = self.real.read(SOUND) == 0 && self.real.read(SOUND + 1) == 0;
            if quiet && done(&mut self.real) {
                return;
            }
            self.wait(0.1);
        }
        panic!(
            "the real walker never got there: state {}",
            self.real.read(BATTLE_STATE)
        );
    }

    fn until_ours(&mut self, done: impl Fn(&hle::Battle) -> bool) {
        for _ in 0..50 {
            let hle::Screen::Battle(battle) = self.ours.screen() else {
                panic!("ours is not in a battle: {:?}", self.ours.screen());
            };
            if done(&battle) {
                return;
            }
            self.ours.press(hle::Button::Middle);
            self.wait(0.1);
        }
        panic!("ours never got there: {:?}", self.ours.screen());
    }

    fn tap(&mut self, key: Key) {
        self.real.set_button(key, true);
        self.real.run_seconds(0.1).unwrap();
        self.real.set_button(key, false);
        self.real.run_seconds(0.1).unwrap();
    }

    fn connect(&mut self) {
        self.press(Key::Enter, hle::Button::Middle);
        self.start_link();
    }

    fn start_link(&mut self) {
        let start = self.real.now();
        self.real.set_button(Key::Enter, true);
        assert!(
            self.real.run_until(LINK_START, 1.0).unwrap(),
            "no link start"
        );
        let own = self.generator();
        self.ms = self.real_ms();
        self.ours.tick(self.ms);
        self.ours.set_rng_state(own as u64);
        self.ours.press(hle::Button::Middle);
        self.ours.connect(self.cycles());
        let held = (self.real.now() - start) as f64 / citrine_walker_lab::walker::CLOCK_HZ as f64;
        self.real.run_seconds((0.2 - held).max(0.0)).unwrap();
        self.real.set_button(Key::Enter, false);
    }

    fn session(&mut self, requests: &[Request]) -> (Vec<Option<Reply>>, Vec<Option<Reply>>) {
        let clock = citrine_walker_lab::walker::CLOCK_HZ;
        let w = &mut self.real;
        w.ir_take();
        harness::wait_for_advert(w, 10.0).unwrap();
        w.run_cycles(clock / 1000).unwrap();
        let ack = harness::exchange(
            w,
            &Packet::new(CMD_ASSERT_MASTER, 0x01, MASTER_SESSION, &[]),
        )
        .unwrap();
        assert_eq!(ack.cmd, CMD_SLAVE_ACK);
        let session = ack.session ^ MASTER_SESSION;
        let fa_ms = self.real_ms();
        let mut now = self.cycles();
        let until = fa_ms * CYCLES_PER_SECOND / 1000;
        while now < until {
            self.ours.take(now);
            now += CYCLES_PER_SECOND / 10_000;
        }
        self.catch_up();
        let master = Packet::new(CMD_ASSERT_MASTER, 0x01, MASTER_SESSION, &[]).wire();
        self.ours.send(&master, now);
        now += airtime(master.len());
        let ours_ack = poll(&mut self.ours, &mut now).expect("slave ack");
        assert_eq!(ours_ack.session, ack.session);
        let mut real = Vec::new();
        let mut ours = Vec::new();
        for (cmd, extra, payload) in requests {
            let request = Packet::new(*cmd, *extra, session, payload);
            harness::send(&mut self.real, &request);
            if *cmd == 0x32 {
                self.real.run_until(CLOCK_SET, 0.05).unwrap();
            }
            self.ms = self.real_ms();
            self.ours.tick(self.ms);
            let mut now = self.cycles();
            self.ours.send(&request.wire(), now);
            now += airtime(request.wire().len());
            ours.push(poll(&mut self.ours, &mut now).map(Reply::from));
            real.push(
                harness::read_packet(&mut self.real, 0.05)
                    .ok()
                    .map(Reply::from),
            );
        }
        self.catch_up();
        (real, ours)
    }
}

fn poll(w: &mut hle::Walker, now: &mut u64) -> Option<Packet> {
    let until = *now + CYCLES_PER_SECOND / 20;
    while *now < until {
        *now += CYCLES_PER_SECOND / 10_000;
        if let Some(wire) = w.take(*now) {
            return Packet::from_wire(&wire);
        }
    }
    None
}

fn region(at: usize) -> &'static str {
    match at {
        0x0000..0x0080 => "magic",
        0x0080..0x00ED => "factory",
        0x00ED..0x0156 => "walker info",
        0x0156..0x0170 => "health",
        0x01ED..0x0270 => "second copies",
        0x8F00..0x91BE => "route",
        0x91BE..0xB800 => "route images",
        0xB800..0xBF00 => "received",
        0xBF00..0xCC00 => "special route",
        0xCC00..0xCE80 => "team",
        0xCE80..0xCEF0 => "results",
        0xCEF0..0xCF0C => "history",
        0xCF0C..0xD480 => "event log",
        0xD480..0xDE24 => "staging",
        0xDE24..0xF6F8 => "peers",
        _ => "other",
    }
}

fn differences(real: &[u8], ours: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < real.len() {
        if real[at] == ours[at] {
            at += 1;
            continue;
        }
        let start = at;
        while at < real.len() && real[at] != ours[at] && at - start < 16 {
            at += 1;
        }
        out.push(format!(
            "{start:04X} {}: real {:02X?} ours {:02X?}",
            region(start),
            &real[start..at],
            &ours[start..at]
        ));
    }
    out
}

fn assert_same_eeprom(pair: &Pair, what: &str) {
    assert_close_eeprom(pair, what, 0);
}

fn assert_close_eeprom(pair: &Pair, what: &str, log_seconds: u32) {
    let real = pair.real.eeprom();
    let mut ours = pair.ours.eeprom().bytes().to_vec();
    for entry in 0..LOG_ENTRIES {
        let at = EVENT_LOG + entry * LOG_ENTRY;
        let time = |bytes: &[u8]| u32::from_be_bytes([0, 1, 2, 3].map(|i| bytes[at + i]));
        let gap = time(real).abs_diff(time(&ours));
        if gap > 0 && gap <= log_seconds {
            eprintln!("{what}: log entry {entry} written {gap} s apart");
            ours[at..at + 4].copy_from_slice(&real[at..at + 4]);
        }
    }
    let diffs = differences(real, &ours);
    assert!(
        diffs.is_empty(),
        "{what}: {} differing runs\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

fn trainer_info() -> Vec<u8> {
    let mut info = vec![0; 0x68];
    info[0x00..0x04].copy_from_slice(&1u32.to_le_bytes());
    info[0x08..0x0A].copy_from_slice(&7u16.to_le_bytes());
    info[0x0C..0x0E].copy_from_slice(&31_337u16.to_le_bytes());
    info[0x0E..0x10].copy_from_slice(&0x1234u16.to_le_bytes());
    for (i, byte) in info[0x10..0x38].iter_mut().enumerate() {
        *byte = 0x40 + i as u8;
    }
    for (i, unit) in [0x0136u16, 0x0145, 0x0138, 0x0149, 0xFFFF]
        .iter()
        .enumerate()
    {
        info[0x48 + 2 * i..0x4A + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    info[0x5B] = ROLLOVER_HOUR << 3;
    info[0x5C] = 0x02;
    info[0x5E] = 0x01;
    info[0x60..0x64].copy_from_slice(&SYNC.to_be_bytes());
    info
}

fn route_upload() -> Vec<u8> {
    let mut route: Vec<u8> = (0..ROUTE_SIZE)
        .map(|i| {
            if i % 512 < 256 {
                (i / 24 * 7) as u8
            } else {
                (i * 13) as u8
            }
        })
        .collect();
    let walking = [
        25u8, 0, 0xEC, 0, 84, 0, 39, 0, 98, 0, 0, 0, 15, 0x20, 0x02, 0,
    ];
    route[..16].copy_from_slice(&walking);
    for (i, unit) in [0x0137u16, 0x0130, 0x0141, 0x0145, 0xFFFF]
        .iter()
        .enumerate()
    {
        route[0x10 + 2 * i..0x12 + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    route[0x26] = 100;
    for (slot, species) in [16u16, 19, 41].iter().enumerate() {
        let at = 0x52 + 16 * slot;
        route[at..at + 16].fill(0);
        route[at..at + 2].copy_from_slice(&species.to_le_bytes());
        route[at + 12] = 5 + slot as u8;
    }
    route[0x82..0x88].fill(0);
    route[0x88..0x8B].copy_from_slice(&[100, 100, 100]);
    for slot in 0..10 {
        let item = 0x11 + slot as u16;
        route[0x8C + 2 * slot..0x8E + 2 * slot].copy_from_slice(&item.to_le_bytes());
        route[0xA0 + 2 * slot..0xA2 + 2 * slot].copy_from_slice(&[0, 0]);
        route[0xB4 + slot] = 100;
    }
    route
}

fn team_upload() -> Vec<u8> {
    (0..TEAM_SIZE).map(|i| (i as u8) ^ 0x3C).collect()
}

fn runs(block: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < block.len() {
        let flag_at = out.len();
        out.push(0);
        for bit in (0..8).rev() {
            if at >= block.len() {
                break;
            }
            let mut run = 0;
            while at > 0 && run < 18 && at + run < block.len() && block[at + run] == block[at - 1] {
                run += 1;
            }
            if run >= 3 {
                out[flag_at] |= 1 << bit;
                out.extend_from_slice(&[((run - 3) << 4) as u8, 0x00]);
                at += run;
            } else {
                out.push(block[at]);
                at += 1;
            }
        }
    }
    (out.len() <= 124).then_some(out)
}

fn writes(address: usize, data: &[u8]) -> Vec<Request> {
    data.chunks(128)
        .enumerate()
        .map(|(i, block)| {
            let at = address + i * 128;
            let low = (at & 0x80) as u8;
            let extra = (at >> 8) as u8;
            match runs(block) {
                Some(stream) => {
                    let mut packed = vec![0x10, 0x80, 0x00, 0x00];
                    packed.extend(stream);
                    (low, extra, packed)
                }
                None => (low | 0x02, extra, block.to_vec()),
            }
        })
        .collect()
}

fn send_requests() -> Vec<Request> {
    let mut requests = vec![(0x20, 0x01, vec![]), (0x32, 0x01, trainer_info())];
    requests.extend(writes(ROUTE_STAGING, &route_upload()));
    requests.extend(writes(TEAM_STAGING, &team_upload()));
    requests.push((0x38, 0x01, vec![]));
    requests.push((0x5A, 0x01, vec![]));
    requests
}

fn return_requests() -> Vec<Request> {
    let mut requests = vec![(0x20, 0x01, vec![])];
    for (at, len) in [
        (0xCE80u16, 0x80u8),
        (0xCF00, 0x80),
        (0xCEF0, 0x1C),
        (0x0156, 0x19),
    ] {
        let [high, low] = at.to_be_bytes();
        requests.push((0x0C, 0x01, vec![high, low, len]));
    }
    requests.push((0x4E, 0x01, vec![]));
    requests
}

fn assert_same_replies(what: &str, (real, ours): (Vec<Option<Reply>>, Vec<Option<Reply>>)) {
    for (i, (r, o)) in real.iter().zip(&ours).enumerate() {
        assert_eq!(r, o, "{what}: reply {i} differs");
    }
}

fn sent(rom: Vec<u8>) -> Pair {
    let mut pair = Pair::boot(rom);
    pair.start_link();
    let replies = pair.session(&send_requests());
    assert_same_replies("send", replies);
    pair.wait(3.0);
    pair
}

#[test]
fn the_first_boot_formats_like_ours() {
    let Some(rom) = rom() else { return };
    let pair = Pair::boot(rom);
    assert_same_eeprom(&pair, "first boot");
}

#[test]
fn a_send_diffs_clean() {
    let Some(rom) = rom() else { return };
    let pair = sent(rom);
    assert_same_eeprom(&pair, "after the send");
}

#[test]
fn a_walk_across_the_rollover_and_a_return_diff_clean() {
    let Some(rom) = rom() else { return };
    let mut pair = sent(rom);
    pair.clocks("sent");
    pair.walk(300);
    pair.clocks("walked");
    pair.wait(240.0);
    pair.clocks("waited");
    pair.walk(120);
    pair.wait(5.0);
    pair.clocks("walked again");
    pair.wake();
    pair.clocks("woke");
    pair.connect();
    pair.clocks("connecting");
    let replies = pair.session(&return_requests());
    assert_same_replies("return", replies);
    pair.wait(2.0);
    assert_same_eeprom(&pair, "after the return");
}

#[test]
fn dowsing_on_the_same_bush_diffs_clean() {
    let Some(rom) = rom() else { return };
    let mut pair = sent(rom);
    pair.walk(80);
    pair.wake();
    pair.menu_to(hle::App::Dowsing);
    pair.open(DOWSING_OPEN);
    let bush = pair.real.read(BUSH);
    let hle::Screen::Dowsing(_) = pair.ours.screen() else {
        panic!("ours did not open dowsing: {:?}", pair.ours.screen());
    };
    for _ in 0..bush {
        pair.press(Key::Right, hle::Button::Right);
    }
    pair.press(Key::Enter, hle::Button::Middle);
    pair.wait(2.0);
    let hle::Screen::Dowsing(dowsing) = pair.ours.screen() else {
        panic!("ours left dowsing: {:?}", pair.ours.screen());
    };
    assert!(
        matches!(dowsing.state(), hle::DowsingState::Found(_)),
        "ours on bush {bush}: {:?}",
        dowsing.state()
    );
    pair.press(Key::Enter, hle::Button::Middle);
    pair.wait(1.0);
    assert_eq!(
        (pair.real.read(SCREEN), pair.ours.screen()),
        (0, hle::Screen::Home)
    );
    pair.connect();
    let replies = pair.session(&[(0x20, 0x01, vec![])]);
    assert_same_replies("flush", replies);
    pair.wait(2.0);
    assert_same_eeprom(&pair, "after dowsing");
}

fn toward(cursor: u8, bush: u8) -> Option<(Key, hle::Button)> {
    match (bush + 4 - cursor) % 4 {
        0 => None,
        1 | 2 => Some((Key::Right, hle::Button::Right)),
        _ => Some((Key::Left, hle::Button::Left)),
    }
}

#[test]
fn a_radar_battle_and_a_throw_diff_clean() {
    let Some(rom) = rom() else { return };
    let mut pair = sent(rom);
    pair.walk(240);
    pair.wake();
    pair.menu_to(hle::App::Radar);
    pair.open(RADAR_OPEN);
    let hle::Screen::Radar(_) = pair.ours.screen() else {
        panic!("ours did not open the radar: {:?}", pair.ours.screen());
    };
    let mut real_hits = 0;
    let mut ours_hits = 0;
    let mut real_rest = 0.0;
    for _ in 0..600 {
        if pair.real.read(SCREEN) == BATTLE_SCREEN {
            pair.real.run_seconds(0.1).unwrap();
        } else if real_rest > 0.0 || pair.real.read(RADAR_SHOWN) == 0 {
            real_rest -= 0.1;
            pair.real.run_seconds(0.1).unwrap();
        } else {
            match toward(pair.real.read(RADAR_CURSOR), pair.real.read(BUSH)) {
                Some((key, _)) => pair.tap(key),
                None => {
                    pair.tap(Key::Enter);
                    real_hits += 1;
                    real_rest = 1.2;
                }
            }
        }
        pair.catch_up();
        match pair.ours.screen() {
            hle::Screen::Radar(radar) => {
                if let Some(bush) = radar.shown() {
                    match toward(radar.cursor(), bush) {
                        Some((_, button)) => pair.ours.press(button),
                        None => {
                            pair.ours.press(hle::Button::Middle);
                            ours_hits += 1;
                        }
                    }
                }
            }
            hle::Screen::Battle(_) => {
                if pair.real.read(SCREEN) == BATTLE_SCREEN {
                    break;
                }
            }
            other => panic!("ours left the radar: {other:?}, hits {ours_hits}"),
        }
    }
    eprintln!("hits: real {real_hits} ours {ours_hits}");
    assert_eq!(
        pair.real.read(SCREEN),
        BATTLE_SCREEN,
        "the real walker found no battle"
    );
    pair.until_real(|real| {
        real.read(BATTLE_STATE) == APPEARED && real.read(0xF7D2) >= real.read(0xF7D3)
    });
    pair.tap(Key::Enter);
    pair.until_real(|real| real.read(BATTLE_STATE) == CHOOSING);
    pair.until_ours(|battle| battle.state() == hle::BattleState::Choosing);
    let state = pair.generator();
    let thrown_at = pair.ms;
    let before_throw = pair.ours.clone();
    pair.ours.set_rng_state(state as u64);
    pair.press(Key::Enter, hle::Button::Middle);
    let mut outcome = None;
    for _ in 0..300 {
        pair.wait(0.1);
        if let hle::Screen::Battle(battle) = pair.ours.screen()
            && matches!(
                battle.state(),
                hle::BattleState::Caught | hle::BattleState::Fled | hle::BattleState::Lost
            )
        {
            outcome = Some(battle.state());
            break;
        }
    }
    pair.wait(3.0);
    eprintln!(
        "outcome: ours {outcome:?}, real screen {} state {} caught slot {:?}",
        pair.real.read(SCREEN),
        pair.real.read(BATTLE_STATE),
        &pair.real.eeprom()[0xCE8C..0xCE8E]
    );
    for _ in 0..10 {
        if pair.real.read(SCREEN) == 0 && pair.ours.screen() == hle::Screen::Home {
            break;
        }
        if pair.real.read(SCREEN) != 0 {
            pair.tap(Key::Enter);
        }
        if pair.ours.screen() != hle::Screen::Home {
            pair.ours.press(hle::Button::Middle);
        }
        pair.wait(0.5);
    }
    assert_eq!(
        (pair.real.read(SCREEN), pair.ours.screen()),
        (0, hle::Screen::Home)
    );
    let real_caught = caught(pair.real.eeprom());
    if caught(pair.ours.eeprom().bytes()) != real_caught {
        let (tries, ours) = (1..200)
            .scan(state, |state, tries| {
                *state = state.wrapping_mul(0x19660D).wrapping_add(0x3C6EF35F);
                Some((
                    tries,
                    replay_throw(&before_throw, *state, thrown_at, pair.ms),
                ))
            })
            .find(|(_, ours)| caught(ours.eeprom().bytes()) == real_caught)
            .expect("no nearby state gives the real outcome");
        eprintln!("the real outcome came from the state {tries} steps after the throw");
        pair.ours = ours;
    }
    pair.connect();
    let replies = pair.session(&[(0x20, 0x01, vec![])]);
    assert_same_replies("flush", replies);
    pair.wait(2.0);
    assert_close_eeprom(&pair, "after the radar", 5);
}

fn ringed_bubble() -> Vec<u8> {
    let mut bubble = hle::Image::blank(16, 16);
    for at in 0..16 {
        for (x, y) in [(at, 0), (at, 15), (0, at), (15, at)] {
            bubble.shades[y * 16 + x] = 2;
        }
    }
    bubble.encode()
}

fn picture(frame: impl Fn(usize, usize) -> u8, x: usize, y: usize, width: usize) -> String {
    (y..y + 16)
        .map(|row| {
            (x..x + width)
                .map(|col| ['.', '-', '+', '#'][frame(col, row) as usize])
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_radar_bubble_covers_the_grass_like_the_real_walker() {
    let Some(rom) = rom() else { return };
    let mut eeprom = vec![0xFF; EEPROM_SIZE];
    for address in RADAR_BUBBLES {
        eeprom[address..address + 64].copy_from_slice(&ringed_bubble());
    }
    let mut bush = hle::Image::blank(32, 24);
    bush.shades.fill(1);
    bush.shades[0] = 0;
    let bush = bush.encode();
    eeprom[RADAR_BUSH..RADAR_BUSH + bush.len()].copy_from_slice(&bush);
    let mut pair = Pair::boot_from(rom, eeprom);
    pair.start_link();
    let replies = pair.session(&send_requests());
    assert_same_replies("send", replies);
    pair.wait(3.0);
    pair.walk(240);
    pair.wake();
    pair.menu_to(hle::App::Radar);
    pair.open(RADAR_OPEN);
    let mut shown = None;
    for _ in 0..300 {
        pair.wait(0.05);
        let hle::Screen::Radar(radar) = pair.ours.screen() else {
            panic!("ours left the radar: {:?}", pair.ours.screen());
        };
        if pair.real.read(RADAR_SHOWN) != 0
            && let Some(bush) = radar.shown()
        {
            shown = Some((pair.real.read(BUSH), bush));
            break;
        }
    }
    let (real_bush, ours_bush) = shown.expect("no bubble on both walkers");
    pair.wait(0.2);
    assert_eq!(real_bush, ours_bush, "the bubbles are on different bushes");
    let x = 16 + if ours_bush % 2 == 0 { 4 } else { 60 };
    let y = if ours_bush < 2 { 6 } else { 24 };
    let width = (hle::LCD_WIDTH - x).min(24);
    let frame = pair.real.frame();
    let real = picture(|col, row| frame[row * hle::LCD_WIDTH + col], x, y, width);
    let lcd = pair.ours.lcd(0);
    let ours = picture(|col, row| lcd.shade(col, row), x, y, width);
    eprintln!("real bubble at ({x}, {y}):\n{real}\nours:\n{ours}");
    assert_eq!(real, ours);
}

fn caught(eeprom: &[u8]) -> bool {
    eeprom[0xCE8C..0xCE8E] != [0, 0]
}

fn replay_throw(before: &hle::Walker, state: u32, from: u64, until: u64) -> hle::Walker {
    let mut ours = before.clone();
    ours.set_rng_state(state as u64);
    ours.press(hle::Button::Middle);
    let mut ms = from;
    while ms < until {
        ms = (ms + 100).min(until);
        ours.tick(ms);
        let done = match ours.screen() {
            hle::Screen::Home => false,
            hle::Screen::Battle(battle) => matches!(
                battle.state(),
                hle::BattleState::Caught | hle::BattleState::Fled | hle::BattleState::Lost
            ),
            _ => true,
        };
        if done {
            ours.press(hle::Button::Middle);
        }
    }
    ours
}
