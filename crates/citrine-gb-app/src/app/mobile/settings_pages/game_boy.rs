use super::{Sheet, label, opens, pick, pick_from};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet;
use crate::app::system_files::SystemFile;
use crate::app::ui_state::palette::{CustomPalette, PaletteChoice};
use crate::app::ui_state::settings::{ConsoleChoice, GBC_SCREEN_NOTE, Settings};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::palette_strip::paint_strip;
use crate::app::widgets::settings::palette::{
    Action, PaletteEditorState, SHADE_NAMES, all_choices, apply, choice_colors, choice_label,
};
use crate::emulator::gimmicks::CameraSource;
use crate::icons;
use citrine_gb::gb::GbModel;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Context, Rect, RichText, Ui, vec2};

const STRIP: f32 = 22.0;
const CHANNELS_NOTE: &str = "The Game Boy's four sound channels. Pulse 1 and 2 usually carry the tune, Wave the bass and Noise the drums.";

struct GbGame {
    model: GbModel,
}

pub(in crate::app::mobile) fn strip_row(
    ui: &mut Ui,
    settings: &Settings,
    choice: &PaletteChoice,
    header: &[u8],
) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), STRIP + 14.0),
        egui::Sense::hover(),
    );
    let strip = Rect::from_min_size(rect.min + vec2(12.0, 0.0), vec2(rect.width() - 24.0, STRIP));
    paint_strip(ui, strip, &choice_colors(settings, choice, header));
}

fn palette_sheet(
    ctx: &Context,
    title: &str,
    settings: &Settings,
    current: &PaletteChoice,
    header: &[u8],
) -> (Option<PaletteChoice>, bool) {
    let choices = all_choices(settings);
    let shown = sheet::show(ctx, super::sheet_id(), title, |ui| {
        let mut picked = None;
        kit::list(ui, |list| {
            for choice in &choices {
                let label = choice_label(choice);
                let response = list.row(Row::new(&label).checked(choice == current));
                let swatch = Rect::from_min_size(
                    egui::pos2(
                        response.rect.right() - 40.0 - 88.0,
                        response.rect.center().y - 9.0,
                    ),
                    vec2(88.0, 18.0),
                );
                paint_strip(list.ui(), swatch, &choice_colors(settings, choice, header));
                if response.clicked() {
                    picked = Some(choice.clone());
                }
            }
        });
        picked
    });
    let done = shown.inner.is_some() || shown.closed;
    (shown.inner, done)
}

fn channel(row: RowId) -> Option<usize> {
    match row {
        RowId::GbPulse1 => Some(0),
        RowId::GbPulse2 => Some(1),
        RowId::GbWave => Some(2),
        RowId::GbNoise => Some(3),
        _ => None,
    }
}

fn own_palettes_value(settings: &Settings) -> String {
    match settings.palettes.entries().len() {
        0 => "None".to_owned(),
        1 => "1 palette".to_owned(),
        n => format!("{n} palettes"),
    }
}

impl Citrine {
    fn gb_game(&self) -> Option<GbGame> {
        self.emulator.rom_key()?;
        self.emulator.as_gb()?;
        Some(GbGame {
            model: self.emulator.console(),
        })
    }

    pub(super) fn game_boy_page(&mut self, ui: &mut Ui) {
        self.tree_page(
            ui,
            SettingsTab::GameBoy,
            |group| (group == GroupId::GbChannels).then_some(CHANNELS_NOTE),
            Self::game_boy_row,
            |_| {},
        );
    }

    fn game_boy_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let live_camera = self.senses.has_live_camera();
        let camera_error = self.senses.camera_error().map(str::to_owned);
        let picture_set = self.emulator.gimmicks.camera_frame().is_some();
        let s = &mut self.ui.settings;
        match row {
            RowId::RunGamesOn => opens(
                list,
                Row::new(label(row)).value(s.console.to_string()),
                Sheet::Console,
            ),
            RowId::GbBootRom | RowId::GbcBootRom => {
                SystemFile::of_row(row).and_then(|file| self.file_row(list, file))
            }
            RowId::CameraSees if live_camera => {
                let detail = if s.gimmicks.camera == CameraSource::Camera {
                    camera_error.unwrap_or_default()
                } else {
                    String::new()
                };
                opens(
                    list,
                    Row::new(label(row))
                        .value(s.gimmicks.camera.to_string())
                        .detail(&detail),
                    Sheet::Camera,
                )
            }
            RowId::ChoosePicture if !live_camera || s.gimmicks.camera == CameraSource::Picture => {
                let value = if picture_set {
                    "Picture chosen"
                } else {
                    "No picture"
                };
                if list.row(Row::new(label(row)).value(value)).clicked() {
                    self.pick_camera_picture();
                }
                None
            }
            RowId::Palette => {
                let global = s.global_choice();
                let open = opens(
                    list,
                    Row::new(label(row)).value(choice_label(&global)),
                    Sheet::Palette,
                );
                list.custom(|ui| strip_row(ui, s, &global, &[]));
                open
            }
            RowId::OwnPalettes => opens(
                list,
                Row::new(label(row)).value(own_palettes_value(s)),
                Sheet::OwnPalettes,
            ),
            RowId::GbcScreen => opens(
                list,
                Row::new(label(row)).value(s.color_correction.to_string()),
                Sheet::GbcScreen,
            ),
            _ => {
                if let Some(index) = channel(row) {
                    s.dirty |= list.switch(SwitchRow::new(label(row), &mut s.channels[index]));
                }
                None
            }
        }
    }

    pub(super) fn game_boy_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let game = self.gb_game();
        let s = &mut self.ui.settings;
        let state = &mut self.ui.palette_editor;
        match open {
            Sheet::Console => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::RunGamesOn),
                    Some("Automatic picks Game Boy Color for games made for it."),
                    &mut s.console,
                );
                s.dirty |= changed;
                done
            }
            Sheet::GameConsole => {
                let Some(game) = game else {
                    return true;
                };
                let mut choice = ConsoleChoice::of(game.model);
                let options = [ConsoleChoice::GameBoy, ConsoleChoice::GameBoyColor];
                let (changed, done) = pick_from(
                    ctx,
                    crate::app::widgets::settings::this_game::PLAY_ON,
                    Some("Used for this game instead of the console picked on the Game Boy page."),
                    options,
                    &mut choice,
                    |c| c.to_string(),
                );
                if changed {
                    self.events.play_on(choice.resolve(GbModel::Cgb));
                }
                done
            }
            Sheet::GbcScreen => {
                let (changed, done) = pick(
                    ctx,
                    label(RowId::GbcScreen),
                    Some(GBC_SCREEN_NOTE),
                    &mut s.color_correction,
                );
                s.dirty |= changed;
                done
            }
            Sheet::Palette => {
                let (picked, done) =
                    palette_sheet(ctx, label(RowId::Palette), s, &s.global_choice(), &[]);
                if let Some(choice) = picked {
                    apply(Action::SetGlobal(choice), s, state);
                }
                done
            }
            Sheet::OwnPalettes => {
                let mut action = None;
                let mut next = None;
                let shown = sheet::show(ctx, super::sheet_id(), label(RowId::OwnPalettes), |ui| {
                    own_palettes(ui, s, state, &mut action, &mut next);
                });
                if let Some(action) = action {
                    apply(action, s, state);
                }
                if let Some(next) = next {
                    self.mobile.sheet = Some(next);
                    return false;
                }
                shown.closed
            }
            Sheet::PasteCode => paste_code(ctx, s, state),
            _ => {
                let Some(name) = s.custom_palette.clone() else {
                    return true;
                };
                let question = format!("Delete the palette {name}?");
                match crate::app::widgets::confirm::confirm(
                    ctx,
                    "delete_palette",
                    &question,
                    "Delete",
                ) {
                    Some(true) => {
                        apply(Action::Delete(name), s, state);
                        true
                    }
                    Some(false) => true,
                    None => false,
                }
            }
        }
    }
}

fn own_palettes(
    ui: &mut Ui,
    s: &Settings,
    state: &mut PaletteEditorState,
    action: &mut Option<Action>,
    next: &mut Option<Sheet>,
) {
    let global = s.global_choice();
    kit::list(ui, |list| {
        if list
            .row(Row::new("New palette").icon(icons::PLUS).no_chevron())
            .clicked()
        {
            let name = s.palettes.unique_name("My palette");
            *action = Some(Action::Create(CustomPalette::from_theme(
                name,
                &DmgTheme::GreyScale,
            )));
        }
        if list
            .row(Row::new("Duplicate").icon(icons::COPY).no_chevron())
            .clicked()
        {
            let shown = DmgTheme::Custom(choice_colors(s, &global, &[])[0]);
            let name = s.palettes.unique_name(&choice_label(&global));
            *action = Some(Action::Create(CustomPalette::from_theme(name, &shown)));
        }
    });
    if let Some(selected) = s.custom_palette.clone()
        && let Some(palette) = s.palettes.get(&selected).cloned()
    {
        state.focus(&selected, &palette);
        ui.add_space(16.0);
        sheet::label(ui, &palette.name);
        editor(ui, &palette, state, action, next);
    } else {
        state.forget();
    }
}

fn paste_code(ctx: &Context, s: &mut Settings, state: &mut PaletteEditorState) -> bool {
    let Some(name) = s.custom_palette.clone() else {
        return true;
    };
    let mut apply_code = false;
    let shown = sheet::show(ctx, super::sheet_id(), "Paste a share code", |ui| {
        sheet::note(
            ui,
            "Four colours, lightest first, like F2CE44-B89D32-72611C-292406.",
        );
        kit::list(ui, |list| {
            list.custom(|ui| {
                kit::padded(ui, |ui| {
                    let field = egui::TextEdit::singleline(&mut state.code_buffer)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width());
                    if crate::app::widgets::field::framed(ui, field)
                        .response
                        .changed()
                    {
                        state.code_error = false;
                    }
                });
            });
        });
        if state.code_error {
            ui.add_space(6.0);
            ui.label(
                RichText::new("That code doesn't hold four colours.")
                    .color(ui.visuals().error_fg_color),
            );
        }
        ui.add_space(10.0);
        apply_code = kit::primary_button(ui, "Use these colours");
    });
    if apply_code {
        match CustomPalette::parse_code(&name, &state.code_buffer) {
            Some(parsed) => {
                state.code_error = false;
                apply(
                    Action::Recolor {
                        name,
                        colors: parsed.colors,
                    },
                    s,
                    state,
                );
                return true;
            }
            None => state.code_error = true,
        }
    }
    shown.closed
}

fn editor(
    ui: &mut Ui,
    palette: &CustomPalette,
    state: &mut PaletteEditorState,
    action: &mut Option<Action>,
    next: &mut Option<Sheet>,
) {
    kit::list(ui, |list| {
        list.custom(|ui| {
            kit::padded(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Name").size(16.0));
                    let response =
                        kit::field(ui, egui::TextEdit::singleline(&mut state.name_buffer));
                    if response.lost_focus() {
                        action.get_or_insert(Action::Rename {
                            from: palette.name.clone(),
                            to: state.name_buffer.clone(),
                        });
                    }
                });
            })
        });
        list.custom(|ui| {
            kit::padded(ui, |ui| {
                ui.horizontal(|ui| {
                    let side = ui.spacing().interact_size.y;
                    let mut colors = palette.colors;
                    let mut changed = false;
                    let gap = ui.spacing().item_spacing.x;
                    let width = ((ui.available_width() - 3.0 * gap) / 4.0).max(side);
                    ui.spacing_mut().interact_size = vec2(width, side);
                    for (index, color) in colors.iter_mut().enumerate() {
                        changed |= ui
                            .color_edit_button_srgb(color)
                            .on_hover_text(SHADE_NAMES[index])
                            .changed();
                    }
                    if changed {
                        action.get_or_insert(Action::Recolor {
                            name: palette.name.clone(),
                            colors,
                        });
                    }
                });
            })
        });
        let code = palette.to_code();
        if list
            .row(
                Row::new("Copy share code")
                    .icon(icons::COPY)
                    .detail(&code)
                    .no_chevron(),
            )
            .clicked()
        {
            list.ui().ctx().copy_text(code.clone());
        }
        if list
            .row(Row::new("Paste a share code").icon(icons::CLIPBOARD_TEXT))
            .clicked()
        {
            state.code_buffer.clear();
            state.code_error = false;
            *next = Some(Sheet::PasteCode);
        }
        if list
            .row(
                Row::new("Delete palette")
                    .icon(icons::TRASH)
                    .danger()
                    .no_chevron(),
            )
            .clicked()
        {
            *next = Some(Sheet::DeletePalette);
        }
    });
}

#[cfg(all(test, feature = "nds"))]
mod tests {
    use crate::app::Citrine;
    use crate::app::files::nds_tests::Opened;
    use crate::app::tests::{info_frame_out, text_rect};
    use crate::app::ui_state::settings_tree::SettingsTab;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use egui::Context;

    fn shows(app: &mut Citrine, ctx: &Context, text: &str) -> bool {
        info_frame_out(app, ctx, 412.0, 4000.0);
        text_rect(&info_frame_out(app, ctx, 412.0, 4000.0), text).is_some()
    }

    #[test]
    fn the_game_boy_page_keeps_its_rows_with_a_ds_game() {
        let mut opened = Opened::new("gb-page-ds-game");
        let picked = opened.picked();
        let app = &mut opened.app;
        let ctx = Context::default();
        let gbc = super::label(RowId::GbcScreen);
        app.mobile.nav.open_settings(SettingsTab::GameBoy);
        assert!(shows(app, &ctx, gbc));
        app.handle_load_rom(picked);
        app.mobile.nav.open_settings(SettingsTab::GameBoy);
        assert!(shows(app, &ctx, gbc));
        assert!(shows(app, &ctx, super::label(RowId::GbNoise)));
    }
}
