use crate::app::ui_state::palette::{CustomPalette, PaletteChoice};
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::settings::content::LoadedGame;
use crate::icons;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Color32, Rect, RichText, Sense, Ui, vec2};

#[derive(Default)]
pub struct PaletteEditorState {
    editing: Option<String>,
    name_buffer: String,
    code_buffer: String,
    code_error: bool,
}

impl PaletteEditorState {
    fn focus(&mut self, name: &str, palette: &CustomPalette) {
        if self.editing.as_deref() == Some(name) {
            return;
        }
        self.editing = Some(name.to_string());
        self.name_buffer = palette.name.clone();
        self.code_buffer = palette.to_code();
        self.code_error = false;
    }

    fn forget(&mut self) {
        self.editing = None;
        self.name_buffer.clear();
        self.code_buffer.clear();
        self.code_error = false;
    }
}

enum Action {
    Create(CustomPalette),
    Delete(String),
    Rename {
        from: String,
        to: String,
    },
    Recolor {
        name: String,
        colors: [[u8; 3]; 4],
    },
    SetGlobal(PaletteChoice),
    SetGame {
        rom_key: String,
        choice: Option<PaletteChoice>,
    },
}

pub fn show(
    ui: &mut Ui,
    settings: &mut Settings,
    state: &mut PaletteEditorState,
    game: Option<LoadedGame>,
) {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.label("Palette")
            .on_hover_text("Applies to every game without a palette of its own");
        if let Some(chosen) = picker(ui, "dmg_palette_picker", &global_choice(settings), settings) {
            action = Some(Action::SetGlobal(chosen));
        }
    });

    preview(ui, &settings.effective_theme());
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui
            .button(format!("{} New", icons::PLUS))
            .on_hover_text("Start from a blank greyscale")
            .clicked()
        {
            let name = settings.palettes.unique_name("My palette");
            action = Some(Action::Create(CustomPalette::from_theme(
                name,
                &DmgTheme::GreyScale,
            )));
        }

        if ui
            .button(format!("{} Duplicate", icons::COPY))
            .on_hover_text("Make an editable copy of the palette shown")
            .clicked()
        {
            let shown = settings.effective_theme();
            let base = settings
                .custom_palette
                .clone()
                .unwrap_or_else(|| settings.dmg_theme.to_string());
            let name = settings.palettes.unique_name(&base);
            action = Some(Action::Create(CustomPalette::from_theme(name, &shown)));
        }

        if let Some(selected) = settings.custom_palette.clone()
            && settings.palettes.contains(&selected)
            && ui.button(format!("{} Delete", icons::TRASH)).clicked()
        {
            action = Some(Action::Delete(selected));
        }
    });

    if let Some(selected) = settings.custom_palette.clone()
        && let Some(palette) = settings.palettes.get(&selected)
    {
        state.focus(&selected, palette);
        ui.add_space(8.0);
        editor(ui, palette, state, &mut action);
    } else {
        state.forget();
        ui.add_space(4.0);
        ui.small("You can't edit a built-in palette. Duplicate one to make it yours.");
    }

    if let Some(game) = game {
        game_section(ui, settings, game, &mut action);
    }

    if let Some(action) = action {
        apply(action, settings, state);
    }
}

fn global_choice(settings: &Settings) -> PaletteChoice {
    match &settings.custom_palette {
        Some(name) => PaletteChoice::Custom(name.clone()),
        None => PaletteChoice::Builtin(settings.dmg_theme),
    }
}

fn choice_label(choice: &PaletteChoice) -> String {
    match choice {
        PaletteChoice::Builtin(theme) => theme.to_string(),
        PaletteChoice::Custom(name) => name.clone(),
    }
}

fn picker(
    ui: &mut Ui,
    id: &str,
    selected: &PaletteChoice,
    settings: &Settings,
) -> Option<PaletteChoice> {
    let mut chosen = None;

    egui::ComboBox::from_id_salt(id)
        .selected_text(choice_label(selected))
        .show_ui(ui, |ui| {
            for theme in DmgTheme::SELECTABLE {
                let is_selected = *selected == PaletteChoice::Builtin(*theme);
                if ui
                    .selectable_label(is_selected, theme.to_string())
                    .clicked()
                {
                    chosen = Some(PaletteChoice::Builtin(*theme));
                }
            }

            if !settings.palettes.is_empty() {
                ui.separator();
                for entry in settings.palettes.entries() {
                    let is_selected = *selected == PaletteChoice::Custom(entry.name.clone());
                    if ui.selectable_label(is_selected, &entry.name).clicked() {
                        chosen = Some(PaletteChoice::Custom(entry.name.clone()));
                    }
                }
            }
        });

    chosen
}

fn game_section(ui: &mut Ui, settings: &Settings, game: LoadedGame, action: &mut Option<Action>) {
    ui.add_space(12.0);
    ui.separator();

    let title = if game.title.is_empty() {
        "This game"
    } else {
        game.title
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::GAME_CONTROLLER).size(15.0));
        ui.label(RichText::new(title).strong().size(15.0));
    });
    ui.add_space(4.0);

    let own = settings.game_palette(Some(game.rom_key)).cloned();
    let global = global_choice(settings);

    ui.horizontal(|ui| {
        if ui
            .radio(
                own.is_none(),
                format!("Follow the palette above ({})", choice_label(&global)),
            )
            .clicked()
            && own.is_some()
        {
            *action = Some(Action::SetGame {
                rom_key: game.rom_key.to_string(),
                choice: None,
            });
        }
    });

    ui.horizontal(|ui| {
        if ui.radio(own.is_some(), "Use just for this game").clicked() && own.is_none() {
            *action = Some(Action::SetGame {
                rom_key: game.rom_key.to_string(),
                choice: Some(global.clone()),
            });
        }

        let shown = own.clone().unwrap_or_else(|| global.clone());
        if let Some(chosen) = picker(ui, "game_palette_picker", &shown, settings) {
            *action = Some(Action::SetGame {
                rom_key: game.rom_key.to_string(),
                choice: Some(chosen),
            });
        }
    });

    if own.is_some() {
        ui.add_space(4.0);
        preview(ui, &settings.theme_for(Some(game.rom_key)));
    }
}

fn editor(
    ui: &mut Ui,
    palette: &CustomPalette,
    state: &mut PaletteEditorState,
    action: &mut Option<Action>,
) {
    egui::Grid::new("palette_editor_grid")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Name");
            let response = ui.text_edit_singleline(&mut state.name_buffer);
            if response.lost_focus()
                || response.changed() && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                action.get_or_insert(Action::Rename {
                    from: palette.name.clone(),
                    to: state.name_buffer.clone(),
                });
            }
            ui.end_row();

            ui.label("Colours");
            ui.horizontal(|ui| {
                let mut colors = palette.colors;
                let mut changed = false;
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
            ui.end_row();

            ui.label("Share code");
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.code_buffer)
                        .desired_width(190.0)
                        .font(egui::TextStyle::Monospace),
                );
                if response.changed() {
                    state.code_error = false;
                }
                if ui
                    .button(format!("{} Apply", icons::CHECK))
                    .on_hover_text("Read four colours out of the code")
                    .clicked()
                {
                    match CustomPalette::parse_code(&palette.name, &state.code_buffer) {
                        Some(parsed) => {
                            state.code_error = false;
                            action.get_or_insert(Action::Recolor {
                                name: palette.name.clone(),
                                colors: parsed.colors,
                            });
                        }
                        None => state.code_error = true,
                    }
                }
                if ui
                    .button(icons::COPY)
                    .on_hover_text("Copy to clipboard")
                    .clicked()
                {
                    ui.ctx().copy_text(palette.to_code());
                }
            });
            ui.end_row();
        });

    if state.code_error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "That code needs four colours, lightest first, like F2CE44-B89D32-72611C-292406.",
        );
    } else {
        ui.small("Paste a code from someone else, or copy yours to share it.");
    }
}

const SHADE_NAMES: [&str; 4] = ["Lightest", "Light", "Dark", "Darkest"];

fn preview(ui: &mut Ui, theme: &DmgTheme) {
    let height = 26.0;
    let width = ui.available_width().min(240.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let swatch = width / 4.0;

    for (index, color) in theme.palette().iter().enumerate() {
        let cell = Rect::from_min_size(
            rect.min + vec2(swatch * index as f32, 0.0),
            vec2(swatch, height),
        );
        ui.painter().rect_filled(
            cell,
            0.0,
            Color32::from_rgb(color.r(), color.g(), color.b()),
        );
    }
    ui.painter().rect_stroke(
        rect,
        2.0,
        ui.visuals().widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
}

fn apply(action: Action, settings: &mut Settings, state: &mut PaletteEditorState) {
    match action {
        Action::Create(palette) => {
            settings.custom_palette = Some(palette.name.clone());
            settings.palettes.upsert(palette);
            state.forget();
        }
        Action::Delete(name) => {
            settings.palettes.remove(&name);
            settings.custom_palette = None;
            state.forget();
        }
        Action::Rename { from, to } => {
            settings.rename_palette(&from, &to);
            state.forget();
        }
        Action::Recolor { name, colors } => {
            if let Some(palette) = settings.palettes.get_mut(&name) {
                palette.colors = colors;
            }
            state.code_buffer = settings
                .palettes
                .get(&name)
                .map(|p| p.to_code())
                .unwrap_or_default();
        }
        Action::SetGlobal(choice) => match choice {
            PaletteChoice::Builtin(theme) => {
                settings.dmg_theme = theme;
                settings.custom_palette = None;
            }
            PaletteChoice::Custom(name) => settings.custom_palette = Some(name),
        },
        Action::SetGame { rom_key, choice } => settings.set_game_palette(&rom_key, choice),
    }
    settings.dirty = true;
}
