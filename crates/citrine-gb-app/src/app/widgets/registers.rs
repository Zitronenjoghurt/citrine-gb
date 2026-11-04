use crate::app::widgets::settings::form::Form;
use citrine_gb::debug::registers::{Register, RegisterGroup};
use egui::{CollapsingHeader, Label, Response, RichText, Ui, Widget};
use std::collections::BTreeMap;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RegistersState {
    #[serde(default)]
    pub cpu: bool,
    #[serde(default)]
    pub ir: bool,
    #[serde(default)]
    pub ppu: bool,
    #[serde(default)]
    pub timer: bool,
    #[serde(default)]
    pub gbc: bool,
    #[serde(default)]
    pub docked_cpu: bool,
    #[serde(default)]
    pub groups: BTreeMap<String, bool>,
}

impl Default for RegistersState {
    fn default() -> Self {
        Self {
            cpu: true,
            ir: false,
            ppu: false,
            timer: false,
            gbc: false,
            docked_cpu: false,
            groups: BTreeMap::new(),
        }
    }
}

impl RegistersState {
    pub fn open_mut(&mut self, group: &str) -> &mut bool {
        match group {
            "CPU" => &mut self.cpu,
            "Interrupts" => &mut self.ir,
            "PPU" => &mut self.ppu,
            "Timer" => &mut self.timer,
            "GBC" => &mut self.gbc,
            _ => self.groups.entry(group.to_owned()).or_default(),
        }
    }
}

pub struct RegisterTable<'a> {
    groups: &'a [RegisterGroup],
    state: &'a mut RegistersState,
    bare: bool,
}

impl<'a> RegisterTable<'a> {
    pub fn new(groups: &'a [RegisterGroup], state: &'a mut RegistersState) -> Self {
        Self {
            groups,
            state,
            bare: false,
        }
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

impl Widget for RegisterTable<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            for (index, group) in self.groups.iter().enumerate() {
                if self.bare && index == 0 {
                    rows(ui, group);
                    continue;
                }
                let open = self.state.open_mut(group.name);
                let shown = CollapsingHeader::new(RichText::new(group.name).strong())
                    .id_salt(("registers", group.name))
                    .open(Some(*open))
                    .show(ui, |ui| rows(ui, group));
                if shown.header_response.clicked() {
                    *open = !*open;
                }
            }
        })
        .response
    }
}

fn rows(ui: &mut Ui, group: &RegisterGroup) {
    Form::show(ui, ("registers", group.name), |form| {
        form.striped();
        for register in &group.rows {
            form.row(register.name, |ui| value(ui, register));
            if !register.fields.is_empty() {
                form.under(|ui| fields(ui, register));
            }
        }
    });
}

fn value(ui: &mut Ui, register: &Register) -> Response {
    ui.add(Label::new(RichText::new(register.hex()).monospace()))
        .on_hover_text(format!(
            "Decimal {}\nBinary {}",
            register.value,
            register.binary()
        ))
}

fn fields(ui: &mut Ui, register: &Register) {
    ui.spacing_mut().interact_size.y = 0.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for field in register.fields {
            let value = field.read(register.value);
            let flag = field.width == 1 && field.values.is_empty();
            let text = if flag {
                RichText::new(field.name)
            } else if let Some(name) = field.value_name(value) {
                RichText::new(format!("{} {name}", field.name))
            } else {
                RichText::new(format!("{} {value}", field.name))
            };
            let text = match (flag, value) {
                (true, 0) => text.small().weak(),
                (true, _) => text.small().strong(),
                (false, _) => text.small(),
            };
            ui.add(Label::new(text).wrap_mode(egui::TextWrapMode::Extend))
                .on_hover_text(format!("{} is {value}", field.name));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_stored_state_still_loads() {
        let today = r#"{"cpu":false,"ir":true,"ppu":true,"timer":false}"#;

        let mut state: RegistersState = serde_json::from_str(today).expect("loads");

        assert!(!*state.open_mut("CPU"));
        assert!(*state.open_mut("Interrupts"));
        assert!(*state.open_mut("PPU"));
        assert!(!*state.open_mut("Timer"));
        assert!(!*state.open_mut("GBC"));
    }

    #[test]
    fn a_new_state_opens_the_cpu_only() {
        let mut state = RegistersState::default();

        assert!(*state.open_mut("CPU"));
        assert!(!*state.open_mut("PPU"));
    }

    #[test]
    fn the_stored_state_keeps_the_gbc_group() {
        let mut state = RegistersState::default();
        *state.open_mut("GBC") = true;

        let json = serde_json::to_string(&state).expect("saves");
        let mut back: RegistersState = serde_json::from_str(&json).expect("loads");

        assert!(*back.open_mut("GBC"));
    }

    #[test]
    fn a_group_without_its_own_flag_starts_closed_and_remembers() {
        let mut state = RegistersState::default();
        assert!(!*state.open_mut("FIQ"));
        *state.open_mut("FIQ") = true;

        let json = serde_json::to_string(&state).expect("saves");
        let mut back: RegistersState = serde_json::from_str(&json).expect("loads");

        assert!(*back.open_mut("FIQ"));
        assert!(!*back.open_mut("IRQ"));
    }
}
