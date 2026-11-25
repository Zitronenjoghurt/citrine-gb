use super::{Sheet, label, opens, settled, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::audio::{DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use egui::{Context, Ui};

const LATENCY_NOTE: &str = "Lower reacts faster. Higher crackles less.";

fn milliseconds(value: f64) -> String {
    format!("{value:.0} ms")
}

impl Citrine {
    pub(super) fn sound_page(&mut self, ui: &mut Ui) {
        let waiting = self.audio.as_ref().is_some_and(|audio| !audio.is_running());
        self.tree_page(
            ui,
            SettingsTab::Sound,
            |_| None,
            Self::sound_row,
            |page| {
                if waiting {
                    page.note("Sound starts when you load a game or tap the screen.");
                }
            },
        );
    }

    fn sound_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &mut self.ui.settings;
        match row {
            RowId::Volume => opens(
                list,
                Row::new(label(row)).value(sheet::percent(s.volume.into())),
                Sheet::Volume,
            ),
            RowId::Mute => {
                s.dirty |= list.switch(SwitchRow::new(label(row), &mut s.muted));
                None
            }
            #[cfg(not(target_arch = "wasm32"))]
            RowId::OutputDevice => opens(
                list,
                Row::new(label(row)).value(device_name(s)),
                Sheet::Device,
            ),
            RowId::Latency => opens(
                list,
                Row::new(label(row)).value(milliseconds(s.audio_queue_ms.into())),
                Sheet::Latency,
            ),
            RowId::SlowSound => opens(
                list,
                Row::new(label(row)).value(s.slow_sound.to_string()),
                Sheet::Slowdown,
            ),
            RowId::FastForwardSound => opens(
                list,
                Row::new(label(row)).value(s.fast_forward_sound.to_string()),
                Sheet::FastForwardSound,
            ),
            _ => None,
        }
    }

    pub(super) fn sound_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let s = &mut self.ui.settings;
        match open {
            Sheet::Volume => {
                let shown = slide(
                    ctx,
                    label(RowId::Volume),
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
                    label(RowId::Latency),
                    Some(LATENCY_NOTE),
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
                    label(RowId::OutputDevice),
                    Some("Changing this reopens the sound device."),
                    0..names.len(),
                    &mut index,
                    |index| {
                        names[index]
                            .clone()
                            .unwrap_or_else(|| crate::audio::SAME_AS_SYSTEM.to_owned())
                    },
                );
                if changed {
                    s.audio_device = names[index].clone();
                    s.dirty = true;
                }
                done
            }
            Sheet::FastForwardSound => {
                let (changed, done) = super::pick(
                    ctx,
                    label(RowId::FastForwardSound),
                    None,
                    &mut s.fast_forward_sound,
                );
                s.dirty |= changed;
                done
            }
            _ => {
                let (changed, done) =
                    super::pick(ctx, label(RowId::SlowSound), None, &mut s.slow_sound);
                s.dirty |= changed;
                done
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn device_name(s: &Settings) -> String {
    s.audio_device
        .clone()
        .unwrap_or_else(|| crate::audio::SAME_AS_SYSTEM.to_owned())
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::files::tests::Opened;
    use crate::app::tests::{play_frame, text_rect};
    use crate::app::ui_state::settings_tree::SettingsTab;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use egui::Context;

    #[test]
    fn the_phone_sound_page_says_how_slowdown_sounds() {
        let mut opened = Opened::new("phone-slowdown-row");
        opened.app.mobile.nav.open_settings(SettingsTab::Sound);
        let ctx = Context::default();
        play_frame(&mut opened.app, &ctx, Vec::new());
        let output = play_frame(&mut opened.app, &ctx, Vec::new());
        assert!(text_rect(&output, super::label(RowId::SlowSound)).is_some());
        assert!(text_rect(&output, "Normal pitch").is_some());
        assert!(text_rect(&output, super::label(RowId::GbNoise)).is_none());
    }
}
