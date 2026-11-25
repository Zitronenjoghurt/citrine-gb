use crate::app::ui_state::palette::{CustomPalette, PaletteChoice};
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::palette_strip::palette_strip;
use crate::app::widgets::reset_slider::{reset_room, with_reset};
use crate::app::widgets::settings::form::Form;
use crate::icons;
use citrine_gb::gb::ppu::compat::CompatPalette;
use citrine_gb::gb::ppu::types::color::RGBA;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Align, Layout, RichText, Ui};

#[derive(Default)]
pub struct PaletteEditorState {
    editing: Option<String>,
    pub(crate) name_buffer: String,
    pub(crate) code_buffer: String,
    pub(crate) code_error: bool,
}

impl PaletteEditorState {
    pub(crate) fn focus(&mut self, name: &str, palette: &CustomPalette) {
        if self.editing.as_deref() == Some(name) {
            return;
        }
        self.editing = Some(name.to_string());
        self.name_buffer = palette.name.clone();
        self.code_buffer = palette.to_code();
        self.code_error = false;
    }

    pub(crate) fn forget(&mut self) {
        self.editing = None;
        self.name_buffer.clear();
        self.code_buffer.clear();
        self.code_error = false;
    }
}

pub(crate) enum Action {
    Create(CustomPalette),
    Delete(String),
    Rename { from: String, to: String },
    Recolor { name: String, colors: [[u8; 3]; 4] },
    SetGlobal(PaletteChoice),
}

pub fn palette_row(form: &mut Form, settings: &Settings, label: &str) -> Option<Action> {
    let mut action = None;
    form.row_with_hint(
        label,
        "Applies to every game without a palette of its own. Built-in palettes can't be edited; duplicate one to make it yours.",
        |ui| {
            crate::app::widgets::settings::form::on_select_edge(ui);
            let global = settings.global_choice();
            let default = PaletteChoice::Builtin(DmgTheme::default());
            let reset = with_reset(ui, global != default, choice_label(&default), |ui| {
                if let Some(chosen) = picker(ui, "dmg_palette_picker", &global, settings) {
                    action = Some(Action::SetGlobal(chosen));
                }
            });
            if reset {
                action = Some(Action::SetGlobal(default));
            }
        },
    );
    form.under_value(|ui| {
        ui.set_max_width((ui.available_width() - reset_room(ui)).max(0.0));
        palette_strip(ui, &choice_colors(settings, &settings.global_choice(), &[]))
    });
    action
}

pub fn own_palettes(
    form: &mut Form,
    settings: &Settings,
    state: &mut PaletteEditorState,
    label: &str,
) -> Option<Action> {
    let mut action = None;
    form.row(label, |ui| {
        crate::app::widgets::settings::form::on_select_edge(ui);
        let widest = [
            format!("{} New palette", icons::PLUS),
            format!("{} Duplicate", icons::COPY),
            format!("{} Delete", icons::TRASH),
        ]
        .into_iter()
        .map(|text| {
            egui::WidgetText::from(text)
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Button,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
        super::form::need(ui, widest + 2.0 * ui.spacing().button_padding.x);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(format!("{} New palette", icons::PLUS))
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
                let global = settings.global_choice();
                let shown = DmgTheme::Custom(choice_colors(settings, &global, &[])[0]);
                let base = choice_label(&global);
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
    });

    if let Some(selected) = settings.custom_palette.clone()
        && let Some(palette) = settings.palettes.get(&selected)
    {
        state.focus(&selected, palette);
        editor(form, palette, state, &mut action);
    } else {
        state.forget();
    }
    action
}

pub(crate) fn choice_label(choice: &PaletteChoice) -> String {
    match choice {
        PaletteChoice::Builtin(theme) => theme.to_string(),
        PaletteChoice::Custom(name) => name.clone(),
        PaletteChoice::Gbc(palette) => palette.to_string(),
    }
}

pub(crate) fn choice_colors(
    settings: &Settings,
    choice: &PaletteChoice,
    header: &[u8],
) -> Vec<[RGBA; 4]> {
    let theme = match choice {
        PaletteChoice::Builtin(theme) => *theme,
        PaletteChoice::Custom(name) => settings
            .palettes
            .get(name)
            .map(|palette| palette.to_theme())
            .unwrap_or_else(|| settings.effective_theme()),
        PaletteChoice::Gbc(palette) => return palette.colors(header).to_vec(),
    };
    vec![*theme.palette()]
}

pub(crate) fn all_choices(settings: &Settings) -> Vec<PaletteChoice> {
    DmgTheme::SELECTABLE
        .iter()
        .map(|theme| PaletteChoice::Builtin(*theme))
        .chain(
            CompatPalette::SELECTABLE
                .iter()
                .map(|palette| PaletteChoice::Gbc(*palette)),
        )
        .chain(
            settings
                .palettes
                .entries()
                .iter()
                .map(|entry| PaletteChoice::Custom(entry.name.clone())),
        )
        .collect()
}

fn picker(
    ui: &mut Ui,
    id: &str,
    selected: &PaletteChoice,
    settings: &Settings,
) -> Option<PaletteChoice> {
    let mut chosen = None;
    let text = choice_label(selected);
    let spacing = ui.spacing();
    let chrome = 2.0 * spacing.button_padding.x + spacing.icon_width + spacing.icon_spacing;
    let reset = reset_room(ui);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let current = ui.ctx().fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.clone(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    });
    let floor = if super::form::selects_shrink(ui) {
        current.min((super::form::SELECT_MIN - chrome).max(0.0))
    } else {
        current
    };
    super::form::cut_select_need(ui, current + chrome + reset, floor + chrome + reset);

    let shown = crate::app::widgets::generic_select::shorten_end(
        &text,
        ui.available_width() - chrome,
        |part| {
            ui.ctx().fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(part.to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
        },
    );
    let cut = shown != text;
    let combo = egui::ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .truncate()
        .selected_text(shown)
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

            ui.separator();
            for palette in CompatPalette::SELECTABLE {
                let is_selected = *selected == PaletteChoice::Gbc(*palette);
                if ui
                    .selectable_label(is_selected, palette.to_string())
                    .clicked()
                {
                    chosen = Some(PaletteChoice::Gbc(*palette));
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
    if cut {
        combo.response.on_hover_text(text);
    }

    chosen
}

fn editor(
    form: &mut Form,
    palette: &CustomPalette,
    state: &mut PaletteEditorState,
    action: &mut Option<Action>,
) {
    form.row("Name", |ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut state.name_buffer).desired_width(ui.available_width()),
        );
        if response.lost_focus()
            || response.changed() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            action.get_or_insert(Action::Rename {
                from: palette.name.clone(),
                to: state.name_buffer.clone(),
            });
        }
    });

    form.row("Colours", |ui| {
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

    form.row_with_hint("Share code", SHARE_HINT, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(icons::COPY)
                .on_hover_text("Copy to clipboard")
                .clicked()
            {
                ui.ctx().copy_text(palette.to_code());
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
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.code_buffer)
                    .desired_width(ui.available_width())
                    .font(egui::TextStyle::Monospace),
            );
            if response.changed() {
                state.code_error = false;
            }
        });
    });
    if state.code_error {
        let error = form.ctx().style().visuals.error_fg_color;
        form.hint(
            RichText::new(
                "That code needs four colours, lightest first, like F2CE44-B89D32-72611C-292406.",
            )
            .color(error),
        );
    }
}

pub(crate) const SHARE_HINT: &str = "Paste a code from someone else, or copy yours to share it.";

pub(crate) const SHADE_NAMES: [&str; 4] = ["Lightest", "Light", "Dark", "Darkest"];

pub(crate) fn apply(action: Action, settings: &mut Settings, state: &mut PaletteEditorState) {
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
        Action::SetGlobal(choice) => settings.set_global_choice(choice),
    }
    settings.dirty = true;
}
