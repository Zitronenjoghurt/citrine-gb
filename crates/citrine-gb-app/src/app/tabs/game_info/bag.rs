use super::Pages;
use super::parts::{card, text_width};
use citrine_gb_gamedata::{GameState, ItemStack};
use egui::{Align, Layout, RichText, Ui, vec2};

#[derive(Clone, Copy, PartialEq)]
enum Pocket {
    Counted,
    Machines,
    HeldOnce,
}

impl Pages<'_> {
    pub(super) fn bag(&self, ui: &mut Ui, state: &GameState) {
        let bag = &state.bag;
        let sections: [(&str, &[ItemStack], Pocket); 5] = [
            ("Items", &bag.items, Pocket::Counted),
            ("Balls", &bag.balls, Pocket::Counted),
            ("TMs and HMs", &bag.tms_hms, Pocket::Machines),
            ("Berries", &bag.berries, Pocket::Counted),
            ("Key items", &bag.key_items, Pocket::HeldOnce),
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
            for (title, items, pocket) in sections {
                self.bag_section(ui, title, items, pocket);
                ui.add_space(8.0);
            }
            return;
        }
        let (machines, pockets): (Vec<_>, Vec<_>) = sections
            .into_iter()
            .partition(|(_, _, pocket)| *pocket == Pocket::Machines);
        let mut halves: [Vec<(&str, &[ItemStack], Pocket)>; 2] = [Vec::new(), Vec::new()];
        let mut rows = [0, 0];
        for (title, items, pocket) in pockets {
            let side = usize::from(rows[1] < rows[0]);
            rows[side] += items.len() + 2;
            halves[side].push((title, items, pocket));
        }
        if halves[1].is_empty() {
            for (title, items, pocket) in &halves[0] {
                self.bag_section(ui, title, items, *pocket);
                ui.add_space(8.0);
            }
        } else {
            ui.columns(2, |columns| {
                for (column, half) in columns.iter_mut().zip(&halves) {
                    for (title, items, pocket) in half {
                        self.bag_section(column, title, items, *pocket);
                        column.add_space(8.0);
                    }
                }
            });
        }
        for (title, items, pocket) in machines {
            self.bag_section(ui, title, items, pocket);
            ui.add_space(8.0);
        }
    }

    fn bag_section(&self, ui: &mut Ui, title: &str, items: &[ItemStack], pocket: Pocket) {
        let tables = &self.game.tables;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).weak());
            let machine_width = text_width(ui, RichText::new("HM00"));
            for stack in items {
                ui.horizontal(|ui| {
                    let name = tables.item_name(stack.item).unwrap_or_default();
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pocket != Pocket::HeldOnce && !tables.is_hm(stack.item) {
                            ui.label(RichText::new(format!("× {}", stack.count)).weak());
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            if pocket == Pocket::Machines {
                                ui.allocate_ui_with_layout(
                                    vec2(machine_width, ui.spacing().interact_size.y),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_min_width(machine_width);
                                        ui.label(name);
                                    },
                                );
                                if let Some(name) = tables.machine_move(stack.item) {
                                    ui.add(egui::Label::new(name).truncate());
                                }
                            } else {
                                ui.add(egui::Label::new(name).truncate());
                            }
                        });
                    });
                });
            }
        });
    }
}
