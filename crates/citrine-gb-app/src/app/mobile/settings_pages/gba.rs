use super::{Sheet, label, opens, pick_from};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::percent;
use crate::app::ui_state::settings::{GBA_SCREEN_NOTE, GbaScreen};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::emulator::gimmicks::LightSource;
use egui::{Context, Ui};

const CHANNELS_NOTE: &str = "The Game Boy Advance's six sound channels. The first four are the Game Boy's. Direct sound A and B play recorded sound, often the music and voices.";

fn channel(row: RowId) -> Option<usize> {
    match row {
        RowId::GbaPulse1 => Some(0),
        RowId::GbaPulse2 => Some(1),
        RowId::GbaWave => Some(2),
        RowId::GbaNoise => Some(3),
        RowId::DirectSoundA => Some(4),
        RowId::DirectSoundB => Some(5),
        _ => None,
    }
}

impl Citrine {
    pub(super) fn gba_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::Gba,
            |group| (group == GroupId::GbaChannels).then_some(CHANNELS_NOTE),
            Self::gba_row,
            |_| {},
        );
    }

    fn gba_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let light_sensor = self.senses.has_light_sensor();
        let s = &mut self.ui.settings;
        let sensor_on = light_sensor && s.gimmicks.light_source == LightSource::Sensor;
        match row {
            #[cfg(feature = "gba")]
            RowId::GbaBios => self.file_row(list, crate::app::system_files::SystemFile::GbaBios),
            #[cfg(feature = "nds")]
            RowId::GbaJit => {
                s.dirty |= list.switch(
                    SwitchRow::new(label(row), s.gba_jit_mut())
                        .info(crate::app::ui_state::settings::GBA_JIT_HINT),
                );
                None
            }
            #[cfg(feature = "nds")]
            RowId::GbaCompiledCode => {
                let kind = crate::storage::kinds::Kind::CompiledCode;
                if let Some(clear) =
                    super::storage::storage_row(list, &self.ui.storage, kind, None, false)
                {
                    self.ui.storage.request = Some(clear);
                }
                None
            }
            RowId::GbaScreen => opens(
                list,
                Row::new(label(row)).value(s.gba_screen().to_string()),
                Sheet::GbaScreen,
            ),
            RowId::LightSensor if light_sensor => {
                let mut sensor = sensor_on;
                if list.switch(SwitchRow::new(label(row), &mut sensor)) {
                    s.gimmicks.light_source = if sensor {
                        LightSource::Sensor
                    } else {
                        LightSource::Slider
                    };
                    s.dirty = true;
                }
                None
            }
            RowId::Sunlight if !sensor_on => opens(
                list,
                Row::new(label(row)).value(percent(s.gimmicks.light.into())),
                Sheet::Sunlight,
            ),
            _ => {
                if let Some(index) = channel(row) {
                    s.dirty |= list.switch(SwitchRow::new(label(row), &mut s.gba_channels[index]));
                }
                None
            }
        }
    }

    pub(super) fn gba_sheet(&mut self, ctx: &Context) -> bool {
        let s = &mut self.ui.settings;
        let mut value = s.gba_screen().0;
        let (changed, done) = pick_from(
            ctx,
            label(RowId::GbaScreen),
            Some(GBA_SCREEN_NOTE),
            GbaScreen::ALL.map(|screen| screen.0),
            &mut value,
            |value| GbaScreen(value).to_string(),
        );
        if changed {
            s.gba_color_correction = Some(value);
            s.dirty = true;
        }
        done
    }
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::files::tests::Opened;
    use crate::app::tests::{play_frame, text_rect};
    use crate::app::ui_state::settings_tree::SettingsTab;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use egui::Context;

    #[test]
    fn the_phone_gba_page_lists_all_six_channels_with_no_game() {
        let mut opened = Opened::new("phone-gba-channels");
        opened.app.mobile.nav.open_settings(SettingsTab::Gba);
        let ctx = Context::default();
        play_frame(&mut opened.app, &ctx, Vec::new());
        let output = play_frame(&mut opened.app, &ctx, Vec::new());
        for row in [
            RowId::GbaPulse1,
            RowId::GbaNoise,
            RowId::DirectSoundA,
            RowId::DirectSoundB,
        ] {
            assert!(text_rect(&output, super::label(row)).is_some(), "{row:?}");
        }
    }
}
