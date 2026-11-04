use crate::app::Citrine;
use crate::app::tabs::{Tab, linked, playing};
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets::avatar;
use crate::icons;
use crate::play::Stage;
use egui::{Align, Layout, RichText, Ui, vec2};

pub const STRIP_HEIGHT: f32 = 48.0;
pub const STRIP_INSET: f32 = 10.0;
const STRIP_AVATAR: f32 = 28.0;
const STRIP_BUTTON: f32 = 40.0;
const STRIP_GAP: f32 = 8.0;

pub fn strip_height(buttons: usize, width: f32) -> f32 {
    let per_row = ((width - 4.0 + STRIP_GAP) / (STRIP_BUTTON + STRIP_GAP)).floor();
    let per_row = (per_row as usize).max(1);
    STRIP_HEIGHT * buttons.div_ceil(per_row).max(1) as f32
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripButton {
    Hide,
    FastForward,
    Pause,
    SaveState,
    Chat,
    Switch,
    StopRecording,
    More,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripLead {
    Title(String),
    Friend { watching: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripPlan {
    pub lead: StripLead,
    pub buttons: Vec<StripButton>,
}

pub struct StripFacts {
    pub title: String,
    pub session: bool,
    pub room: bool,
    pub watching_only: bool,
    pub linked: bool,
    pub recording: bool,
    pub can_save: bool,
}

pub fn plan(facts: &StripFacts) -> StripPlan {
    let mut buttons = vec![StripButton::Hide];
    if facts.recording {
        buttons.push(StripButton::StopRecording);
    }
    let lead = if facts.room {
        if !facts.watching_only {
            buttons.push(StripButton::Pause);
        }
        if facts.session && facts.linked {
            StripLead::Friend {
                watching: facts.watching_only,
            }
        } else {
            StripLead::Title(facts.title.clone())
        }
    } else if facts.session {
        buttons.push(StripButton::Chat);
        if !facts.watching_only {
            buttons.push(StripButton::Pause);
        }
        StripLead::Friend {
            watching: facts.watching_only,
        }
    } else if facts.linked {
        buttons.extend([StripButton::Switch, StripButton::Pause]);
        StripLead::Title(facts.title.clone())
    } else {
        if !facts.recording {
            buttons.push(StripButton::FastForward);
        }
        buttons.push(StripButton::Pause);
        if facts.can_save {
            buttons.push(StripButton::SaveState);
        }
        StripLead::Title(facts.title.clone())
    };
    buttons.push(StripButton::More);
    StripPlan { lead, buttons }
}

impl Citrine {
    fn room_linked(&self) -> bool {
        let me = self.online.account().map(|a| a.online_id);
        self.play.room().zip(me).is_some_and(|(room, me)| {
            room.links()
                .group_of(me)
                .is_some_and(|g| g.members().len() > 1)
        })
    }

    pub(super) fn strip_facts(&self) -> StripFacts {
        let session = self.play.session();
        let title = match self.link_control() {
            Some(crate::local_link::Game::Second) => linked::title(&self.emulator, false),
            _ => self.emulator.game_name().to_owned(),
        };
        StripFacts {
            title,
            session: session.is_some(),
            room: self
                .play
                .room()
                .is_some_and(|r| r.is_live() && r.members().len() > 1),
            watching_only: session.is_some_and(|s| s.watching_only()),
            linked: self.emulator.link.is_some() || self.room_linked(),
            recording: self.emulator.screen_recorder.is_recording(),
            can_save: self.emulator.rom_key().is_some() && self.emulator.partner.is_none(),
        }
    }

    pub(super) fn strip(&mut self, ui: &mut Ui, buttons_only: bool) {
        let plan = plan(&self.strip_facts());
        let height = STRIP_HEIGHT;
        let mut pressed = None;
        let mut open_friend = false;
        let layout = if buttons_only {
            Layout::right_to_left(Align::Min).with_main_wrap(true)
        } else {
            Layout::right_to_left(Align::Center)
        };
        ui.allocate_ui_with_layout(vec2(ui.available_width(), height), layout, |ui| {
            ui.set_min_height(height);
            ui.add_space(4.0);
            for button in plan.buttons.iter().rev() {
                if self.strip_button(ui, *button) {
                    pressed = Some(*button);
                }
            }
            if buttons_only {
                return;
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add_space(STRIP_INSET);
                match &plan.lead {
                    StripLead::Title(title) => {
                        ui.add(
                            egui::Label::new(RichText::new(title).size(16.0).strong()).truncate(),
                        );
                    }
                    StripLead::Friend { watching } => {
                        open_friend = self.strip_friend(ui, *watching);
                    }
                }
            });
        });
        if open_friend && let Some(session) = self.play.session() {
            let friend = session.friend();
            self.ui.friends.show_profile(friend);
            self.mobile.open(Tab::Friends);
            self.release_touch();
        }
        if let Some(button) = pressed {
            self.press_strip(button);
        }
    }

    fn strip_friend(&self, ui: &mut Ui, watching: bool) -> bool {
        let Some(session) = self.play.session() else {
            return false;
        };
        let friend = session.friend();
        let full = playing::full_name(&self.play, &self.online, friend);
        let name = playing::short_name(&full).to_owned();
        let tapped = avatar::avatar(
            ui,
            &full,
            friend.value(),
            STRIP_AVATAR,
            self.online.friend_presence(friend),
        )
        .on_hover_text(&full)
        .clicked();
        let strong = ui.visuals().strong_text_color();
        if watching {
            ui.add(
                egui::Label::new(RichText::new(format!("Watching {name}")).color(strong))
                    .truncate(),
            );
            return tapped;
        }
        let group = playing::group_link(&self.play, &self.online);
        let (name, ms) = match group {
            Some((partners, slowest)) => (partners, slowest.map(u32::from)),
            None => (name, session.round_trip_ms()),
        };
        let inviting = session.stage() == Stage::Inviting;
        let link = ms.filter(|_| !inviting).map(|ms| {
            let (word, color) = playing::quality(ms, ui.visuals().dark_mode);
            let measure = |text: &str, font: egui::FontId| {
                ui.painter()
                    .layout_no_wrap(text.to_owned(), font, strong)
                    .size()
                    .x
            };
            let name_width = measure(&name, egui::FontId::proportional(16.0));
            let room = ui.available_width() - name_width - ui.spacing().item_spacing.x;
            let body = egui::TextStyle::Body.resolve(ui.style());
            let (link, time) = link_parts(word, ms, room, |text| measure(text, body.clone()));
            (link, time, color, ms)
        });
        ui.add(egui::Label::new(RichText::new(&name).size(16.0).color(strong)).truncate());
        if inviting {
            ui.label(RichText::new("Invite sent").weak());
        } else if let Some((link, time, color, ms)) = link {
            let body = egui::TextStyle::Body.resolve(ui.style());
            let format = |color: egui::Color32| egui::TextFormat {
                font_id: body.clone(),
                color,
                ..Default::default()
            };
            let mut job = egui::text::LayoutJob::default();
            job.append(&link, 0.0, format(color));
            if let Some(time) = time {
                job.append(&time, 0.0, format(ui.visuals().text_color()));
            }
            ui.label(job).on_hover_text(format!("{ms} ms"));
        } else {
            ui.label(RichText::new("Connecting").weak());
        }
        tapped
    }

    fn strip_button(&self, ui: &mut Ui, button: StripButton) -> bool {
        let side = ui.spacing().interact_size.y.max(STRIP_BUTTON);
        let (icon, hint) = match button {
            StripButton::Hide => (icons::EYE_SLASH, "Hide the UI"),
            StripButton::FastForward => (icons::FAST_FORWARD, "Fast forward"),
            StripButton::Pause if self.emulator.running => (icons::PAUSE, "Pause"),
            StripButton::Pause => (icons::PLAY, "Resume"),
            StripButton::SaveState => (icons::FLOPPY_DISK, "Save state"),
            StripButton::Chat => (icons::CHAT_CIRCLE, "Chat"),
            StripButton::Switch => (icons::SWAP, "Switch game"),
            StripButton::StopRecording => (icons::STOP, "Stop recording"),
            StripButton::More => (icons::DOTS_THREE_VERTICAL, "More"),
        };
        let on = button == StripButton::FastForward && self.emulator.fast_forward;
        let mut text = RichText::new(icon).size(21.0);
        if on {
            text = text.color(crate::app::ui_state::ui_theme::ink_on(
                ui.visuals().selection.bg_fill,
            ));
        } else if button == StripButton::StopRecording {
            text = text.color(ui.visuals().error_fg_color);
        }
        let mut widget = egui::Button::new(text)
            .min_size(vec2(side, side))
            .corner_radius(side / 2.0);
        widget = if on {
            widget.fill(ui.visuals().selection.bg_fill)
        } else {
            widget.frame(false)
        };
        let response = ui.add(widget).on_hover_text(hint);
        if button == StripButton::Chat
            && let Some(session) = self.play.session()
            && self.online.unread(session.friend()) > 0
        {
            avatar::status_dot(ui, response.rect.shrink(6.0));
        }
        response.clicked()
    }

    fn press_strip(&mut self, button: StripButton) {
        match button {
            StripButton::Hide => self.mobile.hidden.hide(),
            StripButton::FastForward => {
                self.emulator.fast_forward = !self.emulator.fast_forward;
            }
            StripButton::Pause => self.run_hotkey(HotkeyAction::TogglePause),
            StripButton::SaveState => self.save_state_with_undo(),
            StripButton::Chat => {
                if let Some(session) = self.play.session() {
                    let friend = session.friend();
                    self.ui.friends.chat.open(friend);
                    self.mobile.nav.go(super::nav::Place::Friends);
                }
            }
            StripButton::Switch => self.run_hotkey(HotkeyAction::SwitchGame),
            StripButton::StopRecording => self.run_hotkey(HotkeyAction::ToggleRecording),
            StripButton::More => self.open_more(),
        }
        self.release_touch();
    }
}

fn link_parts(
    word: &str,
    ms: u32,
    room: f32,
    width: impl Fn(&str) -> f32,
) -> (String, Option<String>) {
    let link = format!("{word} link");
    let time = format!(" \u{b7} {ms} ms");
    let fits = width(&format!("{link}{time}")) <= room;
    (link, fits.then_some(time))
}

#[cfg(test)]
mod tests;
