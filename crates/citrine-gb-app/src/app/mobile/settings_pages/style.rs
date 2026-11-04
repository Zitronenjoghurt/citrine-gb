use super::{Sheet, pick, pick_from, slide};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::palette::{CustomPalette, PaletteChoice};
use crate::app::ui_state::settings::{
    GBA_SCREEN_NOTE, GBC_SCREEN_NOTE, GbaScreen, ScalingMode, Settings, rom_header,
};
use crate::app::widgets::palette_strip::paint_strip;
use crate::app::widgets::settings::palette::{
    Action, PaletteEditorState, SHADE_NAMES, all_choices, apply, choice_colors, choice_label,
};
use crate::icons;
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Context, Rect, RichText, Ui, vec2};

const MATRIX_NOTE: &str = "Draws the grid between pixels.";
const GHOSTING_NOTE: &str = "Smears frames together, the way the original LCD did.";
const STRIP: f32 = 22.0;

pub struct StyleGame {
    rom_key: String,
    title: String,
    header: Vec<u8>,
    own_colors: bool,
}

fn strip_row(ui: &mut Ui, settings: &Settings, choice: &PaletteChoice, header: &[u8]) {
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

impl Citrine {
    fn style_game(&self) -> Option<StyleGame> {
        let rom_key = self.emulator.rom_key()?.to_owned();
        let gb = self.emulator.as_gb()?;
        Some(StyleGame {
            rom_key,
            title: self.emulator.game_name().to_owned(),
            header: rom_header(gb),
            own_colors: gb.ppu.cgb_mode(),
        })
    }

    pub(super) fn style_page(&mut self, ui: &mut Ui) {
        let game = self.style_game();
        let palettes = self.emulator.as_gb().is_some();
        let gba = self.emulator.kind() == ConsoleKind::Gba;
        let header = game.as_ref().map(|g| g.header.clone()).unwrap_or_default();
        let s = &mut self.ui.settings;
        let state = &mut self.ui.palette_editor;
        let mut open = None;
        let mut action = None;
        ui.add_space(8.0);
        if palettes {
            let global = s.global_choice();
            kit::list(ui, |list| {
                if list
                    .row(Row::new("Palette").value(choice_label(&global)))
                    .clicked()
                {
                    open = Some(Sheet::Palette);
                }
                list.custom(|ui| strip_row(ui, s, &global, &header));
                if list
                    .row(Row::new("New palette").icon(icons::PLUS).no_chevron())
                    .clicked()
                {
                    let name = s.palettes.unique_name("My palette");
                    action = Some(Action::Create(CustomPalette::from_theme(
                        name,
                        &DmgTheme::GreyScale,
                    )));
                }
                if list
                    .row(Row::new("Duplicate").icon(icons::COPY).no_chevron())
                    .clicked()
                {
                    let shown = DmgTheme::Custom(choice_colors(s, &global, &header)[0]);
                    let name = s.palettes.unique_name(&choice_label(&global));
                    action = Some(Action::Create(CustomPalette::from_theme(name, &shown)));
                }
            });
            if let Some(selected) = s.custom_palette.clone()
                && let Some(palette) = s.palettes.get(&selected).cloned()
            {
                state.focus(&selected, &palette);
                kit::section_label(ui, &palette.name);
                editor(ui, &palette, state, &mut action, &mut open);
            } else {
                state.forget();
            }
            if let Some(game) = &game {
                let title = if game.title.is_empty() {
                    "Running now"
                } else {
                    &game.title
                };
                kit::section_label(ui, "This game");
                kit::list(ui, |list| {
                    if game.own_colors {
                        list.row(
                            Row::new(title)
                                .detail("Uses its own colours, not a palette")
                                .no_chevron(),
                        );
                        return;
                    }
                    let own = s.game_palette(Some(&game.rom_key)).cloned();
                    let mut keep = own.is_some();
                    if list.switch(SwitchRow::new("Own palette", &mut keep).detail(Some(title))) {
                        action = Some(Action::SetGame {
                            rom_key: game.rom_key.clone(),
                            choice: keep.then(|| global.clone()),
                        });
                    }
                    if let Some(own) = own {
                        if list
                            .row(Row::new("Palette").value(choice_label(&own)))
                            .clicked()
                        {
                            open = Some(Sheet::GamePalette);
                        }
                        list.custom(|ui| strip_row(ui, s, &own, &game.header));
                    }
                });
            }
        }
        kit::section_label(ui, "Screen");
        let mut dirty = false;
        kit::list(ui, |list| {
            if list
                .row(Row::new("Scaling").value(s.touch.scaling.to_string()))
                .clicked()
            {
                open = Some(Sheet::TouchScaling);
            }
            let detail = (s.matrix && s.touch.scaling != ScalingMode::Integer)
                .then_some("Scales by whole numbers while on");
            dirty |= list.switch(
                SwitchRow::new("Matrix", &mut s.matrix)
                    .detail(detail)
                    .info(MATRIX_NOTE),
            );
            if s.matrix {
                let rows = [
                    (Sheet::EdgeDarkness, "Edge darkness", s.matrix_edge_darkness),
                    (
                        Sheet::CornerDarkness,
                        "Corner darkness",
                        s.matrix_corner_darkness,
                    ),
                ];
                for (sheet, label, value) in rows {
                    if list
                        .row(Row::new(label).value(sheet::percent(value.into())))
                        .clicked()
                    {
                        open = Some(sheet);
                    }
                }
            }
            dirty |= list.switch(SwitchRow::new("Ghosting", &mut s.ghosting).info(GHOSTING_NOTE));
            if s.ghosting
                && list
                    .row(Row::new("Strength").value(sheet::percent(s.ghosting_strength.into())))
                    .clicked()
            {
                open = Some(Sheet::GhostStrength);
            }
            let (screen, value) = if gba {
                ("GBA screen", GbaScreen(s.color_correction).to_string())
            } else {
                ("GBC screen", s.color_correction.to_string())
            };
            if list.row(Row::new(screen).value(value)).clicked() {
                open = Some(Sheet::GbcScreen);
            }
        });
        s.dirty |= dirty;
        if let Some(action) = action {
            apply(action, s, state);
        }
        self.open_sheet(open);
    }

    pub(super) fn style_sheet(&mut self, ctx: &Context, open: Sheet) -> bool {
        let game = self.style_game();
        let gba = self.emulator.kind() == ConsoleKind::Gba;
        let header = game.as_ref().map(|g| g.header.clone()).unwrap_or_default();
        let s = &mut self.ui.settings;
        let state = &mut self.ui.palette_editor;
        let darkness = |value: &mut f32, default: f32, title: &str| {
            slide(
                ctx,
                title,
                None,
                Amount {
                    value,
                    range: 0.0..=1.0,
                    step: 0.01,
                    default,
                    text: sheet::percent,
                },
            )
        };
        match open {
            Sheet::Palette => {
                let (picked, done) = palette_sheet(ctx, "Palette", s, &s.global_choice(), &header);
                if let Some(choice) = picked {
                    apply(Action::SetGlobal(choice), s, state);
                }
                done
            }
            Sheet::GamePalette => {
                let Some(game) = game else {
                    return true;
                };
                let current = s
                    .game_palette(Some(&game.rom_key))
                    .cloned()
                    .unwrap_or_else(|| s.global_choice());
                let (picked, done) = palette_sheet(ctx, &game.title, s, &current, &header);
                if let Some(choice) = picked {
                    apply(
                        Action::SetGame {
                            rom_key: game.rom_key,
                            choice: Some(choice),
                        },
                        s,
                        state,
                    );
                }
                done
            }
            Sheet::GbcScreen => {
                let (changed, done) = if gba {
                    pick_from(
                        ctx,
                        "GBA screen",
                        Some(GBA_SCREEN_NOTE),
                        GbaScreen::ALL.map(|screen| screen.0),
                        &mut s.color_correction,
                        |value| GbaScreen(value).to_string(),
                    )
                } else {
                    pick(
                        ctx,
                        "GBC screen",
                        Some(GBC_SCREEN_NOTE),
                        &mut s.color_correction,
                    )
                };
                s.dirty |= changed;
                done
            }
            Sheet::EdgeDarkness => {
                let shown = darkness(
                    &mut s.matrix_edge_darkness,
                    Settings::DEFAULT_MATRIX_EDGE_DARKNESS,
                    "Edge darkness",
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::CornerDarkness => {
                let shown = darkness(
                    &mut s.matrix_corner_darkness,
                    Settings::DEFAULT_MATRIX_CORNER_DARKNESS,
                    "Corner darkness",
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::GhostStrength => {
                let shown = darkness(
                    &mut s.ghosting_strength,
                    Settings::DEFAULT_GHOSTING_STRENGTH,
                    "Ghosting strength",
                );
                s.dirty |= shown.inner.changed();
                shown.closed
            }
            Sheet::PasteCode => {
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
                                if ui.add(field).changed() {
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

fn editor(
    ui: &mut Ui,
    palette: &CustomPalette,
    state: &mut PaletteEditorState,
    action: &mut Option<Action>,
    open: &mut Option<Sheet>,
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
            *open = Some(Sheet::PasteCode);
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
            *open = Some(Sheet::DeletePalette);
        }
    });
}
