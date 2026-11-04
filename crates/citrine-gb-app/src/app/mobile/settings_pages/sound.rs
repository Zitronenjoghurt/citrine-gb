use super::{Sheet, settled, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::settings::Settings;
use crate::audio::{DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use egui::{Context, RichText, Ui};

const CHANNELS: [&str; 6] = [
    "Pulse with sweep",
    "Pulse",
    "Wave",
    "Noise",
    "Direct Sound A",
    "Direct Sound B",
];

pub fn channels_value(channels: &[bool]) -> String {
    let on: Vec<String> = channels
        .iter()
        .enumerate()
        .filter(|(_, on)| **on)
        .map(|(index, _)| (index + 1).to_string())
        .collect();
    match on.len() {
        n if n == channels.len() => "All".to_owned(),
        0 => "None".to_owned(),
        _ => on.join(", "),
    }
}

fn milliseconds(value: f64) -> String {
    format!("{value:.0} ms")
}

impl Citrine {
    pub(super) fn sound_page(&mut self, ui: &mut Ui) {
        let count = self.emulator.channel_count();
        let s = &mut self.ui.settings;
        let mut open = None;
        let mut dirty = false;
        ui.add_space(8.0);
        kit::list(ui, |list| {
            if list
                .row(Row::new("Volume").value(sheet::percent(s.volume.into())))
                .clicked()
            {
                open = Some(Sheet::Volume);
            }
            dirty |= list.switch(SwitchRow::new("Mute", &mut s.muted));
        });
        ui.add_space(12.0);
        kit::list(ui, |list| {
            if list
                .row(Row::new("Latency").value(milliseconds(s.audio_queue_ms.into())))
                .clicked()
            {
                open = Some(Sheet::Latency);
            }
            if list
                .row(Row::new("Channels").value(channels_value(s.channels_for(count))))
                .clicked()
            {
                open = Some(Sheet::Channels);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if list.row(Row::new("Output").value(device_name(s))).clicked() {
                open = Some(Sheet::Device);
            }
        });
        s.dirty |= dirty;
        if let Some(audio) = self.audio.as_ref()
            && !audio.is_running()
        {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("Sound starts when you load a game or tap the screen.")
                            .size(13.0)
                            .weak(),
                    )
                    .wrap(),
                );
            });
        }
        self.open_sheet(open);
    }

    pub(super) fn sound_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let count = self.emulator.channel_count();
        let s = &mut self.ui.settings;
        match open {
            Sheet::Volume => {
                let shown = slide(
                    ctx,
                    "Volume",
                    None,
                    Amount {
                        value: &mut s.volume,
                        range: 0.0..=1.0,
                        step: 0.01,
                        default: Settings::DEFAULT_VOLUME,
                        text: sheet::percent,
                    },
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::Latency => {
                let shown = slide(
                    ctx,
                    "Latency",
                    Some("Lower reacts faster. Higher crackles less."),
                    Amount {
                        value: &mut s.audio_queue_ms,
                        range: MIN_LATENCY_MS..=MAX_LATENCY_MS,
                        step: 5.0,
                        default: DEFAULT_LATENCY_MS,
                        text: milliseconds,
                    },
                );
                if settled(&shown.inner) {
                    s.dirty = true;
                }
                shown.closed
            }
            #[cfg(not(target_arch = "wasm32"))]
            Sheet::Device => {
                let names: Vec<Option<String>> = std::iter::once(None)
                    .chain(crate::audio::Audio::output_devices().into_iter().map(Some))
                    .collect();
                let current = names
                    .iter()
                    .position(|name| *name == s.audio_device)
                    .unwrap_or(0);
                let mut index = current;
                let (changed, done) = super::pick_from(
                    ctx,
                    "Output",
                    Some("Changing this reopens the sound device."),
                    0..names.len(),
                    &mut index,
                    |index| {
                        names[index]
                            .clone()
                            .unwrap_or_else(|| "System default".to_owned())
                    },
                );
                if changed {
                    s.audio_device = names[index].clone();
                    s.dirty = true;
                }
                done
            }
            _ => {
                let shown = sheet::show(ctx, super::sheet_id(), "Channels", |ui| {
                    let mut changed = false;
                    kit::list(ui, |list| {
                        for (index, on) in s.channels_for(count).iter_mut().enumerate() {
                            let label = format!("{} {}", index + 1, CHANNELS[index]);
                            changed |= list.switch(SwitchRow::new(&label, on));
                        }
                    });
                    changed
                });
                s.dirty |= shown.inner;
                shown.closed
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn device_name(s: &Settings) -> String {
    s.audio_device
        .clone()
        .unwrap_or_else(|| "System default".to_owned())
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::files::tests::Opened;
    use crate::app::tests::{play_frame, text_rect};
    use crate::app::ui_state::settings::SettingsTab;
    use egui::Context;

    #[test]
    fn the_phone_sound_page_counts_the_gba_channels() {
        let mut opened = Opened::new("phone-channels");
        let file = opened.picked();
        opened.app.handle_load_rom(file);
        opened.app.ui.settings.gba_channels[4] = false;
        opened.app.mobile.nav.open_settings(SettingsTab::Sound);
        let ctx = Context::default();
        play_frame(&mut opened.app, &ctx, Vec::new());
        let output = play_frame(&mut opened.app, &ctx, Vec::new());
        assert!(text_rect(&output, "1, 2, 3, 4, 6").is_some());
    }
}
