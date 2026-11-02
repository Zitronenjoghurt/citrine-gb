use super::Pages;
use super::parts::{card, text_width};
use citrine_gb_gamedata::{GameState, ItemStack};
use egui::{Align, Layout, RichText, Ui, vec2};

impl Pages<'_> {
    pub(super) fn bag(&self, ui: &mut Ui, state: &GameState) {
        let bag = &state.bag;
        let sections: [(&str, &[ItemStack], bool); 4] = [
            ("Items", &bag.items, false),
            ("Key items", &bag.key_items, false),
            ("Balls", &bag.balls, false),
            ("TMs and HMs", &bag.tms_hms, true),
        ];
        let sections: Vec<_> = sections
            .into_iter()
            .filter(|(_, items, _)| !items.is_empty())
            .collect();
        if sections.is_empty() {
            ui.label(RichText::new("The bag is empty").weak());
            return;
        }
        if self.mobile || sections.len() == 1 {
            for (title, items, machines) in sections {
                self.bag_section(ui, title, items, machines);
                ui.add_space(8.0);
            }
            return;
        }
        let (left, right) = sections.split_at(1.max(sections.len() / 2));
        ui.columns(2, |columns| {
            for (column, half) in columns.iter_mut().zip([left, right]) {
                for (title, items, machines) in half {
                    self.bag_section(column, title, items, *machines);
                    column.add_space(8.0);
                }
            }
        });
    }

    pub(super) fn bag_section(
        &self,
        ui: &mut Ui,
        title: &str,
        items: &[ItemStack],
        machines: bool,
    ) {
        let tables = &self.game.tables;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).weak());
            let machine_width = text_width(ui, RichText::new("HM00"));
            for stack in items {
                ui.horizontal(|ui| {
                    let name = tables.item_name(stack.item).unwrap_or_default();
                    if machines {
                        ui.allocate_ui_with_layout(
                            vec2(machine_width, ui.spacing().interact_size.y),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_min_width(machine_width);
                                ui.label(name);
                            },
                        );
                        if let Some(name) = tables.machine_move(stack.item) {
                            ui.label(name);
                        }
                    } else {
                        ui.label(name);
                    }
                    if !tables.is_hm(stack.item) {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("× {}", stack.count)).weak());
                        });
                    }
                });
            }
        });
    }
}
