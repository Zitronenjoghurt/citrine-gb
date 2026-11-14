use crate::app::ui_state::ds_screens::{Arrangement, Placement, ScreensAction};
use crate::app::ui_state::ui_theme::ink_on;
use crate::app::widgets::menu_row::menu_row_with;
use crate::icons;
use egui::{Rect, Response, RichText, Sense, Ui, pos2, vec2};
use strum::IntoEnumIterator;

const TILE_HEIGHT: f32 = 62.0;
const TILE_GAP: f32 = 6.0;
const MENU_WIDTH: f32 = 300.0;

pub const TOUCH_NOTE: &str = "Touch needs the bottom screen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picked {
    Act(ScreensAction),
    More,
}

#[derive(Debug, Clone, Default)]
pub struct Keys {
    pub swap: Option<String>,
    pub one: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Facts {
    pub placement: Placement,
    pub separate_tabs: Option<bool>,
    pub keys: Keys,
}

pub fn quick_list(ui: &mut Ui, facts: &Facts) -> Option<Picked> {
    let mut picked = None;
    let placement = facts.placement;
    if facts.separate_tabs == Some(true) {
        if row(ui, icons::BROWSERS, "Separate tabs", None, Some(true)).clicked() {
            picked = Some(Picked::Act(ScreensAction::SeparateTabs));
        }
        ui.separator();
        if row(
            ui,
            icons::SLIDERS_HORIZONTAL,
            "More screen settings",
            Some(icons::CARET_RIGHT),
            None,
        )
        .clicked()
        {
            picked = Some(Picked::More);
        }
        return picked;
    }
    let chosen = (!placement.one_screen).then_some(placement.arrangement);
    if let Some(arrangement) = tiles(ui, chosen) {
        picked = Some(Picked::Act(ScreensAction::Arrange(arrangement)));
    }
    ui.add_space(4.0);
    let swap = row(
        ui,
        placement.swap_icon(),
        "Swap screens",
        facts.keys.swap.as_deref(),
        Some(placement.swapped),
    );
    if swap.clicked() {
        picked = Some(Picked::Act(ScreensAction::Swap));
    }
    let one = row(
        ui,
        icons::MONITOR,
        placement.only_label(),
        facts.keys.one.as_deref(),
        Some(placement.one_screen),
    );
    if one.clicked() {
        picked = Some(Picked::Act(ScreensAction::OneScreen));
    }
    if placement.touch_hidden() {
        let indent = ui.spacing().button_padding.x * 2.0 + ui.spacing().interact_size.y * 0.8;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.label(RichText::new(TOUCH_NOTE).weak().small());
        });
    }
    if let Some(on) = facts.separate_tabs
        && row(ui, icons::BROWSERS, "Separate tabs", None, Some(on)).clicked()
    {
        picked = Some(Picked::Act(ScreensAction::SeparateTabs));
    }
    ui.separator();
    if row(
        ui,
        icons::SLIDERS_HORIZONTAL,
        "More screen settings",
        Some(icons::CARET_RIGHT),
        None,
    )
    .clicked()
    {
        picked = Some(Picked::More);
    }
    picked
}

fn row(ui: &mut Ui, icon: &str, label: &str, end: Option<&str>, switch: Option<bool>) -> Response {
    menu_row_with(ui, format!("{icon}  {label}"), end, switch)
}

pub fn tiles(ui: &mut Ui, chosen: Option<Arrangement>) -> Option<Arrangement> {
    let count = Arrangement::iter().count() as f32;
    let width = ((ui.available_width() - TILE_GAP * (count - 1.0)) / count).max(64.0);
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TILE_GAP;
        for arrangement in Arrangement::iter() {
            if tile(ui, arrangement, chosen == Some(arrangement), width).clicked() {
                picked = Some(arrangement);
            }
        }
    });
    picked
}

fn tile(ui: &mut Ui, arrangement: Arrangement, on: bool, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, TILE_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, arrangement.label())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let fill = if on {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let ink = if on {
        ink_on(fill)
    } else {
        visuals.widgets.inactive.text_color()
    };
    let painter = ui.painter();
    painter.rect_filled(rect, visuals.widgets.inactive.corner_radius, fill);
    let picture =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(40.0, 26.0));
    for part in diagram(arrangement, picture) {
        painter.rect_filled(part, 1.5, ink.gamma_multiply(0.85));
    }
    painter.text(
        pos2(rect.center().x, rect.bottom() - 13.0),
        egui::Align2::CENTER_CENTER,
        arrangement.label(),
        egui::FontId::proportional(12.5),
        ink,
    );
    response
}

fn diagram(arrangement: Arrangement, at: Rect) -> Vec<Rect> {
    let part = |x: f32, y: f32, w: f32, h: f32| {
        Rect::from_min_size(
            at.min + vec2(x, y) * at.width() / 40.0,
            vec2(w, h) * at.width() / 40.0,
        )
    };
    match arrangement {
        Arrangement::Stacked => vec![part(12.0, 0.0, 16.0, 12.0), part(12.0, 13.5, 16.0, 12.0)],
        Arrangement::SideBySide => vec![part(2.0, 6.0, 17.5, 13.0), part(20.5, 6.0, 17.5, 13.0)],
        Arrangement::BigAndSmall => vec![part(2.0, 3.0, 26.0, 19.5), part(29.5, 14.5, 8.5, 6.5)],
    }
}

pub fn menu_width() -> f32 {
    MENU_WIDTH
}

pub fn chip_label(placement: Placement, tabs: bool) -> String {
    let (icon, label) = placement.chip();
    if tabs {
        format!("{}  Tabs  {}", icons::BROWSERS, icons::CARET_DOWN)
    } else {
        format!("{icon}  {label}  {}", icons::CARET_DOWN)
    }
}
