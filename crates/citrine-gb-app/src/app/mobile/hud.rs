use crate::app::Citrine;
use crate::app::hud::cells::{Facts, cell};
use crate::app::hud::chip::summary;
use crate::app::hud::editor::{CARD_HINT, drop_index};
use crate::app::hud::paint::Look;
use crate::app::hud::{DEFAULT_OPACITY, Direction, Group, Hud, SIZES, Value, addable};
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::mobile::sheet::{self, Amount};
use crate::app::ui_state::settings_tree::GroupId;
use crate::icons;
use egui::{Align, Context, Id, Layout, RichText, Sense, Stroke, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudSheet {
    List,
    Edit(u32),
    Size(u32),
    Card(u32),
    Add(u32),
}

impl HudSheet {
    fn back(self) -> Option<HudSheet> {
        match self {
            HudSheet::List | HudSheet::Edit(_) => None,
            HudSheet::Size(id) | HudSheet::Card(id) | HudSheet::Add(id) => Some(HudSheet::Edit(id)),
        }
    }
}

enum Picked {
    Toggle,
    Edit(u32),
    New,
    More,
}

enum Change {
    Sheet(HudSheet),
    Remove(u32),
}

fn percent(value: f64) -> String {
    format!("{value:.0}%")
}

fn live(value: Value, facts: &Facts) -> String {
    if value == Value::Graph {
        return String::new();
    }
    let cell = cell(value, facts);
    match cell.unit.as_ref() {
        "" => cell.value,
        "%" => format!("{}%", cell.value),
        unit => format!("{} {unit}", cell.value),
    }
}

impl Citrine {
    pub(super) fn hud_sheet(&mut self, ctx: &Context) {
        let Some(open) = self.mobile.hud_sheet else {
            return;
        };
        if !self.mobile.nav.on_game() {
            self.mobile.hud_sheet = None;
            return;
        }
        let next = match open {
            HudSheet::List => self.hud_list_sheet(ctx),
            HudSheet::Edit(id) => self.hud_edit_sheet(ctx, id),
            HudSheet::Size(id) => self.hud_amount_sheet(ctx, id, false),
            HudSheet::Card(id) => self.hud_amount_sheet(ctx, id, true),
            HudSheet::Add(id) => self.hud_add_sheet(ctx, id),
        };
        if let Some(next) = next {
            self.mobile.hud_sheet = next;
        }
    }

    fn hud_list_sheet(&mut self, ctx: &Context) -> Option<Option<HudSheet>> {
        let ds = self.emulator.played_kind() == citrine_gb::console::ConsoleKind::Nds;
        let settings = &self.ui.settings.hud;
        let shown = sheet::show(ctx, Id::new("hud_list_sheet"), "HUD", |ui| {
            let mut picked = None;
            kit::list(ui, |list| {
                let mut on = settings.shown;
                if list.switch(SwitchRow::new("Show HUDs", &mut on)) {
                    picked = Some(Picked::Toggle);
                }
                for hud in &settings.huds {
                    let icon = match hud.direction {
                        Direction::Row => icons::SQUARE_SPLIT_HORIZONTAL,
                        Direction::Column => icons::SQUARE_SPLIT_VERTICAL,
                    };
                    let label = summary(hud.summary(ds));
                    if list.row(Row::new(&label).icon(icon)).clicked() {
                        picked = Some(Picked::Edit(hud.id));
                    }
                }
                if list
                    .row(Row::new("New HUD").icon(icons::PLUS).no_chevron())
                    .clicked()
                {
                    picked = Some(Picked::New);
                }
            });
            ui.add_space(12.0);
            kit::list(ui, |list| {
                if list
                    .row(Row::new("More HUD settings").icon(icons::SLIDERS_HORIZONTAL))
                    .clicked()
                {
                    picked = Some(Picked::More);
                }
            });
            picked
        });
        let hud = &mut self.ui.settings.hud;
        let next = match shown.inner {
            Some(Picked::Toggle) => {
                hud.shown = !hud.shown;
                self.ui.settings.dirty = true;
                None
            }
            Some(Picked::Edit(id)) => {
                hud.shown = true;
                Some(Some(HudSheet::Edit(id)))
            }
            Some(Picked::New) => {
                hud.shown = true;
                let id = hud.add();
                self.ui.settings.dirty = true;
                Some(Some(HudSheet::Edit(id)))
            }
            Some(Picked::More) => {
                self.find_group(GroupId::Huds, true);
                Some(None)
            }
            None => None,
        };
        next.or(shown.closed.then_some(None))
    }

    fn hud_edit_sheet(&mut self, ctx: &Context, id: u32) -> Option<Option<HudSheet>> {
        let ds = self.emulator.played_kind() == citrine_gb::console::ConsoleKind::Nds;
        let facts = crate::app::hud::cells::gather(
            crate::app::hud::cells::Sources {
                frame_log: &mut self.ui.frame_log,
                load: self.ui.performance.view(),
                emulator: &self.emulator,
                stats: &self.stats,
                play: &self.play,
                online: &mut self.online,
            },
            &mut self.ui.hud.cells,
            &self.ui.settings.hud,
        );
        let opacity = self.ui.settings.hud.opacity();
        let look = Look::of(&ctx.style().visuals, opacity);
        let Some(hud) = self.ui.settings.hud.get_mut(id) else {
            return Some(None);
        };
        let mut changed = false;
        let shown = sheet::show(ctx, Id::new("hud_edit_sheet"), "HUD", |ui| {
            let mut change = None;
            kit::section_label(ui, "Layout");
            if let Some(direction) = sheet::choices(
                ui,
                [Direction::Row, Direction::Column],
                hud.direction,
                |direction| match direction {
                    Direction::Row => "Row".into(),
                    Direction::Column => "Column".into(),
                },
            ) {
                hud.direction = direction;
                changed = true;
            }
            ui.add_space(12.0);
            kit::list(ui, |list| {
                let size = (hud.scale() * 100.0).round();
                if list
                    .row(Row::new("Size").value(percent(size.into())))
                    .clicked()
                {
                    change = Some(Change::Sheet(HudSheet::Size(id)));
                }
                let card = (opacity * 100.0).round();
                if list
                    .row(Row::new("Card").value(percent(card.into())).info(CARD_HINT))
                    .clicked()
                {
                    change = Some(Change::Sheet(HudSheet::Card(id)));
                }
            });
            kit::section_label(ui, "Values");
            kit::list(ui, |list| {
                changed |= value_rows(list, hud, &facts, look);
                let more = Group::ALL
                    .into_iter()
                    .any(|group| addable(&hud.values, ds, group).next().is_some());
                if more
                    && list
                        .row(Row::new("Add a value").icon(icons::PLUS))
                        .clicked()
                {
                    change = Some(Change::Sheet(HudSheet::Add(id)));
                }
            });
            ui.add_space(12.0);
            kit::list(ui, |list| {
                if list
                    .row(
                        Row::new("Remove HUD")
                            .icon(icons::TRASH)
                            .danger()
                            .no_chevron(),
                    )
                    .clicked()
                {
                    change = Some(Change::Remove(id));
                }
            });
            change
        });
        if changed {
            self.ui.settings.dirty = true;
        }
        match shown.inner {
            Some(Change::Sheet(sheet)) => Some(Some(sheet)),
            Some(Change::Remove(id)) => {
                self.remove_hud(id);
                Some(None)
            }
            None => shown.closed.then_some(None),
        }
    }

    fn hud_amount_sheet(&mut self, ctx: &Context, id: u32, card: bool) -> Option<Option<HudSheet>> {
        let settings = &mut self.ui.settings.hud;
        let (title, mut value, range, default) = if card {
            (
                "Card",
                (settings.opacity() * 100.0).round(),
                0.0..=100.0,
                (DEFAULT_OPACITY * 100.0).round(),
            )
        } else {
            let size = settings.get(id).map_or(1.0, Hud::scale);
            (
                "Size",
                (size * 100.0).round(),
                SIZES.start() * 100.0..=SIZES.end() * 100.0,
                100.0,
            )
        };
        let shown = sheet::show(ctx, Id::new(("hud_amount_sheet", card)), title, |ui| {
            if card {
                sheet::note(ui, CARD_HINT);
            }
            sheet::amount(
                ui,
                Amount {
                    value: &mut value,
                    range,
                    step: 5.0,
                    default,
                    text: percent,
                },
            )
        });
        if shown.inner.changed() {
            if card {
                settings.opacity = value / 100.0;
            } else if let Some(hud) = settings.get_mut(id) {
                hud.size = value / 100.0;
            }
            self.ui.settings.dirty = true;
        }
        shown.closed.then(|| HudSheet::Card(id).back())
    }

    fn hud_add_sheet(&mut self, ctx: &Context, id: u32) -> Option<Option<HudSheet>> {
        let ds = self.emulator.played_kind() == citrine_gb::console::ConsoleKind::Nds;
        let Some(hud) = self.ui.settings.hud.get_mut(id) else {
            return Some(None);
        };
        let shown = sheet::show(ctx, Id::new("hud_add_sheet"), "Add a value", |ui| {
            let mut picked = None;
            for group in Group::ALL {
                let values: Vec<Value> = addable(&hud.values, ds, group).collect();
                if values.is_empty() {
                    continue;
                }
                kit::section_label(ui, group.name());
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(sheet::INSET);
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for value in values {
                        if kit::pill(ui, value.name(), false)
                            .on_hover_text(value.hint())
                            .clicked()
                        {
                            picked = Some(value);
                        }
                    }
                });
            }
            ui.add_space(8.0);
            picked
        });
        if let Some(value) = shown.inner {
            hud.values.push(value);
            self.ui.settings.dirty = true;
            return Some(Some(HudSheet::Edit(id)));
        }
        shown.closed.then(|| HudSheet::Add(id).back())
    }
}

fn value_rows(list: &mut kit::List<'_>, hud: &mut Hud, facts: &Facts, look: Look) -> bool {
    let mut moved = None;
    let mut removed = None;
    let rows: Vec<(usize, Value)> = hud
        .values
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, value)| facts.ds || !value.ds_only())
        .collect();
    for (index, value) in rows {
        let response = list.custom(|ui| {
            let height = kit::ROW_HEIGHT.max(ui.spacing().interact_size.y + 6.0);
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.add_space(kit::SIDE - 4.0);
                    ui.dnd_drag_source(Id::new(("hud_phone_value", hud.id, value)), index, |ui| {
                        let (rect, _) = ui.allocate_exact_size(vec2(28.0, height), Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            icons::DOTS_SIX_VERTICAL,
                            egui::FontId::proportional(18.0),
                            ui.visuals().weak_text_color(),
                        );
                    });
                    let (dot, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 4.5, look.group(value.group()));
                    ui.label(RichText::new(value.name()).size(16.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(kit::SIDE - 6.0);
                        let side = ui.spacing().interact_size.y.max(36.0);
                        if ui
                            .add(
                                egui::Button::new(RichText::new(icons::X).size(18.0))
                                    .frame(false)
                                    .min_size(vec2(side, side)),
                            )
                            .clicked()
                        {
                            removed = Some(index);
                        }
                        ui.label(RichText::new(live(value, facts)).size(15.0).weak());
                    });
                },
            )
            .response
        });
        if let Some(from) = response.dnd_hover_payload::<usize>() {
            let below = list
                .ui()
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| pos.y > response.rect.center().y);
            if *from != index {
                let y = if below {
                    response.rect.bottom()
                } else {
                    response.rect.top()
                };
                let color = list.ui().visuals().selection.stroke.color;
                list.ui()
                    .painter()
                    .hline(response.rect.x_range(), y, Stroke::new(2.0_f32, color));
            }
            if let Some(from) = response.dnd_release_payload::<usize>() {
                moved = Some((*from, drop_index(*from, index, below)));
            }
        }
    }
    if let Some((from, to)) = moved {
        hud.move_value(from, to);
        return true;
    }
    if let Some(index) = removed {
        hud.values.remove(index);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::HudSheet;
    use crate::app::Citrine;
    use crate::app::mobile;
    use crate::app::tests::{phone_app, shown_whole, tap, text_containing};
    use crate::app::ui_state::settings_tree::SettingsTab;
    use citrine_gb::gb::GbModel;
    use citrine_gb::rom::Rom;
    use egui::Context;

    fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 480.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.handle_event_queue(touch);
        })
    }

    #[test]
    fn more_hud_settings_opens_the_screen_page_at_its_huds() {
        let mut app = phone_app("hud-more-phone");
        app.emulator
            .load_rom(
                &Rom::new(&[0u8; 0x8000]),
                GbModel::Dmg,
                #[cfg(not(target_arch = "wasm32"))]
                None,
            )
            .expect("load");
        app.mobile.nav.show_game();
        app.mobile.hud_sheet = Some(HudSheet::List);
        let ctx = Context::default();
        let mut output = frame(&mut app, &ctx, Vec::new());
        for _ in 0..20 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        let more = text_containing(&output, "More HUD settings").expect("the sheet entry");
        frame(&mut app, &ctx, tap(more.center()));
        for _ in 0..40 {
            output = frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Screen));
        assert!(
            shown_whole(&output, "HUDs"),
            "the HUDs heading is out of view"
        );
    }
}
