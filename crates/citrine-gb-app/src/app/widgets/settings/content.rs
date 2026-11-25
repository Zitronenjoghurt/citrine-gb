use crate::app::big_screen::scroll::Following;
use crate::app::events::AppEventQueue;
use crate::app::ui_state::keybinds::BindingCapture;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab, Surface};
use crate::app::widgets::info::InfoState;
use crate::app::widgets::settings::form::{Form, RowTag};
use crate::app::widgets::settings::page::Page;
use crate::app::widgets::settings::palette::PaletteEditorState;
use crate::app::widgets::settings::scope::{self, View};
use crate::app::widgets::settings::{online, pages, storage, your_data};
use crate::audio::Audio;
use egui::{Response, ScrollArea, Ui, Widget};

pub struct SettingsContent<'a> {
    pub(super) settings: &'a mut Settings,
    pub(super) capture: &'a mut BindingCapture,
    pub(super) palette_editor: &'a mut PaletteEditorState,
    pub(super) audio: Option<&'a Audio>,
    pub(super) device: Device<'a>,
    pub(super) boot_roms: [Option<&'a str>; 2],
    #[cfg(feature = "gba")]
    pub(super) gba_bios: Option<&'a str>,
    #[cfg(feature = "nds")]
    pub(super) ds: super::ds::DsState<'a>,
    pub(super) events: Option<&'a mut AppEventQueue>,
    pub(super) online: Option<(&'a mut crate::online::Online, &'a mut online::JoinForm)>,
    pub(super) your_data: Option<&'a mut your_data::YourDataUi>,
    pub(super) storage: Option<&'a mut storage::StorageUi>,
    pub(super) info: Option<&'a mut InfoState>,
    top_space: f32,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub(super) discord: Option<(
        Option<crate::discord::GameNow>,
        Option<egui::TextureId>,
        bool,
    )>,
}

#[derive(Clone, Copy, Default)]
pub struct Device<'a> {
    pub light_sensor: bool,
    pub live_camera: bool,
    pub camera_error: Option<&'a str>,
}

impl<'a> SettingsContent<'a> {
    pub fn new(
        settings: &'a mut Settings,
        capture: &'a mut BindingCapture,
        palette_editor: &'a mut PaletteEditorState,
        audio: Option<&'a Audio>,
        events: Option<&'a mut AppEventQueue>,
    ) -> Self {
        Self {
            settings,
            capture,
            palette_editor,
            audio,
            device: Device::default(),
            boot_roms: [None; 2],
            #[cfg(feature = "gba")]
            gba_bios: None,
            #[cfg(feature = "nds")]
            ds: super::ds::DsState {
                account: None,
                files: [None; 3],
                mic: None,
            },
            events,
            online: None,
            your_data: None,
            storage: None,
            info: None,
            top_space: 0.0,
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            discord: None,
        }
    }

    pub fn device(mut self, device: Device<'a>) -> Self {
        self.device = device;
        self
    }

    pub fn boot_roms(mut self, names: [Option<&'a str>; 2]) -> Self {
        self.boot_roms = names;
        self
    }

    #[cfg(feature = "gba")]
    pub fn gba_bios(mut self, name: Option<&'a str>) -> Self {
        self.gba_bios = name;
        self
    }

    #[cfg(feature = "nds")]
    pub fn ds(mut self, state: super::ds::DsState<'a>) -> Self {
        self.ds = state;
        self
    }

    pub fn top_space(mut self, space: f32) -> Self {
        self.top_space = space;
        self
    }

    pub fn storage(mut self, state: &'a mut storage::StorageUi) -> Self {
        self.storage = Some(state);
        self
    }

    pub fn your_data(mut self, state: &'a mut your_data::YourDataUi) -> Self {
        self.your_data = Some(state);
        self
    }

    pub fn info(mut self, state: &'a mut InfoState) -> Self {
        self.info = Some(state);
        self
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    pub fn discord(
        mut self,
        game: Option<crate::discord::GameNow>,
        screen: Option<egui::TextureId>,
        pictures_off: bool,
    ) -> Self {
        self.discord = Some((game, screen, pictures_off));
        self
    }

    pub fn online(
        mut self,
        online: &'a mut crate::online::Online,
        form: &'a mut online::JoinForm,
    ) -> Self {
        self.online = Some((online, form));
        self
    }
}

impl Widget for SettingsContent<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        if let Some(row) = self.settings.find.take() {
            super::spotlight::aim(ui.ctx(), label(row));
        }
        if let Some(group) = self.settings.find_group.take() {
            super::spotlight::aim_heading(ui.ctx(), group.title());
        }
        let margin = ui.visuals().clip_rect_margin;
        let top_space = self.top_space;
        ui.scope(|ui| {
            ui.visuals_mut().clip_rect_margin = 0.0;
            ScrollArea::vertical()
                .id_salt("settings_content_scroll")
                .auto_shrink([false, true])
                .show_following(ui, |ui| {
                    ui.visuals_mut().clip_rect_margin = margin;
                    ui.add_space(top_space);
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(ui.available_width().min(MAX_COLUMN_WIDTH));
                        super::form::shared_labels(ui, |ui| match self.settings.current_tab {
                            SettingsTab::Screen => pages::screen::show(self, ui),
                            SettingsTab::Sound => pages::sound::show(self, ui),
                            SettingsTab::Controls => pages::controls::show(self, ui),
                            SettingsTab::Playing => pages::playing::show(self, ui),
                            SettingsTab::Capture => pages::capture::show(self, ui),
                            SettingsTab::GameBoy => pages::game_boy::show(self, ui),
                            SettingsTab::Gba => pages::gba::show(self, ui),
                            SettingsTab::Ds => pages::ds::show(self, ui),
                            SettingsTab::App => pages::app::show(self, ui),
                            SettingsTab::Online => self.online_page(ui),
                            SettingsTab::Sync => pages::sync::show(self, ui),
                            SettingsTab::Storage => self.storage_page(ui),
                            SettingsTab::About => pages::about::show(self, ui),
                            SettingsTab::Developer => pages::developer::show(self, ui),
                        })
                    })
                    .response
                })
                .inner
        })
        .inner
    }
}

impl SettingsContent<'_> {
    fn online_page(self, ui: &mut Ui) {
        let mut spare = online::JoinForm::default();
        match self.online {
            Some((client, form)) => online::show(ui, self.settings, Some(client), form),
            None => online::show(ui, self.settings, None, &mut spare),
        }
    }

    fn storage_page(self, ui: &mut Ui) {
        let mut spare = storage::StorageUi::default();
        let mut spare_data = your_data::YourDataUi::default();
        let state = self.storage.unwrap_or(&mut spare);
        let data = self.your_data.unwrap_or(&mut spare_data);
        storage::show(ui, state, data);
    }
}

pub(super) const MAX_COLUMN_WIDTH: f32 = 480.0;

pub(super) fn label(row: RowId) -> &'static str {
    row.label(Surface::Desktop)
}

pub(super) fn drawn(row: RowId, dev_mode: bool) -> bool {
    row.shown_on(Surface::Desktop, dev_mode)
}

pub(super) fn rows(group: GroupId, view: View) -> impl Iterator<Item = RowId> {
    group
        .rows()
        .filter(move |row| drawn(*row, view.dev_mode) && view.row(*row))
}

pub(super) fn groups(tab: SettingsTab, view: View) -> impl Iterator<Item = GroupId> {
    tab.groups_on(Surface::Desktop, view.dev_mode)
        .filter(move |group| view.group(*group))
}

pub(super) fn view(s: &Settings, tab: SettingsTab) -> View {
    View::of(s, tab, Surface::Desktop)
}

pub(super) fn scope_bar(page: &mut Page, s: &mut Settings, view: View) {
    if !view.bar {
        return;
    }
    let mut scope = s.scope();
    let title = scope::game_title(s).to_owned();
    let game = format!("{} {title}", scope::MARK);
    page.plain(|ui| {
        scope::buttons(
            ui,
            &mut scope,
            (Scope::AllGames, scope::ALL_GAMES),
            (Scope::ThisGame, &game),
            &title,
        );
    });
    s.set_scope(scope);
}

pub(super) fn hidden_rows(
    page: &mut Page,
    s: &mut Settings,
    tab: SettingsTab,
    view: View,
    mut row_ui: impl FnMut(&mut Form, &mut Settings, GroupId, RowId),
) {
    if !view.game_only {
        return;
    }
    page.unseen(|ui| {
        Form::show(ui, "hidden_rows", |form| {
            for group in tab.groups_on(Surface::Desktop, view.dev_mode) {
                for row in group
                    .rows()
                    .filter(|row| drawn(*row, view.dev_mode) && !view.row(*row))
                {
                    row_ui(form, s, group, row);
                }
            }
        });
    });
}

pub(super) fn tagged(
    form: &mut Form,
    s: &mut Settings,
    view: View,
    row: RowId,
    add: impl FnOnce(&mut Form, &mut Settings),
) {
    let tag = view.tag(s, row).map(|tag| RowTag {
        hover: tag.hover(scope::game_title(s)),
        tag,
    });
    if form.with_tag(tag, |form| add(form, s)) {
        s.drop_later(row, view.surface);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::ds_screens::Arrangement;
    use crate::app::ui_state::settings::ScalingMode;
    use crate::app::widgets::settings::pages::app::LAYOUT_HINT;
    use crate::app::widgets::settings::pages::game_boy::CHANNELS_HINT;
    use crate::app::widgets::settings::pages::playing::AUTOSAVE_NOTE;

    fn render(settings: Settings) -> Vec<(egui::Rect, String)> {
        render_with(settings, Device::default())
    }

    fn render_with(settings: Settings, device: Device) -> Vec<(egui::Rect, String)> {
        render_at(settings, device, 800.0)
    }

    fn render_at(mut settings: Settings, device: Device, width: f32) -> Vec<(egui::Rect, String)> {
        let ctx = egui::Context::default();
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None)
                    .device(device)
                    .ui(ui);
            });
        });
        output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.rect.translate(text.pos.to_vec2()),
                    text.galley.text().trim().to_owned(),
                )),
                _ => None,
            })
            .collect()
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_microphone_row_shows_the_level_while_the_ds_page_is_open() {
        use crate::app::ui_state::ui_theme;
        use crate::app::widgets::mic_level::METER_HEIGHT;
        use crate::audio::mic::{MicTest, Microphone, OpenError};

        let draw = |tab: SettingsTab, microphone: Microphone, mic: Option<MicTest>| {
            let mut settings = Settings {
                current_tab: tab,
                microphone,
                ..Settings::default()
            };
            let ctx = egui::Context::default();
            let mut capture = BindingCapture::default();
            let mut editor = PaletteEditorState::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 4000.0),
                )),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None)
                        .ds(super::super::ds::DsState {
                            account: None,
                            files: [None; 3],
                            mic,
                        })
                        .ui(ui);
                });
            });
            let lit_fill = ui_theme::meter(&ctx.style().visuals);
            let segments: Vec<egui::Color32> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.rect.height() == METER_HEIGHT => {
                        Some(rect.fill)
                    }
                    _ => None,
                })
                .collect();
            let lit = segments.iter().filter(|fill| **fill == lit_fill).count();
            let bars = (segments.len(), lit);
            let texts: Vec<String> = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                    _ => None,
                })
                .collect();
            (settings.mic_test_shown, bars, texts)
        };

        let (asked, base, _) = draw(SettingsTab::Ds, Microphone::Off, None);
        assert!(asked, "picking a microphone here starts the test at once");
        assert_eq!(base, (0, 0), "no meter while the microphone is off");

        let (asked, speaking, _) = draw(
            SettingsTab::Ds,
            Microphone::Default,
            Some(MicTest::Level(0.5)),
        );
        assert!(asked);
        assert_eq!(speaking, (12, 6), "twelve segments, half of them lit");

        let (_, silent, _) = draw(SettingsTab::Ds, Microphone::Default, None);
        assert_eq!(silent, (12, 0), "an unlit meter before the first reading");

        let missing = MicTest::Problem(OpenError::NoDevice);
        let (_, bars, texts) = draw(SettingsTab::Ds, Microphone::Default, Some(missing));
        assert_eq!(bars, base);
        assert!(
            texts
                .iter()
                .any(|text| Some(text.as_str()) == missing.words())
        );

        let (asked, gba, _) = draw(SettingsTab::Gba, Microphone::Default, None);
        assert!(!asked, "another page stops the test");
        assert_eq!(gba, draw(SettingsTab::Gba, Microphone::Off, None).1);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn compiled_game_code_sits_under_each_jit_switch_and_on_storage_only() {
        if !citrine_gb::nds::machine::JIT_AVAILABLE {
            return;
        }
        let clear_label = label(RowId::CompiledCode);
        assert_eq!(
            clear_label,
            crate::storage::kinds::Kind::CompiledCode.name()
        );
        assert_eq!(label(RowId::GbaCompiledCode), clear_label);
        for tab in SettingsTab::iter_with_dev_mode(true) {
            let texts = render(Settings {
                current_tab: tab,
                dev_mode: true,
                ..Settings::default()
            });
            let clear: Vec<_> = texts
                .iter()
                .filter(|(_, text)| text.ends_with(clear_label))
                .collect();
            if tab == SettingsTab::Storage {
                assert_eq!(clear.len(), 1, "{clear:?}");
                continue;
            }
            let (switch, next) = match tab {
                SettingsTab::Ds => (RowId::Jit, Some(RowId::Arm9Bios)),
                SettingsTab::Gba => (RowId::GbaJit, None),
                _ => {
                    assert!(clear.is_empty(), "{tab:?}: {clear:?}");
                    continue;
                }
            };
            let top = |wanted: &str| {
                texts
                    .iter()
                    .find(|(_, text)| text == wanted)
                    .map(|(rect, _)| rect.top())
                    .unwrap_or_else(|| panic!("{wanted} missing: {texts:?}"))
            };
            assert_eq!(clear.len(), 1, "{tab:?}: {clear:?}");
            let clear = clear[0].0.top();
            assert!(top(label(switch)) < clear, "{tab:?}");
            if let Some(next) = next {
                assert!(clear < top(label(next)), "{tab:?}");
            }
        }
        let texts = render_at(
            Settings {
                current_tab: SettingsTab::Ds,
                dev_mode: true,
                ..Settings::default()
            },
            Device::default(),
            310.0,
        );
        let height = |wanted: &str| {
            texts
                .iter()
                .find(|(_, text)| text == wanted)
                .map(|(rect, _)| rect.height())
                .unwrap_or_else(|| panic!("{wanted} missing: {texts:?}"))
        };
        assert!(
            height(clear_label) < 1.5 * height(label(RowId::Jit)),
            "{clear_label} wraps at 310 pt"
        );
    }

    #[test]
    fn every_system_file_row_has_its_info_mark() {
        use crate::app::system_files::SystemFile;
        for file in SystemFile::all() {
            let tab = match file {
                SystemFile::BootRom(_) => SettingsTab::GameBoy,
                #[cfg(feature = "gba")]
                SystemFile::GbaBios => SettingsTab::Gba,
                #[cfg(feature = "nds")]
                SystemFile::Ds(_) => SettingsTab::Ds,
            };
            let texts = render(Settings {
                current_tab: tab,
                ..Settings::default()
            });
            let wanted = label(file.row());
            let (row, _) = texts
                .iter()
                .find(|(_, text)| text == wanted)
                .unwrap_or_else(|| panic!("{wanted} missing: {texts:?}"));
            assert!(
                texts.iter().any(|(rect, text)| text == crate::icons::INFO
                    && rect.left() > row.left()
                    && (rect.center().y - row.center().y).abs() < row.height()),
                "{wanted} has no info mark on its line"
            );
        }
    }

    #[test]
    fn the_game_boy_palette_controls_start_at_the_select_edge() {
        let mut settings = Settings {
            current_tab: SettingsTab::GameBoy,
            ..Settings::default()
        };
        let palette = super::super::palette::choice_label(&settings.global_choice());
        let ctx = egui::Context::default();
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..SETTLE_RUNS {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None)
                            .ui(ui);
                    });
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.galley.text().trim().to_owned(),
                    )),
                    _ => None,
                })
                .collect();
        }
        let widgets: Vec<(egui::Rect, bool)> = ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .map(|w| (w.rect, w.sense.senses_click()))
                .collect()
        });
        let control_of = |wanted: &str| {
            let (text, _) = texts
                .iter()
                .find(|(_, text)| text.ends_with(wanted))
                .unwrap_or_else(|| panic!("{wanted} missing: {texts:?}"));
            widgets
                .iter()
                .filter(|(rect, click)| {
                    *click && rect.contains(text.center()) && rect.width() < 200.0
                })
                .map(|(rect, _)| *rect)
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap_or_else(|| panic!("no control holds {wanted}"))
        };
        let select = control_of("Automatic");
        for wanted in ["Like a GBC", palette.as_str(), "New palette", "Duplicate"] {
            let left = control_of(wanted).left();
            assert!(
                (left - select.left()).abs() < 0.5,
                "{wanted} starts at {left}, the selects at {}",
                select.left()
            );
        }
        let picker = control_of(palette.as_str()).right();
        assert!(
            (picker - select.right()).abs() < 0.5,
            "the palette picker ends at {picker}, the selects at {}",
            select.right()
        );
        let strip = widgets
            .iter()
            .find(|(rect, click)| {
                !click
                    && (rect.height() - 26.0).abs() < 0.5
                    && (rect.left() - select.left()).abs() < 0.5
            })
            .map(|(rect, _)| rect.right())
            .expect("the palette strip shows under the picker");
        assert!(
            (strip - select.right()).abs() < 0.5,
            "the palette strip ends at {strip}, the selects at {}",
            select.right()
        );
    }

    #[test]
    fn selects_show_their_whole_value_in_the_desktop_dock() {
        let on_tab = |tab| Settings {
            current_tab: tab,
            ..Settings::default()
        };
        #[allow(unused_mut)]
        let mut cases = vec![
            (on_tab(SettingsTab::GameBoy), "Automatic"),
            (on_tab(SettingsTab::App), "Automatic"),
        ];
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        {
            use crate::app::ui_state::ds_3d::{Resolution, TextureFilter};
            let mut ds = on_tab(SettingsTab::Ds);
            ds.ds_3d.resolution = Resolution::Double;
            ds.ds_3d.texture_filter = TextureFilter::Bilinear;
            cases.push((ds, "Bilinear"));
        }
        for (mut settings, shown) in cases {
            let tab = settings.current_tab;
            let ctx = egui::Context::default();
            let mut capture = BindingCapture::default();
            let mut editor = PaletteEditorState::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                ..Default::default()
            };
            let mut texts = Vec::new();
            let mut elided = Vec::new();
            for _ in 0..2 {
                let output = ctx.run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                            SettingsContent::new(
                                &mut settings,
                                &mut capture,
                                &mut editor,
                                None,
                                None,
                            )
                            .ui(ui);
                        });
                    });
                });
                let galleys = output
                    .shapes
                    .into_iter()
                    .filter_map(|clipped| match clipped.shape {
                        egui::Shape::Text(text) => Some(text.galley),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                texts = galleys
                    .iter()
                    .map(|galley| galley.text().trim().to_owned())
                    .collect::<Vec<_>>();
                elided = galleys
                    .iter()
                    .filter(|galley| galley.elided)
                    .map(|galley| galley.text().trim().to_owned())
                    .collect::<Vec<_>>();
            }
            assert!(texts.iter().any(|text| text == shown), "{tab:?}: {texts:?}");
            assert!(elided.is_empty(), "{tab:?} cut short: {elided:?}");
        }
    }

    #[test]
    fn the_selects_of_a_group_share_one_left_edge() {
        let ctx = egui::Context::default();
        let mut settings = Settings {
            current_tab: SettingsTab::GameBoy,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None)
                            .ui(ui);
                    });
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.galley.text().trim().to_owned(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let lefts = ["Automatic", "None", "Live camera"]
            .into_iter()
            .flat_map(|value| {
                texts
                    .iter()
                    .filter(move |(_, text)| text == value)
                    .map(|(rect, _)| rect.left())
            })
            .collect::<Vec<_>>();
        assert!(lefts.len() >= 3, "{texts:?}");
        assert!(
            lefts.iter().all(|left| (left - lefts[0]).abs() < 0.5),
            "ragged select edges {lefts:?}"
        );
    }

    fn texts_of(tab: SettingsTab) -> Vec<String> {
        render(Settings {
            current_tab: tab,
            dev_mode: true,
            ..Settings::default()
        })
        .into_iter()
        .map(|(_, text)| text)
        .collect()
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_birthday_spells_out_its_month_where_it_fits() {
        let texts = texts_of(SettingsTab::Ds);
        assert!(texts.iter().any(|text| text == "January"), "{texts:?}");
        assert!(!texts.iter().any(|text| text == "Jan"), "{texts:?}");
    }

    #[test]
    fn fractions_read_as_percent_with_no_space() {
        for tab in [
            SettingsTab::Screen,
            SettingsTab::Sound,
            SettingsTab::Gba,
            SettingsTab::App,
        ] {
            let texts = render(Settings {
                current_tab: tab,
                matrix: true,
                ghosting: true,
                ..Settings::default()
            })
            .into_iter()
            .map(|(_, text)| text)
            .collect::<Vec<_>>();
            let odd = texts
                .iter()
                .filter(|text| {
                    (text.contains('.') && text.parse::<f32>().is_ok()) || text.ends_with(" %")
                })
                .collect::<Vec<_>>();
            assert!(odd.is_empty(), "{tab:?} shows {odd:?}");
            assert!(
                texts.iter().any(|text| text.ends_with('%')),
                "{tab:?} shows no percent"
            );
        }
    }

    #[test]
    fn the_grid_note_quotes_the_same_size_label() {
        let note = crate::app::ui_state::settings::grid_note(
            ScalingMode::Fit,
            crate::app::ui_state::settings_tree::Surface::Desktop,
        );
        assert!(note.contains("\"Every pixel is the same size\""), "{note}");
        assert!(note.contains("\"Integer (sharpest)\""), "{note}");
    }

    #[test]
    fn no_page_repeats_its_own_name() {
        for tab in SettingsTab::iter_with_dev_mode(true) {
            let texts = texts_of(tab);
            assert!(!texts.is_empty(), "{} rendered nothing", tab.title());
            assert!(
                !texts.iter().any(|text| text == tab.title()),
                "{} shows its own name",
                tab.title()
            );
        }
    }

    #[test]
    fn explainers_stay_behind_their_marks() {
        let explainers = [
            AUTOSAVE_NOTE,
            LAYOUT_HINT,
            CHANNELS_HINT,
            "keeps settings in",
            "Minutes of play between autosaves",
            "Lower reacts faster",
            "Draws thin lines between",
            "The touch layout shows these",
            "Overwrite the starred snapshot slot",
            "Applies to every game",
        ];
        for tab in SettingsTab::iter_with_dev_mode(true) {
            for text in texts_of(tab) {
                assert!(
                    !explainers.iter().any(|explainer| text.contains(explainer)),
                    "{} shows {text:?} on the page",
                    tab.title()
                );
            }
        }
        let joined = render(joined_online());
        assert!(joined.iter().any(|(_, text)| text == "Relay ID"));
        for (_, text) in joined {
            assert!(
                !text.contains("Anyone with your relay ID"),
                "Online shows {text:?} on the page"
            );
        }
    }

    #[test]
    fn the_app_calls_it_relay_id_everywhere() {
        let old_names = [format!("sign-in {}", "key"), format!("sign in {}", "key")];
        let says_old_name = |text: &str| {
            let text = text.to_lowercase();
            old_names.iter().any(|old| text.contains(old.as_str()))
        };
        let mut pages: Vec<_> = SettingsTab::iter_with_dev_mode(true)
            .flat_map(texts_of)
            .collect();
        pages.extend(render(joined_online()).into_iter().map(|(_, text)| text));
        for text in pages {
            assert!(!says_old_name(&text), "Settings shows {text:?}");
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut folders = vec![
            crates.join("citrine-gb-app/src"),
            crates.join("citrine-gb-server/src"),
            crates.join("citrine-gb-net/src"),
        ];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).unwrap();
                    assert!(
                        !says_old_name(&source),
                        "{} says the old name",
                        path.display()
                    );
                }
            }
        }
    }

    fn joined_online() -> Settings {
        let mut settings = Settings {
            current_tab: SettingsTab::Online,
            ..Settings::default()
        };
        settings.online.remember(
            &crate::online::ServerAddress::parse("gb.lemon.industries").unwrap(),
            &citrine_gb_net::RelayId::random(),
            "pw",
            &citrine_gb_net::Account {
                online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
                display_name: "Clara".into(),
                storage_used: 0,
                quota_bytes: 0,
                two_factor: false,
                codes_left: 0,
            },
            &Default::default(),
        );
        settings
    }

    fn every_row(content: SettingsContent<'_>) -> SettingsContent<'_> {
        #[cfg(feature = "nds")]
        let content = content.ds(super::super::ds::DsState {
            account: None,
            files: [Some("bios9.bin"), Some("bios7.bin"), Some("firmware.bin")],
            mic: None,
        });
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        let content = content.discord(None, None, false);
        content.device(Device {
            light_sensor: true,
            live_camera: true,
            camera_error: None,
        })
    }

    const SETTLE_RUNS: usize = 5;

    fn laid_out(mut settings: Settings, width: f32) -> super::super::page::tests::Laid {
        use super::super::page::tests::{after_pass, quiet};
        let ctx = egui::Context::default();
        quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut laid = Default::default();
        for _ in 0..SETTLE_RUNS {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            laid = after_pass(&ctx);
        }
        laid
    }

    #[test]
    fn every_desktop_page_draws_its_rows_with_the_shared_widgets() {
        use super::super::page::tests::assert_unified;
        let page = |tab: Option<SettingsTab>| match tab {
            Some(tab) => Settings {
                current_tab: tab,
                dev_mode: true,
                matrix: true,
                ghosting: true,
                ..Settings::default()
            },
            None => joined_online(),
        };
        let tabs = SettingsTab::iter_with_dev_mode(true)
            .map(Some)
            .chain([None]);
        for tab in tabs {
            for width in [217.0, 800.0] {
                let (marks, controls) = laid_out(page(tab), width);
                let name = match tab {
                    Some(tab) => format!("{tab:?} at {width}"),
                    None => format!("Online signed in at {width}"),
                };
                assert_unified(&name, &marks, &controls);
            }
        }
        for width in [217.0, 800.0].into_iter().filter(|_| cfg!(feature = "nds")) {
            let (marks, controls) = laid_out(ds_style(false), width);
            assert_unified(&format!("DS screens at {width}"), &marks, &controls);
        }
    }

    fn ds_style(separate_tabs: bool) -> Settings {
        let mut settings = Settings {
            current_tab: SettingsTab::Ds,
            ..Settings::default()
        };
        settings.ds_screens.desktop.arrangement = Arrangement::BigAndSmall;
        settings.ds_screens.separate_tabs = separate_tabs;
        settings
    }

    #[cfg(all(
        feature = "nds",
        not(any(target_arch = "wasm32", target_os = "android"))
    ))]
    #[test]
    fn discord_offers_the_ds_screen_choice_whatever_the_game() {
        let label = label(RowId::DiscordDsScreens);
        let shown = |screenshot: bool| -> Vec<String> {
            let mut settings = Settings {
                current_tab: SettingsTab::App,
                ..Settings::default()
            };
            settings.discord.enabled = true;
            settings.discord.screenshot = screenshot;
            render(settings).into_iter().map(|(_, text)| text).collect()
        };
        let texts = shown(true);
        assert!(texts.iter().any(|text| text == label), "{texts:?}");
        let texts = shown(false);
        assert!(!texts.iter().any(|text| text == label), "{texts:?}");
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_ds_page_shows_its_screen_rows_with_no_game() {
        let texts = |settings: Settings| -> Vec<String> {
            render(settings).into_iter().map(|(_, text)| text).collect()
        };
        let ds = texts(ds_style(false));
        for row in [
            "Screens",
            "Screens in two tabs",
            "Small screen corner",
            "Small screen size",
            "Gap",
            "Rotation",
        ] {
            assert!(ds.iter().any(|text| text == row), "{row}: {ds:?}");
        }
        for gone in [
            label(RowId::GbcScreen),
            label(RowId::GbaScreen),
            label(RowId::Palette),
        ] {
            assert!(!ds.iter().any(|text| text == gone), "{gone}: {ds:?}");
        }
        let tabs = texts(ds_style(true));
        for gone in ["Small screen corner", "Gap"] {
            assert!(!tabs.iter().any(|text| text == gone), "{gone}: {tabs:?}");
        }
        assert!(tabs.iter().any(|text| text == "Rotation"), "{tabs:?}");
    }

    fn cut_short(mut settings: Settings, width: f32) -> Vec<String> {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut cut = Vec::new();
        for _ in 0..SETTLE_RUNS {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            cut = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.elided => {
                        Some(text.galley.text().trim().to_owned())
                    }
                    _ => None,
                })
                .collect();
        }
        cut
    }

    fn desktop_pages() -> impl Iterator<Item = (String, Settings)> {
        SettingsTab::iter_with_dev_mode(true)
            .map(|tab| {
                (
                    format!("{tab:?}"),
                    Settings {
                        current_tab: tab,
                        dev_mode: true,
                        matrix: true,
                        ghosting: true,
                        scaling: ScalingMode::Fit,
                        ds_3d: crate::app::ui_state::ds_3d::Ds3d {
                            resolution: crate::app::ui_state::ds_3d::Resolution::Double,
                            ..Default::default()
                        },
                        discord: crate::discord::DiscordSettings {
                            enabled: true,
                            screenshot: true,
                            ..Default::default()
                        },
                        gimmicks: crate::emulator::gimmicks::GimmickSettings {
                            light_source: crate::emulator::gimmicks::LightSource::Slider,
                            camera: crate::emulator::gimmicks::CameraSource::Picture,
                            ..Default::default()
                        },
                        ..Settings::default()
                    },
                )
            })
            .chain([("Online signed in".to_owned(), joined_online())])
    }

    fn info_mark_side() -> f32 {
        let ctx = egui::Context::default();
        let mut side = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                side = crate::app::widgets::info_mark::info_mark_width(ui)
                    - ui.spacing().item_spacing.x / 2.0;
            });
        });
        side
    }

    fn stacked_rows(marks: &super::super::page::tests::Laid) -> usize {
        let (marks, controls) = marks;
        let side = info_mark_side();
        marks
            .iter()
            .filter(|(kind, _)| *kind == super::super::page::Mark::Row)
            .filter(|(_, row)| {
                controls.iter().any(|control| {
                    control.width() > side + 0.5
                        && row.height() < 60.0
                        && row.contains(control.center())
                        && control.left() < row.left() + 20.0
                        && control.top() > row.top() + 12.0
                })
            })
            .count()
    }

    const LONG_VALUES: [&str; 4] = [
        "cgb1-",
        "Integer (sharpest)",
        "Beside the screen",
        "Your library",
    ];

    #[test]
    fn the_default_dock_keeps_each_label_beside_its_value() {
        for ((name, settings), (_, again)) in desktop_pages().zip(desktop_pages()) {
            let laid = laid_out(settings, 217.0);
            assert_eq!(stacked_rows(&laid), 0, "{name} stacks a row at 217 pt");
            let cut = cut_short(again, 217.0);
            assert!(
                cut.iter()
                    .all(|text| LONG_VALUES.iter().any(|long| text.starts_with(long))),
                "{name} cuts short at 217 pt: {cut:?}"
            );
        }
    }

    #[cfg(all(
        feature = "nds",
        not(any(target_arch = "wasm32", target_os = "android"))
    ))]
    fn text_galleys(mut settings: Settings, width: f32) -> Vec<std::sync::Arc<egui::Galley>> {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut galleys = Vec::new();
        for _ in 0..SETTLE_RUNS {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            galleys = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley),
                    _ => None,
                })
                .collect();
        }
        galleys
    }

    #[cfg(all(
        feature = "nds",
        not(any(target_arch = "wasm32", target_os = "android"))
    ))]
    #[test]
    fn the_ds_screen_rows_keep_one_line_and_their_whole_value_in_the_dock() {
        use crate::discord::{ShotInterval, ShotScreens};
        use strum::IntoEnumIterator;
        type Case = (ShotInterval, ShotScreens);
        let settings = |(every, screens): Case| {
            let mut settings = Settings {
                current_tab: SettingsTab::App,
                ..Settings::default()
            };
            settings.discord.enabled = true;
            settings.discord.screenshot = true;
            settings.discord.every = every;
            settings.discord.screens = screens;
            settings
        };
        let every_value = ShotInterval::iter()
            .flat_map(|every| ShotScreens::iter().map(move |screens| (every, screens)))
            .collect::<Vec<Case>>();
        let defaults = [(ShotInterval::default(), ShotScreens::default())];
        for (width, cases) in [(233.0, every_value), (217.0, defaults.to_vec())] {
            for case in cases {
                let labels = [label(RowId::DiscordEvery), label(RowId::DiscordDsScreens)];
                let galleys = text_galleys(settings(case), width);
                let shown = galleys
                    .iter()
                    .map(|galley| galley.text().trim())
                    .collect::<Vec<_>>();
                for row in &labels {
                    assert!(shown.contains(row), "{case:?} at {width}: no {row}");
                }
                let short = galleys
                    .iter()
                    .filter(|galley| {
                        let text = galley.text().trim();
                        (labels.contains(&text) && galley.rows.len() > 1)
                            || (galley.elided
                                && ["Both", "Top", "Bottom", "min"]
                                    .iter()
                                    .any(|value| text.contains(value)))
                    })
                    .map(|galley| galley.text().trim().to_owned())
                    .collect::<Vec<_>>();
                assert!(
                    short.is_empty(),
                    "{case:?} at {width} pt wraps or cuts {short:?}"
                );
            }
        }
    }

    fn label_lefts(mut settings: Settings, width: f32) -> Vec<(String, f32)> {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut lefts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            lefts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => {
                        Some((text.galley.text().trim().to_owned(), text.pos.x))
                    }
                    _ => None,
                })
                .collect();
        }
        lefts
    }

    const BOX_CHILDREN: [RowId; 6] = [
        RowId::LeftHanded,
        RowId::Haptics,
        RowId::DiscordGame,
        RowId::DiscordDetails,
        RowId::DiscordTime,
        RowId::DiscordShot,
    ];

    #[test]
    fn a_child_row_starts_where_its_checkbox_label_starts() {
        use strum::IntoEnumIterator;
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let step = ctx.style().spacing.icon_width + super::super::page::BOX_GAP;
        let mut found = Vec::new();
        for (name, settings) in desktop_pages() {
            let tab = settings.current_tab;
            let lefts = label_lefts(settings, 800.0);
            let left = |row: RowId| {
                lefts
                    .iter()
                    .find(|(text, _)| text == row.label(Surface::Desktop))
                    .map(|(_, left)| *left)
            };
            for row in RowId::iter().filter(|row| row.page() == tab) {
                let Some(parent) = row.parent() else {
                    continue;
                };
                assert_eq!(parent.page(), row.page(), "{row:?} and {parent:?}");
                assert!(parent < row, "{parent:?} comes after {row:?}");
                let (Some(child), Some(top)) = (left(row), left(parent)) else {
                    continue;
                };
                let boxed = if BOX_CHILDREN.contains(&row) {
                    step
                } else {
                    0.0
                };
                assert!(
                    (child - boxed - top).abs() < 0.5,
                    "{name}: {row:?} label at {child}, {parent:?} label at {top}, box step {boxed}; a new checkbox child joins BOX_CHILDREN"
                );
                found.push(row);
            }
        }
        assert!(
            found.len() >= 15,
            "only {} child rows found: {found:?}",
            found.len()
        );
    }

    const TOP_AND_CHILD: [(RowId, RowId); 3] = [
        (RowId::Scaling, RowId::GridLines),
        (RowId::StickDeadZone, RowId::TouchSize),
        (RowId::InputDelay, RowId::TypingText),
    ];

    #[test]
    fn a_top_level_label_starts_at_the_box_and_a_child_at_its_label() {
        let ctx = egui::Context::default();
        let step = ctx.style().spacing.icon_width + super::super::page::BOX_GAP;
        let mut found = Vec::new();
        for (name, settings) in desktop_pages() {
            let tab = settings.current_tab;
            let lefts = label_lefts(settings, 800.0);
            let left = |row: RowId| {
                lefts
                    .iter()
                    .find(|(text, _)| text == row.label(Surface::Desktop))
                    .map(|(_, left)| *left)
            };
            for (top, child) in TOP_AND_CHILD
                .into_iter()
                .filter(|(top, _)| top.page() == tab)
            {
                let (Some(top_left), Some(child_left)) = (left(top), left(child)) else {
                    continue;
                };
                assert!(
                    (child_left - top_left - step).abs() < 0.5,
                    "{name}: {top:?} label at {top_left}, {child:?} label at {child_left}, box step {step}"
                );
                found.push(top);
            }
        }
        assert!(found.len() >= 2, "only {found:?} found");
    }

    #[test]
    fn a_slider_with_room_keeps_its_label_on_one_line() {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut settings = Settings {
            current_tab: SettingsTab::Capture,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(217.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut lines = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            lines = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().starts_with("Screenshot") => {
                        Some(text.galley.rows.len())
                    }
                    _ => None,
                })
                .collect();
        }
        assert_eq!(lines, [1]);
    }

    #[test]
    fn a_checkbox_label_runs_the_row_in_the_dock_at_1280() {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        let mut settings = Settings {
            current_tab: SettingsTab::Screen,
            scaling: ScalingMode::Fit,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(DOCK_AT_1280, 4000.0),
            )),
            ..Default::default()
        };
        let wanted = label(RowId::SameSizePixels);
        let mut lines = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            lines = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.galley.rows.len())
                    }
                    _ => None,
                })
                .collect();
        }
        assert_eq!(lines, [1], "{wanted}");
    }

    fn screen_shapes_at(
        width: f32,
        theme: crate::app::ui_state::ui_theme::UiTheme,
    ) -> Vec<egui::Shape> {
        screen_clipped_at(width, theme)
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    fn screen_clipped_at(
        width: f32,
        theme: crate::app::ui_state::ui_theme::UiTheme,
    ) -> Vec<egui::epaint::ClippedShape> {
        page_clipped_at(SettingsTab::Screen, width, theme)
    }

    fn page_clipped_at(
        tab: SettingsTab,
        width: f32,
        theme: crate::app::ui_state::ui_theme::UiTheme,
    ) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        super::super::page::tests::quiet(&ctx);
        theme.apply(&ctx);
        let mut settings = Settings {
            current_tab: tab,
            scaling: ScalingMode::Fit,
            matrix: true,
            ghosting: true,
            ..Settings::default()
        };
        settings.hud.shown = true;
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..3 {
            shapes = ctx
                .run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        every_row(SettingsContent::new(
                            &mut settings,
                            &mut capture,
                            &mut editor,
                            None,
                            None,
                        ))
                        .ui(ui);
                    });
                })
                .shapes;
        }
        shapes
    }

    fn text_lefts(shapes: &[egui::Shape]) -> Vec<(String, f32)> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.pos.x + text.galley.rect.left(),
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn headings_top_labels_and_boxes_share_one_edge_and_children_start_at_the_label() {
        let shapes = screen_shapes_at(DOCK_AT_1280, crate::app::ui_state::ui_theme::UiTheme::Light);
        let lefts = text_lefts(&shapes);
        let left = |wanted: &str| {
            lefts
                .iter()
                .find(|(text, _)| text == wanted)
                .unwrap_or_else(|| panic!("no {wanted:?} in {lefts:?}"))
                .1
        };
        let step = egui::Style::default().spacing.icon_width + super::super::page::BOX_GAP;
        let edge = left(label(RowId::Scaling));
        let labels = edge + step;
        for (wanted, at) in [
            ("Size", edge),
            ("Screen effects", edge),
            ("HUDs", edge),
            (label(RowId::SameSizePixels), labels),
            (label(RowId::MatchRefresh), labels),
            (label(RowId::LcdGrid), labels),
            (label(RowId::Ghosting), labels),
            (label(RowId::ShowHuds), labels),
            (label(RowId::GridLines), labels),
            (label(RowId::GridCorners), labels),
            (label(RowId::GhostingStrength), labels),
            (label(RowId::HudOpacity), labels),
        ] {
            assert!(
                (left(wanted) - at).abs() < 0.5,
                "{wanted} starts at {}, not {at}; the edge is {edge}",
                left(wanted)
            );
        }
        let style = egui::Style::default();
        let boxes: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Rect(rect)
                    if (rect.rect.width() - style.spacing.icon_width).abs() < 0.5
                        && (rect.rect.height() - style.spacing.icon_width).abs() < 0.5 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(boxes.len(), 5, "{boxes:?}");
        for rect in boxes {
            assert!(
                (rect.left() - edge).abs() < 0.5,
                "box {rect:?} doesn't start at the edge {edge}"
            );
        }
    }

    #[test]
    fn a_box_shows_whole_and_a_heading_icon_keeps_four_points_to_the_pane() {
        use super::super::page::BOX_HANG;
        let theme = crate::app::ui_state::ui_theme::UiTheme::Light;
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let style = ctx.style();
        let size = style.spacing.icon_width;
        let boxes: Vec<_> = screen_clipped_at(DOCK_AT_1280, theme)
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Rect(rect)
                    if (rect.rect.width() - size).abs() < 0.5
                        && (rect.rect.height() - size).abs() < 0.5 =>
                {
                    Some((rect.rect, clipped.clip_rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(boxes.len(), 5, "{boxes:?}");
        for (rect, clip) in boxes {
            assert!(clip.contains_rect(rect), "box {rect:?} cut by {clip:?}");
        }
        let beside_list = 6.0 / 2.0 + style.spacing.item_spacing.x;
        let in_tab = crate::app::tabs::CONTENT_MARGIN.left as f32;
        for (pane, edge) in [("beside the page list", beside_list), ("in a tab", in_tab)] {
            assert!(
                edge - BOX_HANG >= 4.0,
                "{pane}: the heading icon is {} from the pane edge",
                edge - BOX_HANG
            );
        }
    }

    #[test]
    fn a_binding_name_starts_at_the_text_edge() {
        let theme = crate::app::ui_state::ui_theme::UiTheme::Light;
        let left = |wanted: &str| {
            page_clipped_at(SettingsTab::Controls, DOCK_AT_1280, theme)
                .into_iter()
                .find_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.pos.x + text.galley.rect.left())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {wanted} on Controls"))
        };
        let label = left("Stick dead zone");
        for name in ["Down", "Select"] {
            assert!(
                (left(name) - label).abs() < 0.5,
                "{name} at {}, labels at {label}",
                left(name)
            );
        }
    }

    #[test]
    fn an_empty_box_edge_reads_against_its_fill_and_the_panel() {
        use crate::app::ui_state::ui_theme::{UiTheme, box_edge, contrast};
        for theme in [UiTheme::Light, UiTheme::Dark] {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let visuals = ctx.style().visuals.clone();
            for fill in [
                visuals.widgets.inactive.bg_fill,
                visuals.widgets.hovered.bg_fill,
            ] {
                let edge = box_edge(&visuals, fill);
                assert!(contrast(edge, fill) >= 3.0, "{theme}: {edge:?} on {fill:?}");
                assert!(
                    contrast(edge, visuals.panel_fill) >= 3.0,
                    "{theme}: {edge:?} on the panel"
                );
            }
        }
    }

    #[test]
    fn a_really_narrow_dock_stacks_its_rows() {
        let width = super::super::page::STACK_BELOW - 10.0;
        let settings = Settings {
            current_tab: SettingsTab::Playing,
            ..Settings::default()
        };
        let laid = laid_out(settings, width);
        assert!(stacked_rows(&laid) > 3, "Playing at {width} pt");
    }

    #[test]
    fn scrolled_rows_never_show_above_the_page() {
        let ctx = egui::Context::default();
        let mut settings = Settings {
            current_tab: SettingsTab::App,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let mut top = f32::NAN;
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.separator();
                top = ui.cursor().top();
                SettingsContent::new(&mut settings, &mut capture, &mut editor, None, None).ui(ui);
            });
        });
        let clips = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Text(_)))
            .map(|clipped| clipped.clip_rect.top())
            .collect::<Vec<_>>();
        assert!(!clips.is_empty());
        for clip in clips {
            assert!(
                clip >= top - 0.01,
                "rows show from {clip}, the page starts at {top}"
            );
        }
    }

    #[test]
    fn each_console_page_lists_its_own_sound_channels_with_no_game() {
        let texts = |tab: SettingsTab| -> Vec<String> {
            render(Settings {
                current_tab: tab,
                ..Settings::default()
            })
            .into_iter()
            .map(|(_, text)| text)
            .collect()
        };
        let gb = texts(SettingsTab::GameBoy);
        for row in [
            RowId::GbPulse1,
            RowId::GbPulse2,
            RowId::GbWave,
            RowId::GbNoise,
        ] {
            assert!(gb.iter().any(|text| text == label(row)), "{row:?}: {gb:?}");
        }
        assert!(!gb.iter().any(|text| text == label(RowId::DirectSoundA)));
        if cfg!(feature = "gba") {
            let gba = texts(SettingsTab::Gba);
            for row in [RowId::GbaNoise, RowId::DirectSoundA, RowId::DirectSoundB] {
                assert!(
                    gba.iter().any(|text| text == label(row)),
                    "{row:?}: {gba:?}"
                );
            }
        }
        let sound = texts(SettingsTab::Sound);
        assert!(!sound.iter().any(|text| text == label(RowId::GbNoise)));
    }

    #[test]
    fn every_tree_row_shows_on_its_desktop_page() {
        let reveal = |tab: SettingsTab| {
            let mut settings = Settings {
                current_tab: tab,
                dev_mode: true,
                matrix: true,
                ghosting: true,
                scaling: ScalingMode::Fit,
                ..Settings::default()
            };
            settings.discord.enabled = true;
            settings.discord.screenshot = true;
            settings.ds_3d.resolution = crate::app::ui_state::ds_3d::Resolution::Double;
            settings.ds_screens.desktop.arrangement = Arrangement::BigAndSmall;
            settings.ds_online.service = crate::app::ui_state::ds_online::OnlineService::Kaeru;
            settings.microphone = crate::audio::mic::Microphone::Default;
            settings.push_to_talk = true;
            settings
        };
        let needs_a_device_or_file = [
            RowId::CameraSees,
            RowId::LightSensor,
            RowId::KeepFirmware,
            RowId::NetworkDevices,
        ];
        let whole_page_widgets = [SettingsTab::Online, SettingsTab::Storage];
        for tab in SettingsTab::iter_with_dev_mode(true) {
            if whole_page_widgets.contains(&tab) {
                continue;
            }
            let texts: Vec<String> = render(reveal(tab))
                .into_iter()
                .map(|(_, text)| text)
                .collect();
            for row in tab.rows() {
                if !drawn(row, true) || needs_a_device_or_file.contains(&row) {
                    continue;
                }
                let label = label(row);
                assert!(
                    texts
                        .iter()
                        .any(|text| text == label || text.ends_with(label)),
                    "{tab:?} misses {row:?} ({label:?}): {texts:?}"
                );
            }
        }
        let device = Device {
            light_sensor: true,
            live_camera: true,
            camera_error: None,
        };
        let gb: Vec<String> = render_with(
            Settings {
                current_tab: SettingsTab::GameBoy,
                ..Settings::default()
            },
            device,
        )
        .into_iter()
        .map(|(_, text)| text)
        .collect();
        assert!(
            gb.iter().any(|text| text == label(RowId::CameraSees)),
            "{gb:?}"
        );
        assert!(
            !gb.iter().any(|text| text == label(RowId::ChoosePicture)),
            "{gb:?}"
        );
        if cfg!(feature = "gba") {
            let gba: Vec<String> = render_with(
                Settings {
                    current_tab: SettingsTab::Gba,
                    ..Settings::default()
                },
                device,
            )
            .into_iter()
            .map(|(_, text)| text)
            .collect();
            assert!(
                gba.iter().any(|text| text == label(RowId::LightSensor)),
                "{gba:?}"
            );
        }
    }

    fn reset_column() -> f32 {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                width = crate::app::widgets::reset_slider::reset_width(ui);
            });
        });
        width
    }

    const DOCK_AT_1280: f32 = 324.0;

    fn pages_with_ds_screens() -> impl Iterator<Item = (String, Settings)> {
        let ds = cfg!(feature = "nds").then(|| {
            let mut mic = ds_style(false);
            mic.microphone = crate::audio::mic::Microphone::Default;
            [
                ("DS screens".to_owned(), ds_style(false)),
                ("DS with a microphone".to_owned(), mic),
            ]
        });
        desktop_pages().chain(ds.into_iter().flatten())
    }

    fn painted_rects(
        mut settings: Settings,
        width: f32,
    ) -> (
        super::super::page::tests::Laid,
        Vec<egui::Rect>,
        Vec<egui::Rect>,
    ) {
        use super::super::page::tests::{after_pass, quiet};
        let ctx = egui::Context::default();
        quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut laid = Default::default();
        let mut rects = Vec::new();
        let mut texts = Vec::new();
        for _ in 0..SETTLE_RUNS {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            laid = after_pass(&ctx);
            rects.clear();
            texts.clear();
            for clipped in output.shapes {
                match clipped.shape {
                    egui::Shape::Rect(rect) if rect.rect.is_positive() => rects.push(rect.rect),
                    egui::Shape::Text(text) => texts.push(text.visual_bounding_rect()),
                    _ => {}
                }
            }
        }
        (laid, rects, texts)
    }

    #[test]
    fn each_page_puts_its_controls_at_one_x_in_the_default_dock() {
        let gap = egui::Style::default().spacing.item_spacing.x;
        let mut faults = Vec::new();
        for (name, settings) in pages_with_ds_screens() {
            let _ = super::super::form::take_dock_rows();
            let _ = laid_out(settings, 217.0);
            let mut last = std::collections::BTreeMap::new();
            for row in super::super::form::take_dock_rows() {
                last.insert(row.label.clone(), row);
            }
            for row in last.values() {
                let column = row.page;
                let section = last
                    .values()
                    .filter(|other| other.section == row.section && row.shared);
                if ["Nickname", "Birthday"].contains(&row.label.as_str()) && !row.shared {
                    faults.push(format!("{name}: {} has no section column", row.label));
                }
                if let Some(other) = section
                    .clone()
                    .find(|other| (other.value_x - row.value_x).abs() >= 0.5)
                {
                    faults.push(format!(
                        "{name}: {} puts its value at {}, {} in its section at {}",
                        row.label, row.value_x, other.label, other.value_x
                    ));
                }
                let at_column = (row.value_x - column).abs() < 0.5;
                let for_a_word = section.clone().chain([row]).any(|other| {
                    row.value_x > other.floor - gap / 2.0 - 0.5 && row.value_x < other.floor + 0.5
                });
                let on_one_line = section.clone().chain([row]).any(|other| {
                    other.line > column + 0.5
                        && row.value_x > other.line - gap / 2.0 - 0.5
                        && row.value_x < other.line + 0.5
                });
                if !at_column && !for_a_word && !on_one_line {
                    faults.push(format!(
                        "{name}: {} puts its value at {}, the column is {column} pt",
                        row.label, row.value_x
                    ));
                }
            }
        }
        assert!(faults.is_empty(), "{faults:#?}");
    }

    #[test]
    fn the_3d_resolution_label_takes_one_line_in_the_default_dock() {
        let mut seen = false;
        for ((name, settings), (_, again)) in pages_with_ds_screens().zip(pages_with_ds_screens()) {
            let _ = super::super::form::take_dock_rows();
            let _ = laid_out(settings, 217.0);
            if !super::super::form::take_dock_rows()
                .iter()
                .any(|row| row.label == "3D resolution")
            {
                continue;
            }
            seen = true;
            let wrapped = wrapped_labels(again, 217.0);
            assert!(
                !wrapped.iter().any(|text| text.starts_with("3D resolution")),
                "{name}: {wrapped:?}"
            );
        }
        assert_eq!(seen, cfg!(feature = "nds"));
    }

    #[test]
    fn every_desktop_page_puts_its_controls_at_one_x() {
        use super::super::page::Mark;
        use crate::app::widgets::reset_slider::RESET_GAP;
        let reset = reset_column();
        let mut faults = Vec::new();
        for width in [DOCK_AT_1280, 800.0] {
            let mut shared: Option<(String, f32)> = None;
            for (name, settings) in pages_with_ds_screens() {
                let ((marks, controls), painted, _) = painted_rects(settings, width);
                let columns = marks
                    .iter()
                    .filter(|(kind, _)| *kind == Mark::Control)
                    .map(|(_, rect)| *rect)
                    .collect::<Vec<_>>();
                let Some(x) = columns.first().map(|column| column.left()) else {
                    continue;
                };
                match &shared {
                    Some((first, at)) if (x - at).abs() >= 0.5 => faults.push(format!(
                        "{name} at {width}: controls start at {x}, on {first} at {at}"
                    )),
                    Some(_) => {}
                    None => shared = Some((name.clone(), x)),
                }
                for column in &columns {
                    if (column.left() - x).abs() >= 0.5 {
                        faults.push(format!(
                            "{name} at {width}: a control column starts at {}, the first at {x}",
                            column.left()
                        ));
                    }
                    let inside = controls
                        .iter()
                        .filter(|control| column.contains(control.center()))
                        .collect::<Vec<_>>();
                    let edge = column.right() - reset;
                    if let Some(first) = inside
                        .iter()
                        .map(|control| control.left())
                        .filter(|left| *left < edge - 0.5)
                        .reduce(f32::min)
                        && (first - x).abs() >= 0.5
                    {
                        faults.push(format!(
                            "{name} at {width}: a control starts at {first}, the column at {x}"
                        ));
                    }
                    let drawn = painted
                        .iter()
                        .filter(|rect| rect.left() >= x - 0.5 && column.contains(rect.center()));
                    for rect in inside.into_iter().chain(drawn) {
                        if rect.left() < edge - 0.5 && rect.right() > edge - RESET_GAP + 0.5 {
                            faults.push(format!(
                                "{name} at {width}: {rect:?} runs into the reset column at {edge}"
                            ));
                        }
                    }
                }
            }
        }
        faults.dedup();
        assert!(faults.is_empty(), "{faults:#?}");
    }

    #[test]
    fn every_reset_arrow_sits_centred_on_its_control() {
        use super::super::page::Mark;
        use crate::app::widgets::reset_slider::RESET_GAP;
        let reset = reset_column();
        let mut faults = Vec::new();
        for width in [DOCK_AT_1280, 800.0] {
            for (name, settings) in pages_with_ds_screens() {
                let ((marks, _), painted, texts) = painted_rects(settings, width);
                for column in marks
                    .iter()
                    .filter(|(kind, _)| *kind == Mark::Control)
                    .map(|(_, rect)| *rect)
                {
                    let edge = column.right() - reset;
                    let resets = painted.iter().filter(|rect| {
                        column.contains(rect.center())
                            && rect.left() >= edge - 0.5
                            && (rect.width() - reset).abs() < 0.5
                    });
                    for arrow in resets {
                        let y = arrow.center().y;
                        let off = painted
                            .iter()
                            .filter(|rect| {
                                rect.left() >= column.left() - 0.5
                                    && rect.right() <= edge - RESET_GAP + 0.5
                                    && rect.y_range().contains(y)
                            })
                            .map(|rect| (rect.center().y - y).abs())
                            .reduce(f32::min);
                        let text_beside = texts.iter().any(|text| {
                            text.left() >= column.left() - 0.5
                                && text.right() <= edge - RESET_GAP + 0.5
                                && text.y_range().contains(y)
                        });
                        match off {
                            Some(off) if off < 0.75 => {}
                            None if text_beside => {}
                            Some(off) => faults.push(format!(
                                "{name} at {width}: the reset at {arrow:?} sits {off} off its control"
                            )),
                            None => faults.push(format!(
                                "{name} at {width}: the reset at {arrow:?} has no control beside it"
                            )),
                        }
                    }
                }
            }
        }
        faults.dedup();
        assert!(faults.is_empty(), "{faults:#?}");
    }

    #[test]
    fn the_shared_label_column_holds_every_desktop_label() {
        use super::super::form::{DESKTOP_LABELS, take_widest_label};
        let mut widest = 0.0_f32;
        let mut by_page = Vec::new();
        for (name, settings) in pages_with_ds_screens() {
            let _ = take_widest_label();
            let _ = laid_out(settings, 800.0);
            let page = take_widest_label();
            by_page.push(format!("{name}: {page}"));
            widest = widest.max(page);
        }
        assert!(
            widest > 0.0 && widest <= DESKTOP_LABELS + 0.5,
            "DESKTOP_LABELS is {DESKTOP_LABELS}, the widest label {widest}: {by_page:#?}"
        );
    }

    #[test]
    fn the_grid_shows_no_scaling_but_the_one_chosen() {
        use strum::IntoEnumIterator;
        for scaling in [ScalingMode::Fit, ScalingMode::Stretch] {
            let texts = render(Settings {
                current_tab: SettingsTab::Screen,
                matrix: true,
                scaling,
                same_size_pixels: false,
                ..Settings::default()
            });
            let other = texts
                .iter()
                .filter(|(_, text)| {
                    ScalingMode::iter()
                        .filter(|mode| *mode != scaling)
                        .any(|mode| text.contains(&mode.to_string()))
                })
                .collect::<Vec<_>>();
            assert!(other.is_empty(), "at {scaling} the page shows {other:?}");
        }
    }

    fn wrapped_labels(settings: Settings, width: f32) -> Vec<String> {
        wrapped_label_rows(settings, width)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    fn wrapped_label_rows(settings: Settings, width: f32) -> Vec<(String, Vec<String>)> {
        wrapped_label_galleys(settings, width)
            .into_iter()
            .map(|galley| {
                let rows = galley
                    .rows
                    .iter()
                    .map(|row| row.glyphs.iter().map(|glyph| glyph.chr).collect())
                    .collect();
                (galley.text().trim().to_owned(), rows)
            })
            .collect()
    }

    fn wrapped_label_galleys(
        mut settings: Settings,
        width: f32,
    ) -> Vec<std::sync::Arc<egui::Galley>> {
        use super::super::page::Mark;
        use super::super::page::tests::{after_pass, quiet};
        let ctx = egui::Context::default();
        quiet(&ctx);
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut wrapped = Vec::new();
        for _ in 0..4 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    every_row(SettingsContent::new(
                        &mut settings,
                        &mut capture,
                        &mut editor,
                        None,
                        None,
                    ))
                    .ui(ui);
                });
            });
            let (marks, _) = after_pass(&ctx);
            let Some(x) = marks
                .iter()
                .find(|(kind, _)| *kind == Mark::Control)
                .map(|(_, rect)| rect.left())
            else {
                return Vec::new();
            };
            wrapped = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text)
                        if text.galley.rows.len() > 1
                            && text.pos.x + text.galley.rect.right() <= x =>
                    {
                        Some(text.galley)
                    }
                    _ => None,
                })
                .collect();
        }
        wrapped
    }

    #[test]
    fn a_label_in_the_default_dock_wraps_only_between_words() {
        let split = pages_with_ds_screens()
            .flat_map(|(name, settings)| {
                wrapped_label_rows(settings, 217.0)
                    .into_iter()
                    .filter(|(text, rows)| {
                        let words = rows
                            .iter()
                            .flat_map(|row| row.split_whitespace().map(str::to_owned))
                            .collect::<Vec<String>>();
                        words != text.split_whitespace().collect::<Vec<_>>()
                    })
                    .map(move |(text, rows)| format!("{name}: {text} as {rows:?}"))
            })
            .collect::<Vec<_>>();
        assert!(split.is_empty(), "labels split inside a word: {split:#?}");
    }

    #[test]
    fn a_wrapped_label_starts_each_line_at_one_edge_in_the_default_dock() {
        let ragged = pages_with_ds_screens()
            .flat_map(|(name, settings)| {
                wrapped_label_galleys(settings, 217.0)
                    .into_iter()
                    .filter_map(move |galley| {
                        let starts: Vec<f32> = galley
                            .rows
                            .iter()
                            .filter_map(|row| {
                                row.glyphs
                                    .iter()
                                    .find(|glyph| !glyph.chr.is_whitespace())
                                    .map(|glyph| row.pos.x + glyph.pos.x)
                            })
                            .collect();
                        let first = starts.first().copied()?;
                        starts
                            .iter()
                            .any(|x| (x - first).abs() > 0.5)
                            .then(|| format!("{name}: {} at {starts:?}", galley.text()))
                    })
            })
            .collect::<Vec<_>>();
        assert!(ragged.is_empty(), "wrapped lines start apart: {ragged:#?}");
    }

    #[test]
    fn desktop_labels_take_one_line_in_the_dock_at_1280() {
        let wrapped = pages_with_ds_screens()
            .flat_map(|(name, settings)| {
                wrapped_labels(settings, DOCK_AT_1280)
                    .into_iter()
                    .map(move |label| format!("{name}: {label}"))
            })
            .collect::<Vec<_>>();
        assert!(wrapped.is_empty(), "labels on two lines: {wrapped:#?}");
    }

    #[test]
    fn app_and_ds_pages_settle_without_discards_in_the_desktop_dock() {
        for tab in [SettingsTab::App, SettingsTab::Ds] {
            settles_without_discards(tab);
        }
    }

    fn settles_without_discards(tab: SettingsTab) {
        let ctx = egui::Context::default();
        let mut settings = Settings {
            current_tab: tab,
            ..Settings::default()
        };
        let mut capture = BindingCapture::default();
        let mut editor = PaletteEditorState::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut reasons = Vec::new();
        for _ in 0..SETTLE_RUNS {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        let content = SettingsContent::new(
                            &mut settings,
                            &mut capture,
                            &mut editor,
                            None,
                            None,
                        );
                        content.ui(ui);
                    });
                });
            });
            reasons = output.platform_output.request_discard_reasons;
        }
        assert!(reasons.is_empty(), "{reasons:?}");
    }
}
