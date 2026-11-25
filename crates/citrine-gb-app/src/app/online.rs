use super::Citrine;
use super::tabs;
use crate::app::tabs::Tab;
use crate::app::ui_state::synced;
use crate::app::widgets::snapshots::{FailedStep, failed_message};
use crate::play::{Ending, MyGame, Stage};
use crate::utils::save_files::LINK_ROM_FILTER;
use egui::Context;

impl Citrine {
    fn tell_gts_trades(&mut self) {
        let traded = self.online.take_gts_traded();
        #[cfg(feature = "nds")]
        if !traded.is_empty() {
            self.ui
                .gts
                .learn_names(&self.game_info, &self.emulator.store);
        }
        #[cfg(feature = "nds")]
        for deposit in traded {
            if let Some(note) = self.ui.gts.traded_note(&deposit) {
                self.toasts
                    .success(note)
                    .button("Open", super::events::AppEvent::OpenTab { tab: Tab::Gts });
            }
        }
        let sim_notes = self.online.take_gts_sim_notes();
        #[cfg(feature = "nds")]
        for note in sim_notes {
            let text = tabs::gts::sim_note(&note);
            self.toasts
                .info(text)
                .button("Open", super::events::AppEvent::OpenTab { tab: Tab::Gts });
        }
        #[cfg(not(feature = "nds"))]
        drop((traded, sim_notes));
    }

    #[cfg(ds_wifi)]
    fn tell_ds_net_refusal(&mut self) {
        if let Some(refusal) = self.emulator.ds_wifi.take_refusal() {
            self.show_ds_net_refusal(refusal);
        }
    }

    #[cfg(ds_wifi)]
    fn show_ds_net_refusal(&mut self, refusal: crate::ds_wifi::Refusal) {
        use crate::ds_wifi::Refusal;
        let text = match refusal {
            Refusal::SignedOut => {
                "DS online play goes through your relay. Sign in, or connect without it."
            }
            Refusal::RelayOff => {
                "This relay doesn't offer DS online play. You can connect without it."
            }
        };
        self.toasts
            .error(text)
            .button("Settings", super::events::AppEvent::FindDsDirect)
            .needs_action();
    }

    pub(super) fn update_online(&mut self, ctx: &Context) {
        let now = web_time::Instant::now();
        let touched = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down());
        let held = self.emulator.held_buttons();
        if touched || held.bits() != self.activity.held.bits() {
            self.activity.last_input = now;
        }
        self.activity.held = held;
        #[cfg(target_os = "android")]
        if crate::platform::android::lifecycle::take_shown() {
            self.activity.last_input = now;
        }
        #[cfg(target_os = "android")]
        let gone = !crate::platform::android::lifecycle::visible();
        #[cfg(not(target_os = "android"))]
        let gone = false;
        let away =
            gone || now.duration_since(self.activity.last_input) >= crate::online::AWAY_AFTER;
        let game = self
            .emulator
            .open_game()
            .filter(|_| self.ui.settings.online.show_playing);
        self.online.set_activity(away, game);
        let hidden = self.is_mobile(ctx) && ctx.input(|i| i.viewport().focused) == Some(false);
        let paused = self.emulator.has_game() && !self.emulator.is_running();
        self.play.set_status(paused, hidden);
        self.play.tick_watch(now);
        if self.play.room().is_some_and(|r| r.watching().is_some()) {
            ctx.request_repaint();
        }
        let settings = &self.ui.settings.online;
        self.online.choose_device_name(&settings.device_name);
        self.online.set_typing_text(&settings.typing_text);
        self.online
            .set_cloud(settings.cloud_owner(), settings.cloud_saves);
        let keep = (self.ui.library.revision(), settings.keep_saves);
        if self.cloud_keep != Some(keep) {
            self.cloud_keep = Some(keep);
            let roms = self
                .ui
                .library
                .entries()
                .into_iter()
                .filter_map(|game| game.rom_key.clone());
            self.online.cloud_keep(keep.1, roms);
        }
        self.online.network_settings(
            settings.network_saves,
            settings.network_allowed.iter().map(|key| key.0),
        );
        let sync_cloud = settings.cloud_saves && settings.sync_settings;
        let sync_network =
            crate::app::widgets::settings::pages::sync::network_on(&self.ui.settings)
                && settings.sync_settings;
        let typing = settings.typing_indicators;
        self.update_push();
        self.online.update(now);
        #[cfg(ds_wifi)]
        self.emulator.ds_wifi.pump(&mut self.online);
        #[cfg(ds_wifi)]
        self.tell_ds_net_refusal();
        if cfg!(feature = "nds") && self.ui.settings.ds_online.gts_here() {
            self.online.gts_want_mine();
        }
        self.tell_gts_trades();
        #[cfg(ds_wifi)]
        {
            let deposits = self.online.gts_mine().map_or(0, <[_]>::len);
            if self
                .ui
                .gts
                .switch
                .guard(ctx, &mut self.ui.settings.ds_online, deposits)
            {
                self.ui.settings.dirty = true;
            }
        }
        self.follow_push(now);
        self.online.set_typing_indicators(typing, now);
        self.online.compose(self.ui.friends.chat.composing(), now);
        if self
            .online
            .sync_settings(&mut self.ui.settings.synced, sync_cloud, sync_network)
        {
            let values = self.ui.settings.synced.values();
            synced::adopt(&mut self.ui.settings, &values);
            self.ui.settings.unsaved = true;
        }
        self.settle_cloud_game(now);
        self.play
            .set_input_delay(self.ui.settings.online.input_delay.frames());
        self.play.set_direct(self.ui.settings.online.direct);
        #[cfg(feature = "nds")]
        self.play
            .set_ds_user(self.emulator.ds_user(self.play.ds_mac()));
        tabs::playing::refresh_shelf(&mut self.play, &mut self.ui.playing, &self.ui.recent);
        self.play.update(&mut self.online, &self.threads, now);
        self.follow_opened_invite(now);
        if let Some(invite) = self.ui.playing.join_looking.take()
            && self
                .play
                .invites()
                .iter()
                .any(|i| i.session == invite.session)
        {
            self.answer_ask(&invite);
        }

        if let Some(joined) = self.online.take_joined() {
            let online = &mut self.ui.settings.online;
            online.remember(
                &joined.address,
                &joined.relay_id,
                &joined.password,
                &joined.account,
                &joined.trust,
            );
            self.ui.settings.dirty = true;
            self.ui.join = Default::default();
            if !self.is_mobile(ctx) {
                self.open_tab(Tab::Friends);
            }
        }
        if let Some(account) = self.online.take_account_change() {
            self.ui.settings.online.note_account(account);
            self.ui.settings.dirty = true;
        }
        if let Some(trust) = self.online.take_trust() {
            self.ui.settings.online.set_trust(&trust);
            self.ui.settings.dirty = true;
        }
        if let Some(address) = self.online.take_signed_out() {
            self.online.leave();
            let key = address.key();
            self.ui.settings.online.forget_current(&key);
            self.ui.settings.dirty = true;
            self.ui.join = crate::app::widgets::settings::online::JoinForm::signed_out_of(key);
        }
        if self.online.take_deleted() {
            self.online.leave();
            if let Some(key) = self.ui.settings.online.joined.clone() {
                self.ui.settings.online.forget_current(&key);
            }
            self.ui.settings.dirty = true;
            self.ui.join = Default::default();
        }
        if !self.ui.settings.online.is_joined()
            && let Some(location) = self.dock.find_tab(&Tab::Friends)
        {
            self.dock.remove_tab(location);
        }
    }

    pub(super) fn settle_play(&mut self, ctx: &Context) {
        self.settle_room();
        if std::mem::take(&mut self.ui.playing.hardcore_refused) {
            self.hardcore_refused("Play together is off in hardcore");
        }
        if let Some(ended) = &self.ui.playing.ended
            && match ended.paused {
                true => self.emulator.running,
                false => self.play.invites().iter().any(|i| i.from == ended.friend),
            }
        {
            self.ui.playing.ended = None;
        }
        match self.play.session() {
            Some(session) if session.is_live() => {
                let friend_tab = session.has_friend_tab();
                if session.stage() != Stage::Inviting && !self.ui.playing.shown {
                    self.ui.playing.shown = true;
                    if self.is_mobile(ctx) {
                        self.close_mobile_sheet();
                    }
                }
                if !self.is_mobile(ctx) {
                    self.sync_friend_tab(friend_tab);
                }
                return;
            }
            Some(session) => {
                let friend = session.friend();
                let title = tabs::playing::game_title(session);
                let played = session.mode().is_some();
                let failure = session.failure().map(str::to_owned);
                let ending = match session.stage() {
                    Stage::Ended(ending) => Some(ending),
                    _ => None,
                };
                let mut paused = false;
                if let Some(session) = self.play.close() {
                    match self.emulator.finish_session(session) {
                        Ok(held) => paused = held,
                        Err(err) => {
                            self.toasts.error(format!("Couldn't keep the game: {err}"));
                        }
                    }
                }
                if let Some(ending) = ending {
                    let name = tabs::playing::member_name(&self.play, &self.online, friend);
                    let line = match failure {
                        Some(why) if ending == Ending::Failed => {
                            Some(tabs::playing::failed_line(&name, &why))
                        }
                        _ => tabs::playing::ending_line(&name, &title, ending, played),
                    };
                    self.ui.playing.ended = line.map(|line| tabs::playing::Ended {
                        friend,
                        line,
                        paused,
                    });
                }
                self.ui.playing.asking_stop = false;
                self.ui.playing.shown = false;
                if let Some(next) = self.ui.playing.switch_to.take() {
                    self.play_again(next);
                    return;
                }
                if let Some(invite) = self.ui.playing.join_after.take() {
                    self.answer_ask(&invite);
                    return;
                }
            }
            None => {}
        }
        if self.keeps_link_tab() {
            return;
        }
        if let Some(location) = self.dock.find_tab(&Tab::Playing) {
            self.dock.remove_tab(location);
        }
        if self.mobile.showing(Tab::Playing) {
            self.close_mobile_sheet();
        }
    }

    fn settle_room(&mut self) {
        if self
            .play
            .room()
            .is_some_and(|r| r.members().len() > 1 && r.is_live())
        {
            return;
        }
        let playing = &mut self.ui.playing;
        playing.room_sheet = None;
        playing.relinking = None;
        if self.play.room().is_none() {
            playing.room_seen = 0;
            playing.room_draft.clear();
        }
        if let Some(location) = self.dock.find_tab(&Tab::RoomChat) {
            self.dock.remove_tab(location);
        }
    }

    pub(super) fn play_picker(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.ui.playing.wants_copy) {
            self.ui.playing.copy_for = true;
            self.try_start_audio();
            crate::utils::file_loader::FileLoader::new()
                .title("Open your copy of the game")
                .add_filter(LINK_ROM_FILTER.0, LINK_ROM_FILTER.1)
                .dispatch(&self.files.spawner, self.files.play_rom_tx.clone());
        }
        let Some((target, pick)) = tabs::playing::picker(
            ctx,
            &mut self.ui.playing,
            &self.online,
            &self.ui.recent,
            &self.play,
        ) else {
            return;
        };
        match pick {
            tabs::playing::Pick::Homebrew(game) => {
                self.picked_for(target, |app| app.play_shelf_game(game));
            }
            #[cfg(not(target_arch = "wasm32"))]
            tabs::playing::Pick::Recent(path) => {
                self.picked_for(target, |app| app.load_recent(path));
            }
            tabs::playing::Pick::File => {
                self.ui.playing.file_for = Some(target);
                self.try_start_audio();
                crate::utils::file_loader::FileLoader::new()
                    .title("Open a game to play together")
                    .add_filter(LINK_ROM_FILTER.0, LINK_ROM_FILTER.1)
                    .dispatch(&self.files.spawner, self.files.play_rom_tx.clone());
            }
            tabs::playing::Pick::Theirs => {
                if let tabs::playing::PickFor::Join(invite) = target {
                    self.join_theirs(&invite);
                }
            }
            tabs::playing::Pick::Download => {
                if let tabs::playing::PickFor::Join(invite) = target {
                    self.join_with(&invite, MyGame::Download);
                }
            }
            tabs::playing::Pick::Nothing => match target {
                tabs::playing::PickFor::Join(invite) => self.join_with(&invite, MyGame::Nothing),
                tabs::playing::PickFor::Change => self.put_my_game_away(),
                tabs::playing::PickFor::Invite(_) => {}
            },
        }
    }

    pub(super) fn picked_for(
        &mut self,
        target: tabs::playing::PickFor,
        load: impl FnOnce(&mut Self),
    ) {
        match target {
            tabs::playing::PickFor::Invite(friend) => {
                load(self);
                if self.emulator.open_game().is_some() {
                    self.play_again(friend);
                }
            }
            tabs::playing::PickFor::Join(invite) => {
                load(self);
                if let Some(file) = self.emulator.game_file() {
                    self.join_with(&invite, MyGame::Own(file));
                }
            }
            tabs::playing::PickFor::Change => {
                if !self.play.is_live() {
                    return;
                }
                self.put_my_game_away();
                load(self);
                if let Some(file) = self.emulator.game_file()
                    && !self.play.bring(MyGame::Own(file))
                {
                    self.toasts.error("Couldn't change the game");
                }
            }
        }
    }

    pub(super) fn unlink_load_confirm(&mut self, ctx: &Context) {
        let Some(slot) = self.ui.playing.unlink_load else {
            return;
        };
        let Some(partners) = tabs::playing::group_partners(&self.play, &self.online) else {
            self.ui.playing.unlink_load = None;
            return;
        };
        let choice = super::widgets::confirm::confirm_losing_progress(
            ctx,
            "unlink_load_confirm",
            &format!("Load this state? You'll unlink from {partners}."),
            "Load",
        );
        let Some(confirmed) = choice else {
            return;
        };
        self.ui.playing.unlink_load = None;
        if !confirmed {
            return;
        }
        tabs::playing::unlink_group(&mut self.play, &mut self.emulator);
        let name = self.emulator.snapshot_name(slot);
        match self.emulator.load_snapshot(slot) {
            Ok(_) => self.toasts.success(format!("Loaded {name}")),
            Err(err) => self
                .toasts
                .error(failed_message(FailedStep::Load, &name, &err)),
        };
        self.ui.snapshots.invalidate();
    }

    pub(super) fn put_my_game_away(&mut self) {
        tabs::playing::unlink_group(&mut self.play, &mut self.emulator);
        let holds = self.play.session().is_some_and(|s| s.my_game().is_some());
        if !holds || !self.play.bring(MyGame::Nothing) {
            return;
        }
        if let Some(back) = self.play.session_mut().and_then(|s| s.take_returned()) {
            self.emulator.keep_returned(back);
        }
    }

    pub(super) fn join_with(&mut self, invite: &crate::play::IncomingInvite, game: MyGame) {
        let joined = tabs::playing::join_with(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            invite,
            game,
        );
        if !joined {
            self.toasts.error("Couldn't answer the invite");
        }
    }

    pub(super) fn answer_ask(&mut self, invite: &crate::play::IncomingInvite) {
        let joined = tabs::playing::join(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            &self.ui.recent,
            invite,
        );
        if !joined {
            self.toasts.error("Couldn't answer the invite");
        }
    }

    fn join_theirs(&mut self, invite: &crate::play::IncomingInvite) {
        let joined = tabs::playing::join_theirs(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            &self.ui.recent,
            invite,
        );
        if !joined {
            self.toasts.error("Couldn't answer the invite");
        }
    }

    pub(super) fn play_again(&mut self, friend: citrine_gb_net::OnlineId) {
        let invited = tabs::playing::invite(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            &self.ui.recent,
            friend,
        );
        if !invited {
            self.toasts.error("Couldn't send the invite");
        }
    }
}
