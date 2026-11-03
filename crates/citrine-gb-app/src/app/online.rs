use super::Citrine;
use super::tabs;
use crate::app::tabs::Tab;
use crate::app::ui_state::synced;
use crate::play::{MyGame, Stage};
use egui::Context;

impl Citrine {
    pub(super) fn update_online(&mut self, ctx: &Context) {
        let now = web_time::Instant::now();
        let touched = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down());
        let held = self.emulator.held_buttons();
        if touched || held.bits() != self.activity.held.bits() {
            self.activity.last_input = now;
        }
        self.activity.held = held;
        let away = now.duration_since(self.activity.last_input) >= crate::online::AWAY_AFTER;
        let cartridge = &self.emulator.gb.cartridge;
        let game = (self.ui.settings.online.show_playing && cartridge.has_rom_loaded)
            .then_some(cartridge.header.sha1);
        self.online.set_activity(away, game);
        let settings = &self.ui.settings.online;
        self.online
            .set_cloud(settings.cloud_owner(), settings.cloud_saves);
        let sync = settings.cloud_saves && settings.sync_settings;
        self.online.update(now);
        self.online
            .set_typing_indicators(settings.typing_indicators, now);
        self.online.compose(self.ui.friends.chat.composing(), now);
        if self
            .online
            .sync_settings(&mut self.ui.settings.synced, sync)
        {
            let values = self.ui.settings.synced.values();
            synced::adopt(&mut self.ui.settings, &values);
            self.ui.settings.unsaved = true;
        }
        self.settle_cloud_game(now);
        self.play.update(&mut self.online, now);

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
                let played = session.mode().is_some();
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
                    let name = tabs::playing::friend_name(&self.online, friend);
                    self.ui.playing.ended =
                        tabs::playing::ending_line(&name, ending, played).map(|line| {
                            tabs::playing::Ended {
                                friend,
                                line,
                                paused,
                            }
                        });
                }
                self.ui.playing.asking_stop = false;
                self.ui.playing.shown = false;
                if let Some(next) = self.ui.playing.switch_to.take() {
                    self.play_again(next);
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

    pub(super) fn play_picker(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.ui.playing.wants_copy) {
            self.ui.playing.copy_for = true;
            self.try_start_audio();
            crate::utils::file_loader::FileLoader::new()
                .title("Open your copy of the game")
                .add_filter("Game Boy ROMs", &["gb", "gbc"])
                .dispatch(self.files.play_rom_tx.clone());
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
                self.picked_for(target, |app| app.handle_load_rom_data(game.data()));
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
                    .add_filter("Game Boy ROMs", &["gb", "gbc"])
                    .dispatch(self.files.play_rom_tx.clone());
            }
            tabs::playing::Pick::Theirs => {
                if let tabs::playing::PickFor::Join(invite) = target {
                    self.answer_ask(&invite);
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
                let file = self.emulator.game_file();
                if let (Some(session), Some(file)) = (self.play.session_mut(), file)
                    && !session.bring(MyGame::Own(file))
                {
                    self.toasts.error("Couldn't change the game");
                }
            }
        }
    }

    pub(super) fn put_my_game_away(&mut self) {
        let Some(session) = self.play.session_mut() else {
            return;
        };
        if session.my_game().is_none() || !session.bring(MyGame::Nothing) {
            return;
        }
        if let Some(back) = session.take_returned() {
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
