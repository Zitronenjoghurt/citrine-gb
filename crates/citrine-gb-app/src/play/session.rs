use super::engine::{Rollback, Ticks};
use super::messages::to_byte;
use super::{
    DropHistory, Engine, LinkedSave, MyGame, Picture, PlaySession, Returned, Role, Round,
    STALL_SHOWN, SaveKey, Shown, Stage, sha1_of,
};
use citrine_gb::console::{Console, Keys};
use citrine_gb::gb::GameBoy;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb_net::{
    CableByte, GameCard, LinkPath, Mode, OnlineId, Origin, PeerLink, PlayMsg, SessionId,
};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

impl PlaySession {
    pub(super) fn new(friend: OnlineId, role: Role, stage: Stage, sharing: Option<u32>) -> Self {
        let now = Instant::now();
        Self {
            id: None,
            friend,
            key: None,
            apart: false,
            began: false,
            links_in: Vec::new(),
            sync: super::links::sync_for(None, 2, 1),
            lanes: 1,
            ds_cost: None,
            ds_measure: true,
            measured_cost: None,
            input_delay: None,
            role,
            stage,
            borrowing: false,
            downloading: false,
            borrowed_save: None,
            lent: false,
            from_peer: false,
            round: Round::new(1, None, None),
            roms: Vec::new(),
            wants_state: false,
            returned: None,
            pings: VecDeque::new(),
            next_ping: 0,
            round_trips: VecDeque::new(),
            stale_trips: 0,
            ping_path: LinkPath::Relay,
            #[cfg(test)]
            spread_for_test: None,
            pinged_at: None,
            started_at: now,
            heard_at: now,
            let_watch: true,
            friend_lets_watch: false,
            watch_friend: true,
            watch_chosen: false,
            my_cable: true,
            friend_cable: true,
            saves: Vec::new(),
            sharing,
            outbox: Vec::new(),
            game_outbox: Vec::new(),
            sealer: None,
            sealing: VecDeque::new(),
            sealed: Vec::new(),
            link: None,
            direct: super::direct::DirectLink::default(),
            sample_rate: 44_100,
            grouped: false,
            stashed: None,
            outbox_mark: None,
            parked: VecDeque::new(),
            failure: None,
            touch: None,
            ds_user: super::random_ds_user(),
        }
    }

    pub fn id(&self) -> Option<SessionId> {
        self.id
    }

    pub fn is_apart(&self) -> bool {
        self.apart
    }

    pub(super) fn key(&self, online: &crate::online::Online) -> Option<citrine_gb_net::PublicKey> {
        self.key
            .or_else(|| online.friend(self.friend).map(|f| f.public_key))
    }

    pub fn friend(&self) -> OnlineId {
        self.friend
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn sends_game(&self) -> bool {
        matches!(self.stage, Stage::Preparing | Stage::Receiving { .. })
            && self.sharing.is_some()
            && self.round.they_hold_mine == Some(false)
    }

    pub fn state_received(&self) -> Option<(u64, u64)> {
        if self.stage != Stage::Preparing {
            return None;
        }
        let (got, total) = self.link.as_ref()?.receiving()?;
        Some((u64::from(got), u64::from(total)))
    }

    pub fn game_sent(&self) -> Option<(u64, u64)> {
        let total = self.round.mine.as_ref()?.rom.len();
        let done = match (self.round.acked, self.round.sending) {
            (Some((acked, _)), _) => acked,
            (None, Some(offset)) => offset,
            (None, None) => total,
        };
        Some((done.min(total) as u64, total as u64))
    }

    pub fn is_live(&self) -> bool {
        !matches!(self.stage, Stage::Ended(_))
    }

    pub fn my_game(&self) -> Option<&GameCard> {
        self.round.my_card.as_ref()
    }

    pub fn friend_game(&self) -> Option<&GameCard> {
        self.round.theirs.as_ref().and_then(Option::as_ref)
    }

    pub fn shown_game(&self) -> Option<(&GameCard, Option<Arc<[u8]>>)> {
        match &self.round.my_card {
            Some(card) => Some((card, self.my_rom())),
            None => self
                .friend_game()
                .map(|card| (card, self.round.their_rom.clone())),
        }
    }

    pub fn friend_plays(&self) -> Option<bool> {
        self.round.theirs.as_ref().map(Option::is_some)
    }

    pub fn borrowing(&self) -> bool {
        self.borrowing
    }

    pub fn downloading(&self) -> bool {
        self.downloading
    }

    pub fn holds_theirs(&self) -> Option<bool> {
        self.round.i_hold_theirs
    }

    pub fn round(&self) -> u32 {
        self.round.number
    }

    pub fn mode(&self) -> Option<Mode> {
        self.round.start.as_ref().map(|s| s.mode)
    }

    pub fn delay(&self) -> Option<u8> {
        let start = self.round.start.as_ref()?.delay;
        Some(self.round.rollback().map_or(start, |r| r.delay() as u8))
    }

    pub fn path(&self) -> LinkPath {
        self.link.as_ref().map_or(LinkPath::Relay, PeerLink::path)
    }

    pub fn round_trip_ms(&self) -> Option<u32> {
        let mut sorted: Vec<u32> = self.round_trips.iter().copied().collect();
        sorted.sort_unstable();
        sorted.get(sorted.len() / 2).copied()
    }

    pub fn round_trip_spread_ms(&self) -> Option<u32> {
        #[cfg(test)]
        if let Some(ms) = self.spread_for_test {
            return Some(ms);
        }
        let median = self.round_trip_ms()?;
        let most = self.round_trips.iter().copied().max()?;
        Some(most - median)
    }

    pub fn lets_friend_watch(&self) -> bool {
        self.let_watch
    }

    pub fn friend_lets_me_watch(&self) -> bool {
        self.friend_lets_watch
    }

    pub fn watches_friend(&self) -> bool {
        self.watch_friend
    }

    pub fn sees_friend(&self) -> bool {
        self.watch_friend && self.friend_lets_watch && self.friend_picture().is_some()
    }

    pub fn my_cable(&self) -> bool {
        self.my_cable
    }

    pub fn friend_cable(&self) -> bool {
        self.friend_cable
    }

    pub fn cable_plugged(&self) -> bool {
        match &self.round.engine {
            Some(Engine::Rollback(_)) => self.round.shown().is_some_and(Shown::plugged),
            Some(Engine::Cable(c)) => c.plugged,
            Some(Engine::Apart) => false,
            None => self.round.start.as_ref().is_some_and(|s| s.plugged),
        }
    }

    pub fn waiting_for_friend(&self, now: Instant) -> bool {
        self.round
            .shown()
            .is_some_and(|r| r.stalled_for(now) >= STALL_SHOWN)
    }

    pub fn stalled_for(&self, now: Instant) -> Duration {
        self.round
            .shown()
            .map_or(Duration::ZERO, |r| r.stalled_for(now))
    }

    pub fn in_group(&self) -> bool {
        self.round.shown().is_some_and(Shown::group)
    }

    pub fn runs_engine(&self) -> bool {
        matches!(self.round.engine, Some(Engine::Rollback(_)))
    }

    pub fn friend_picture(&self) -> Option<Picture<'_>> {
        self.round.shown().and_then(Shown::friend)
    }

    pub fn fetch(&self) -> Option<super::fetch::Fetch> {
        match self.downloading {
            true => self.round.shown().and_then(Shown::fetch),
            false => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn engine_home(&self) -> bool {
        match &self.round.engine {
            Some(Engine::Rollback(r)) => r.is_home(),
            _ => true,
        }
    }

    pub fn console(&self, local: bool) -> Option<&dyn Console> {
        self.round.rollback().and_then(|r| r.console(local))
    }

    #[cfg(feature = "nds")]
    pub fn shown_nds_mut(&mut self) -> Option<&mut citrine_gb::nds::machine::Nds> {
        let local = self.console(true).is_some();
        self.round.rollback_mut().and_then(|r| r.nds_mut(local))
    }

    pub fn wants_state(&self) -> bool {
        self.wants_state
    }

    pub fn give_state(&mut self, state: Vec<u8>, history: Option<DropHistory>) {
        if !self.wants_state {
            return;
        }
        self.wants_state = false;
        let game = self.round.my_card.as_ref().map(|c| c.hash);
        if let Some(history) = history.filter(|h| h.fits(self.friend, game)) {
            self.outbox.push(PlayMsg::Rejoin {
                round: self.round.number,
                from: history.point(),
            });
            self.round.rejoin = Some(history);
        }
        self.round.my_origin = Some(Origin::Resume(state));
        self.send_setup();
    }

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn resumed_mine(&self) -> bool {
        matches!(self.round.my_origin, Some(Origin::Resume(_)))
    }

    pub fn take_returned(&mut self) -> Option<Returned> {
        self.returned.take()
    }

    pub fn holds_my_game(&self) -> bool {
        let resuming = self.wants_state || matches!(self.round.my_origin, Some(Origin::Resume(_)));
        resuming && self.is_live() && self.round.start.is_none() && self.stage != Stage::Inviting
    }

    pub fn shelves_mine(&self) -> bool {
        self.lent && self.is_live()
    }

    pub fn shares_games(&self) -> bool {
        self.sharing.is_some()
    }

    pub fn watching_only(&self) -> bool {
        self.is_live() && self.stage != Stage::Inviting && self.round.my_card.is_none()
    }

    pub fn has_friend_tab(&self) -> bool {
        self.is_live()
            && self.stage != Stage::Inviting
            && self.round.my_card.is_some()
            && self.friend_plays() == Some(true)
            && self.watch_friend
    }

    pub fn frame(&self) -> u32 {
        self.round.rollback().map_or(0, Rollback::frame)
    }

    pub fn resyncs(&self) -> u32 {
        self.round.rollback().map_or(0, Rollback::resyncs)
    }

    pub fn sync(&self) -> Option<citrine_gb::netplay::Sync> {
        self.round.rollback().map(Rollback::sync)
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate;
    }

    pub fn set_touch(&mut self, touch: Option<citrine_gb::console::Touch>) {
        self.touch = touch;
    }

    #[cfg(test)]
    pub(crate) fn resync_friend(&mut self, edit: impl FnOnce(&mut GameBoy)) -> Option<()> {
        self.round.rollback_mut()?.send_resync_with(edit)
    }

    #[cfg(test)]
    pub(crate) fn log(&self) -> Option<&super::engine::Log> {
        self.round.rollback().map(|r| &r.log)
    }

    #[cfg(test)]
    pub(crate) fn cable_transfers(&self) -> u64 {
        self.round
            .rollback()
            .map_or(0, |r| r.cable_transfers().iter().sum())
    }

    #[cfg(test)]
    pub(crate) fn game_boy(&self, local: bool) -> Option<&citrine_gb::gb::GameBoy> {
        self.round.rollback().and_then(|r| r.game_boy(local))
    }

    #[cfg(test)]
    pub(crate) fn start(&self) -> Option<citrine_gb_net::Start> {
        self.round.start.clone()
    }

    pub fn set_let_watch(&mut self, on: bool) {
        if self.let_watch == on {
            return;
        }
        self.let_watch = on;
        self.outbox.push(PlayMsg::Watch(on));
        let hidden = self.round.rollback().is_some_and(Rollback::hidden);
        if let Some(r) = self.round.rollback_mut() {
            r.show(on);
        }
        if on && hidden && self.is_live() {
            let mine = self.round.mine.take();
            let card = self.round.my_card.clone();
            self.next_round(self.round.number + 1, mine, card);
        }
    }

    pub fn set_watch_friend(&mut self, on: bool) {
        self.watch_friend = on;
        self.watch_chosen = true;
        let shown = self.friend_lets_watch && on;
        if let Some(r) = self.round.rollback_mut() {
            r.watch(shown);
        }
    }

    pub fn set_cable(&mut self, plugged: bool) {
        if self.my_cable == plugged {
            return;
        }
        self.my_cable = plugged;
        self.outbox.push(PlayMsg::Cable(plugged));
        self.join_cables();
    }

    pub(super) fn join_cables(&mut self) {
        let linked = self.my_cable && self.friend_cable;
        match &mut self.round.engine {
            Some(Engine::Rollback(r)) => r.get_mut().request_cable(linked),
            Some(Engine::Cable(c)) => c.plugged = linked,
            Some(Engine::Apart) | None => {}
        }
    }

    pub fn bring(&mut self, game: MyGame) -> bool {
        if !self.is_live() || self.stage == Stage::Inviting || self.id.is_none() {
            return false;
        }
        let (mine, card) = match game {
            MyGame::Own(file) => match file.card() {
                Some(card) => (Some(file), Some(card)),
                None => return false,
            },
            MyGame::Borrow { .. } | MyGame::Download => return false,
            MyGame::Nothing => (None, None),
        };
        self.borrowing = false;
        self.downloading = false;
        self.next_round(self.round.number + 1, mine, card);
        true
    }

    pub(super) fn show_card(&mut self, card: Option<GameCard>) -> bool {
        if !self.is_live()
            || self.stage == Stage::Inviting
            || self.id.is_none()
            || self.round.my_card == card
        {
            return false;
        }
        self.next_round(self.round.number + 1, None, card);
        true
    }

    pub fn hand_rom(&mut self, rom: Arc<[u8]>) -> bool {
        if !self.is_live() || self.id.is_none() {
            return false;
        }
        let Some(hash) = sha1_of(&rom).filter(|h| self.friend_game().map(|g| g.hash) == Some(*h))
        else {
            return false;
        };
        self.keep_rom(hash, rom);
        let mine = self.round.mine.take();
        let card = self.round.my_card.clone();
        self.next_round(self.round.number + 1, mine, card);
        true
    }

    pub fn take_saves(&mut self) -> Vec<LinkedSave> {
        self.settle_frames();
        std::mem::take(&mut self.saves)
    }

    pub fn from_peer(&self) -> bool {
        self.from_peer || self.round.rollback().is_some_and(Rollback::local_from_peer)
    }

    pub fn mark_from_peer(&mut self) {
        self.from_peer = true;
    }

    pub(super) fn note_peer_state(&mut self) {
        if self.round.rollback().is_some_and(Rollback::local_from_peer) {
            self.from_peer = true;
        }
    }

    fn save_key(&self) -> Option<SaveKey> {
        if self.borrowing
            && let Some(card) = &self.round.my_card
        {
            return Some(SaveKey::Borrowed(card.hash));
        }
        let console = self.console(true)?;
        if self.from_peer() {
            let hash = self
                .round
                .my_card
                .as_ref()
                .map_or_else(|| console.game_sha1(), |card| card.hash);
            return Some(SaveKey::Borrowed(hash));
        }
        Some(SaveKey::Rom(console.rom_id().sha256_hex_string()))
    }

    pub(super) fn keep_saves(&mut self, dumps: impl IntoIterator<Item = SramDump>) {
        let Some(key) = self.save_key() else {
            return;
        };
        self.saves.extend(dumps.into_iter().map(|dump| LinkedSave {
            key: key.clone(),
            dump,
        }));
    }

    pub fn drain_audio(&mut self, mine: &mut Vec<f32>, friend: &mut Vec<f32>) {
        if let Some(r) = self.round.rollback_mut() {
            r.drain_audio(mine, friend);
        }
    }

    pub fn run_frame(&mut self, buttons: Keys, now: Instant) -> bool {
        self.run_frames(vec![buttons], now);
        self.settle_frames()
    }

    pub fn run_frames(&mut self, pads: Vec<Keys>, now: Instant) {
        self.settle_frames();
        if self.stage != Stage::Playing {
            return;
        }
        let plan = Ticks {
            pads,
            touch: self.touch,
            cable: Some(self.my_cable && self.friend_cable),
            saves: true,
            round_trip_ms: self.round_trip_ms().unwrap_or(0),
            spread_ms: self.round_trip_spread_ms().unwrap_or(0),
            aim: self.aim(),
            rise_aim: self.rise_aim(),
            now,
        };
        let Some(Engine::Rollback(r)) = &mut self.round.engine else {
            return;
        };
        self.outbox_mark = Some(self.outbox.len());
        if r.has_worker() {
            self.round.seen.publish(r.get());
        }
        r.lend(plan);
    }

    pub fn wait_frames(&self, timeout: std::time::Duration) -> bool {
        match &self.round.engine {
            Some(Engine::Rollback(r)) => r.wait(timeout),
            _ => true,
        }
    }

    pub fn settle_frames(&mut self) -> bool {
        let Some(mark) = self.outbox_mark.take() else {
            return false;
        };
        let Some(r) = self.round.rollback_mut() else {
            return false;
        };
        let ticked = r.take_ticked();
        let measured = r.measured_cost();
        let budget = r.sync().budget();
        self.round.budget_now = Some(budget);
        if let Some(cost) = measured
            && let Some(key) = self.cost_key()
        {
            self.measured_cost = Some((key, cost));
        }
        let mark = mark.min(self.outbox.len());
        self.outbox.splice(mark..mark, ticked.outbox);
        self.keep_saves(ticked.saves);
        ticked.ran
    }

    pub fn frame_timer(&self) -> Option<&crate::utils::avg_timer::AvgTimer> {
        self.round.rollback().map(Rollback::frame_timer)
    }

    pub fn frame_totals(&self) -> Option<crate::frame_log::SessionTotals> {
        let r = self.round.rollback()?;
        let counts = r.counts();
        Some(crate::frame_log::SessionTotals {
            delay: r.delay(),
            ping_ms: self.round_trip_ms(),
            ran: counts.ran,
            waited: counts.waited,
            skipped: counts.skipped,
            repaid: counts.repaid,
            ahead: r.ahead(),
            watching: r.watches(),
        })
    }

    pub fn engine_on_worker(&self) -> bool {
        matches!(&self.round.engine, Some(Engine::Rollback(r)) if r.has_worker())
    }

    pub(super) fn place_engine(&mut self, threads: Option<&citrine_gb_threads::Threads>) {
        if let Some(threads) = threads {
            self.lanes = threads.pool_size();
        }
        let Some(Engine::Rollback(r)) = &mut self.round.engine else {
            return;
        };
        match threads {
            Some(threads) if !r.has_worker() => {
                r.run_on(threads, format!("session: {}", self.friend))
            }
            Some(_) => {}
            None if r.has_worker() => r.park(),
            None => {}
        }
    }

    pub fn pump_link(&mut self, gb: &mut GameBoy) {
        let Some(Engine::Cable(c)) = &mut self.round.engine else {
            return;
        };
        let live = self.stage == Stage::Playing;
        let want = live && c.plugged;
        if c.applied != Some(want) {
            if want {
                gb.plug_link();
            } else {
                gb.unplug_link();
            }
            c.applied = Some(want);
        }
        if !want {
            c.incoming.clear();
            return;
        }
        while let Some(msg) = c.incoming.pop_front() {
            gb.receive_link_message(msg);
        }
        let bytes: Vec<CableByte> = gb.take_link_messages().into_iter().map(to_byte).collect();
        if !bytes.is_empty() {
            self.outbox.push(PlayMsg::Bytes(bytes));
        }
    }

    pub fn finish(mut self) -> Option<Returned> {
        self.retire_round();
        self.returned.take()
    }

    pub fn finish_saves(&mut self) -> Vec<LinkedSave> {
        self.settle_frames();
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
        self.take_saves()
    }
}
