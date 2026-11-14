use super::{Direction, HudSettings};
use crate::app::tabs::TAB_PADDING;
use crate::app::widgets::menu_row::menu_row_with;
use crate::icons;
use egui::{Id, Rect, Ui, WidgetText};

pub const MENU_WIDTH: f32 = 240.0;
const SUMMARY: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Toggle,
    Edit(u32),
    New,
    More,
}

pub fn label() -> String {
    format!("{}  HUD  {}", icons::GAUGE, icons::CARET_DOWN)
}

pub fn button<R>(
    ui: &Ui,
    id: Id,
    after: Rect,
    label: impl Into<WidgetText>,
    hover: &str,
    width: f32,
    list: impl FnOnce(&mut Ui) -> Option<R>,
) -> (Option<R>, Rect) {
    let menu = id.with("menu");
    let open = egui::Popup::is_id_open(ui.ctx(), menu);
    let galley = label
        .into()
        .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button);
    let size = egui::vec2(galley.size().x + TAB_PADDING, after.height());
    let shown = egui::Area::new(id)
        .order(egui::Order::Middle)
        .constrain(false)
        .fixed_pos(after.right_top())
        .show(ui.ctx(), |ui| {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            let painter = ui.painter();
            let widgets = &ui.visuals().widgets;
            if open || response.hovered() {
                painter.rect_filled(
                    rect.shrink2(egui::vec2(2.0, 3.0)),
                    widgets.hovered.corner_radius,
                    widgets.hovered.weak_bg_fill,
                );
            }
            let text = rect.center() - galley.size() / 2.0;
            painter.galley(text, galley, widgets.inactive.text_color());
            let response = response.on_hover_text(hover);
            let picked = egui::Popup::menu(&response)
                .id(menu)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(width)
                .show(list)
                .and_then(|shown| shown.inner);
            (picked, rect)
        });
    shown.inner
}

pub fn summary(text: String) -> String {
    if text.chars().count() <= SUMMARY {
        return text;
    }
    let cut: String = text.chars().take(SUMMARY - 1).collect();
    format!("{}\u{2026}", cut.trim_end_matches([',', ' ']))
}

pub fn list(ui: &mut Ui, settings: &HudSettings, key: Option<&str>, ds: bool) -> Option<Pick> {
    let mut picked = None;
    let show = menu_row_with(
        ui,
        format!("{}  Show HUDs", icons::EYE),
        key,
        Some(settings.shown),
    );
    if show.clicked() {
        picked = Some(Pick::Toggle);
    }
    for hud in &settings.huds {
        let icon = match hud.direction {
            Direction::Row => icons::SQUARE_SPLIT_HORIZONTAL,
            Direction::Column => icons::SQUARE_SPLIT_VERTICAL,
        };
        let row = menu_row_with(
            ui,
            format!("{icon}  {}", summary(hud.summary(ds))),
            None,
            None,
        )
        .on_hover_text(hud.summary(ds));
        if row.clicked() {
            picked = Some(Pick::Edit(hud.id));
        }
    }
    if menu_row_with(ui, format!("{}  New HUD", icons::PLUS), None, None).clicked() {
        picked = Some(Pick::New);
    }
    ui.separator();
    if menu_row_with(
        ui,
        format!("{}  More HUD settings", icons::SLIDERS_HORIZONTAL),
        Some(icons::CARET_RIGHT),
        None,
    )
    .clicked()
    {
        picked = Some(Pick::More);
    }
    if matches!(picked, Some(Pick::Edit(_) | Pick::New | Pick::More)) {
        ui.close();
    }
    picked
}

pub fn apply(pick: Pick, settings: &mut HudSettings, state: &mut super::HudState) {
    match pick {
        Pick::Toggle => settings.shown = !settings.shown,
        Pick::Edit(id) => {
            settings.shown = true;
            state.editor = Some(id);
        }
        Pick::New => {
            settings.shown = true;
            state.editor = Some(settings.add());
        }
        Pick::More => {}
    }
}
