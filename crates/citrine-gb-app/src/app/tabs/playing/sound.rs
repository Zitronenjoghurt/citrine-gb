use super::bar::MIN_SHOWN;
use crate::icons;
use crate::online::OnlineSettings;
use egui::Ui;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FriendSound {
    pub volume: f32,
    pub muted: bool,
}

impl FriendSound {
    pub fn of(settings: &OnlineSettings) -> Self {
        Self {
            volume: settings.friend_volume,
            muted: settings.friend_muted,
        }
    }

    pub fn store(self, settings: &mut OnlineSettings) {
        settings.friend_volume = self.volume;
        settings.friend_muted = self.muted;
    }

    fn silent(self) -> bool {
        self.muted || self.volume == 0.0
    }

    pub(super) fn toggled(self) -> Self {
        if !self.silent() {
            return Self {
                muted: true,
                ..self
            };
        }
        let volume = if self.volume == 0.0 {
            OnlineSettings::DEFAULT_FRIEND_VOLUME
        } else {
            self.volume
        };
        Self {
            volume,
            muted: false,
        }
    }
}

pub fn friend_sound(ui: &mut Ui, sound: FriendSound) -> Option<FriendSound> {
    let side = ui.spacing().interact_size.y;
    let (icon, hint) = if sound.silent() {
        (icons::SPEAKER_SLASH, "Sound on")
    } else {
        (icons::SPEAKER_HIGH, "Mute")
    };
    let mut changed = None;
    if ui
        .add(egui::Button::new(icon).min_size(egui::Vec2::splat(side)))
        .on_hover_text(hint)
        .clicked()
    {
        changed = Some(sound.toggled());
    }
    let mut volume = if sound.muted { 0.0 } else { sound.volume };
    let width = ui.available_width().min(ui.spacing().slider_width);
    if width < MIN_SHOWN {
        return changed;
    }
    ui.spacing_mut().slider_width = width;
    let slider = ui.add(
        egui::Slider::new(&mut volume, 0.0..=1.0)
            .show_value(false)
            .trailing_fill(true),
    );
    if slider.changed() {
        changed = Some(FriendSound {
            volume,
            muted: false,
        });
    }
    changed
}

pub fn friend_sound_bar(ui: &mut Ui, sound: FriendSound) -> Option<FriendSound> {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(ui.spacing().interact_size.y);
                friend_sound(ui, sound)
            })
            .inner
        })
        .inner
}
