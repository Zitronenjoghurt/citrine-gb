use crate::app::layout_sheet::{CardMenuPick, SheetFacts, SheetPick};
use crate::app::ui_state::ds_screens::{Arrangement, Placement};
use crate::app::ui_state::layout_picks::{Place, Preset};
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::screen_layouts::{Item, LayoutConsole, Spots};
use crate::app::ui_state::ui_theme::{brand, ink_on};
use crate::app::widgets::menu_row::menu_row_with;
use crate::app::widgets::settings::form::RowTag;
use crate::app::widgets::settings::scope::{self, Tag};
use crate::icons;
use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, FontId, Id, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

pub const TITLE: &str = "Layout";
pub const NEW_LAYOUT: &str = "New layout";
pub const RENAME: &str = "Rename";
pub const DELETE: &str = "Delete";
pub const RESET: &str = "Reset";
const POPOVER_WIDTH: f32 = 240.0;
const POPOVER_MARGIN: i8 = 12;
const POPOVER_FONT: f32 = 13.0;
const CARD_GAP: f32 = 8.0;
pub const LABEL_GAP: f32 = 8.0;
const LABEL_SPILL: f32 = 16.0;
const CARD_RADIUS: f32 = 8.0;
pub const GRID_COLUMNS: usize = 3;
const NAME_ROOM: usize = 22;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keys {
    pub swap: Option<String>,
    pub one: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Preset(Preset),
    Standard,
    Saved(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub console: LayoutConsole,
    pub place: Place,
    pub placement: Placement,
    pub spots: Spots,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub kind: CardKind,
    pub name: String,
    pub chosen: bool,
    pub picture: Option<Picture>,
    pub key: Option<String>,
    pub edit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardSize {
    pub width: f32,
    pub picture: f32,
    pub font: f32,
    pub gap: f32,
}

impl CardSize {
    pub fn row(place: Place) -> Self {
        match place {
            Place::Portrait => Self {
                width: 56.0,
                picture: 96.0,
                font: 13.0,
                gap: 12.0,
            },
            _ => Self {
                width: 62.0,
                picture: 38.0,
                font: 13.0,
                gap: 8.0,
            },
        }
    }

    pub fn grid(room: f32, font: f32, columns: usize) -> Self {
        let columns = columns as f32;
        let width = ((room - CARD_GAP * (columns - 1.0)) / columns).floor();
        Self {
            width,
            picture: (width * 0.6).round(),
            font,
            gap: CARD_GAP,
        }
    }

    fn label_column(self, ui: &Ui, cards: &[Card]) -> f32 {
        let font = FontId::proportional(self.font);
        let widest = cards
            .iter()
            .map(|card| {
                ui.painter()
                    .layout_no_wrap(card.name.clone(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    .ceil()
            })
            .fold(0.0, f32::max);
        widest
            .min(self.width + LABEL_SPILL)
            .max(self.width + self.gap - LABEL_GAP)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPress {
    Pick(CardKind),
    Menu(u32),
    MenuPick(u32, CardMenuPick),
    MenuClosed,
}

pub fn card(ui: &mut Ui, card: &Card, size: CardSize, column: f32) -> Response {
    let label_height = size.font + 8.0;
    let column = column.max(size.width);
    let (rect, response) =
        ui.allocate_exact_size(vec2(column, size.picture + label_height), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, card.chosen, &card.name)
    });
    let picture = Rect::from_min_size(
        pos2(rect.center().x - size.width / 2.0, rect.top()),
        vec2(size.width, size.picture),
    );
    crate::app::big_screen::ring::mark(
        &response,
        crate::app::big_screen::ring::Mark::Ring {
            rect: picture,
            radius: CARD_RADIUS,
        },
    );
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let accent = brand(visuals);
    let painter = ui.painter();
    let ink = visuals.text_color();
    let fill = if card.chosen {
        visuals.extreme_bg_color.lerp_to_gamma(accent, 0.18)
    } else if response.hovered() {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    painter.rect_filled(picture, CARD_RADIUS, fill);
    if let Some(drawn) = &card.picture {
        paint_picture(painter, drawn, picture.shrink(4.0), visuals);
    }
    if card.chosen {
        painter.rect_stroke(
            picture,
            CARD_RADIUS,
            Stroke::new(2.0_f32, accent),
            StrokeKind::Inside,
        );
        let badge = pos2(picture.right() - 10.0, picture.top() + 10.0);
        painter.circle_filled(badge, 7.0, accent);
        painter.text(
            badge,
            Align2::CENTER_CENTER,
            icons::CHECK,
            FontId::proportional(10.0),
            ink_on(accent),
        );
    }
    let text = if card.chosen {
        visuals.strong_text_color()
    } else {
        ink
    };
    let mut job = LayoutJob::single_section(
        card.name.clone(),
        TextFormat::simple(FontId::proportional(size.font), text),
    );
    job.wrap = TextWrapping::truncate_at_width(column);
    let galley = painter.layout_job(job);
    let at = pos2(
        picture.center().x - galley.size().x / 2.0,
        picture.bottom() + 4.0,
    );
    painter.galley(at, galley, text);
    match &card.key {
        Some(key) => response.on_hover_text(key_hint(&card.name, key)),
        None => response,
    }
}

pub fn key_hint(name: &str, key: &str) -> String {
    format!("{name} ({key})")
}

fn paint_picture(painter: &egui::Painter, picture: &Picture, room: Rect, visuals: &egui::Visuals) {
    let grey = visuals.weak_text_color().gamma_multiply(0.8);
    let accent = brand(visuals);
    let controls = grey.gamma_multiply(0.7);
    let screens = match picture.place {
        Place::Portrait => {
            let height = room.height();
            let y = room.top() + height * 0.8;
            let radius = (room.width() * 0.09).min(height * 0.06);
            painter.circle_filled(pos2(room.left() + room.width() * 0.18, y), radius, controls);
            painter.circle_filled(
                pos2(room.right() - room.width() * 0.18, y),
                radius,
                controls,
            );
            let bar = Rect::from_center_size(
                pos2(room.center().x, room.top() + height * 0.92),
                vec2(room.width() * 0.2, (height * 0.03).max(2.0)),
            );
            painter.rect_filled(bar, 1.0, controls);
            room.with_max_y(room.top() + height * 0.64)
        }
        Place::Landscape => {
            let side = room.width() * 0.14;
            let radius = (side * 0.4).min(room.height() * 0.12);
            painter.circle_filled(
                pos2(room.left() + side * 0.5, room.center().y),
                radius,
                controls,
            );
            painter.circle_filled(
                pos2(room.right() - side * 0.5, room.center().y),
                radius,
                controls,
            );
            room.shrink2(vec2(side, 0.0))
        }
        Place::Window | Place::FullScreen | Place::BigScreen => room,
    };
    for (touch, part) in parts(picture, screens, room) {
        let fill = if touch { accent } else { grey };
        painter.rect_filled(part, 2.0, fill);
    }
}

pub fn parts(picture: &Picture, screens: Rect, room: Rect) -> Vec<(bool, Rect)> {
    let base = match picture.console {
        LayoutConsole::Ds => ds_parts(picture.placement),
        LayoutConsole::GameBoy => vec![(false, Rect::from_min_size(Pos2::ZERO, vec2(10.0, 9.0)))],
        LayoutConsole::Gba => vec![(false, Rect::from_min_size(Pos2::ZERO, vec2(3.0, 2.0)))],
    };
    fit(&base, screens.shrink(1.0))
        .into_iter()
        .map(|(touch, rect)| {
            let item = if touch {
                Item::TouchScreen
            } else {
                Item::Screen
            };
            match picture.spots.get(&item) {
                Some(spot) => (touch, spot.rect(room, rect.size())),
                None => (touch, rect),
            }
        })
        .collect()
}

fn ds_parts(placement: Placement) -> Vec<(bool, Rect)> {
    let part = |x: f32, y: f32, w: f32| Rect::from_min_size(pos2(x, y), vec2(w, w * 0.75));
    let first = placement.swapped;
    if placement.one_screen {
        return vec![(first, part(0.0, 0.0, 4.0))];
    }
    match placement.arrangement {
        Arrangement::Stacked => vec![(first, part(0.0, 0.0, 4.0)), (!first, part(0.0, 3.3, 4.0))],
        Arrangement::SideBySide => {
            vec![(first, part(0.0, 0.0, 4.0)), (!first, part(4.3, 0.0, 4.0))]
        }
        Arrangement::BigAndSmall => {
            vec![
                (first, part(0.0, 0.0, 4.0)),
                (!first, part(4.2, 2.0, 1.333)),
            ]
        }
    }
}

fn fit(parts: &[(bool, Rect)], area: Rect) -> Vec<(bool, Rect)> {
    let Some(bounds) = parts.iter().map(|(_, r)| *r).reduce(|a, b| a.union(b)) else {
        return Vec::new();
    };
    let scale = (area.width() / bounds.width()).min(area.height() / bounds.height());
    let size = bounds.size() * scale;
    let origin = area.center() - size / 2.0;
    parts
        .iter()
        .map(|(touch, rect)| {
            let min = origin + (rect.min - bounds.min) * scale;
            (*touch, Rect::from_min_size(min, rect.size() * scale))
        })
        .collect()
}

fn press(
    ui: &mut Ui,
    shown: &Card,
    size: CardSize,
    column: f32,
    menu: Option<u32>,
    focus: bool,
) -> Option<CardPress> {
    let response = card(ui, shown, size, column);
    if focus && shown.chosen {
        response.request_focus();
    }
    let own = match shown.kind {
        CardKind::Saved(id) => Some(id),
        CardKind::Preset(_) | CardKind::Standard => shown.edit,
    };
    let Some(id) = own else {
        return response.clicked().then_some(CardPress::Pick(shown.kind));
    };
    if menu == Some(id) {
        let saved = matches!(shown.kind, CardKind::Saved(_));
        return card_menu(ui, &response, id, saved);
    }
    if response.secondary_clicked() || response.long_touched() {
        return Some(CardPress::Menu(id));
    }
    response.clicked().then_some(CardPress::Pick(shown.kind))
}

fn card_menu(ui: &Ui, card: &Response, id: u32, saved: bool) -> Option<CardPress> {
    let mut open = true;
    let picked = egui::Popup::new(
        Id::new("layout_card_menu"),
        ui.ctx().clone(),
        card.rect,
        ui.layer_id(),
    )
    .kind(egui::PopupKind::Menu)
    .open_bool(&mut open)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .show(|ui| {
        ui.set_min_width(140.0);
        let mut picked = None;
        if !saved {
            let reset = format!("{}  {RESET}", icons::ARROW_COUNTER_CLOCKWISE);
            return menu_row_with(ui, reset, None, None)
                .clicked()
                .then_some(CardMenuPick::Reset);
        }
        if menu_row_with(
            ui,
            format!("{}  {RENAME}", icons::PENCIL_SIMPLE),
            None,
            None,
        )
        .clicked()
        {
            picked = Some(CardMenuPick::Rename);
        }
        if menu_row_with(ui, format!("{}  {DELETE}", icons::TRASH), None, None).clicked() {
            picked = Some(CardMenuPick::Delete);
        }
        picked
    })
    .and_then(|shown| shown.inner);
    match picked {
        Some(pick) => Some(CardPress::MenuPick(id, pick)),
        None if !open => Some(CardPress::MenuClosed),
        None => None,
    }
}

pub fn card_row(
    ui: &mut Ui,
    cards: &[Card],
    size: CardSize,
    menu: Option<u32>,
) -> Option<CardPress> {
    let room = ui.available_width();
    let column = size.label_column(ui, cards);
    let slots = (((room + LABEL_GAP) / (column + LABEL_GAP)).floor() as usize).max(1);
    let per_line = slots.min(cards.len().max(1));
    let gap = row_gap(room, column, slots);
    let used = column * per_line as f32 + gap * per_line.saturating_sub(1) as f32;
    let lead = ((room - used) / 2.0).max(0.0);
    let mut pressed = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = CARD_GAP;
        for line in cards.chunks(per_line) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_space(lead);
                for shown in line {
                    pressed = press(ui, shown, size, column, menu, false).or(pressed);
                }
            });
        }
    });
    pressed
}

fn row_gap(room: f32, column: f32, slots: usize) -> f32 {
    let gaps = slots.saturating_sub(1).max(1) as f32;
    ((room - column * slots as f32) / gaps).max(LABEL_GAP)
}

pub fn card_grid(
    ui: &mut Ui,
    cards: &[Card],
    size: CardSize,
    menu: Option<u32>,
    focus: bool,
) -> Option<CardPress> {
    let mut pressed = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = CARD_GAP;
        for row in cards.chunks(grid_columns(cards.len())) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = CARD_GAP;
                for shown in row {
                    pressed = press(ui, shown, size, size.width, menu, focus).or(pressed);
                }
            });
        }
    });
    pressed
}

pub fn card_pick(press: CardPress) -> SheetPick {
    match press {
        CardPress::Pick(kind) => SheetPick::Card(kind),
        CardPress::Menu(id) => SheetPick::Menu(id),
        CardPress::MenuPick(id, pick) => SheetPick::CardMenu(id, pick),
        CardPress::MenuClosed => SheetPick::CloseMenu,
    }
}

pub fn grid_columns(cards: usize) -> usize {
    [GRID_COLUMNS, GRID_COLUMNS + 1]
        .into_iter()
        .find(|columns| cards <= *columns || cards % columns != 1)
        .unwrap_or(GRID_COLUMNS)
}

pub fn popover_width(cards: usize) -> f32 {
    let columns = grid_columns(cards);
    let card = CardSize::grid(POPOVER_WIDTH, POPOVER_FONT, GRID_COLUMNS).width;
    POPOVER_WIDTH.max(card * columns as f32 + CARD_GAP * (columns - 1) as f32)
}

pub fn short_game(title: &str) -> String {
    if title.chars().count() <= NAME_ROOM {
        return title.to_owned();
    }
    let cut: String = title.chars().take(NAME_ROOM - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

pub fn popover_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::menu(style).inner_margin(POPOVER_MARGIN)
}

fn button_text_width(ui: &Ui, text: &str) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
}

pub fn popover(ui: &mut Ui, facts: &SheetFacts, menu: Option<u32>) -> Option<SheetPick> {
    let mut picked = None;
    ui.set_width(popover_width(facts.cards.len()));
    ui.horizontal(|ui| {
        if !facts.here {
            let back = ui.add(egui::Button::new(icons::ARROW_LEFT).frame(false));
            back.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back"));
            if back.on_hover_text("Back").clicked() {
                picked = Some(SheetPick::Back);
            }
        }
        ui.label(RichText::new(TITLE).strong());
        if let (Some(tag), Some(game)) = (&facts.tag, &facts.game) {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let tag = match tag {
                    Tag::Pick { game, value } => Tag::Pick {
                        game: short_game(game),
                        value: value.clone(),
                    },
                    tag => tag.clone(),
                };
                let tag = RowTag {
                    hover: tag.hover(game),
                    tag,
                };
                if tag.show(ui, false) {
                    picked = Some(SheetPick::DropGame);
                }
            });
        }
    });
    let first = [scope::ALL_GAMES, EVERY_MODE]
        .map(|text| button_text_width(ui, text))
        .into_iter()
        .fold(0.0, f32::max)
        + 2.0 * ui.spacing().button_padding.x;
    if let Some(game) = &facts.game {
        let label = format!("{} {}", scope::MARK, short_game(game));
        let mut games = facts.level.games;
        scope::buttons_from(
            ui,
            &mut games,
            (Scope::AllGames, scope::ALL_GAMES),
            (Scope::ThisGame, &label),
            game,
            first,
        );
        if games != facts.level.games {
            picked = Some(SheetPick::Games(games));
        }
    }
    let mut every = facts.level.every_mode;
    scope::buttons_from(
        ui,
        &mut every,
        (true, EVERY_MODE),
        (false, facts.mode),
        facts.mode,
        first,
    );
    if every != facts.level.every_mode {
        picked = Some(SheetPick::Modes(every));
    }
    ui.add_space(4.0);
    if facts.tabs != Some(true) {
        let columns = grid_columns(facts.cards.len());
        let size = CardSize::grid(ui.available_width(), POPOVER_FONT, columns);
        if let Some(press) = card_grid(ui, &facts.cards, size, menu, false) {
            picked = Some(card_pick(press));
        }
        ui.separator();
        if facts.here && row(ui, icons::PLUS, NEW_LAYOUT, None, None).clicked() {
            picked = Some(SheetPick::New);
        }
        if facts.here && row(ui, icons::PENCIL_SIMPLE, EDIT, None, None).clicked() {
            picked = Some(SheetPick::Edit);
        }
        if let Some(swapped) = facts.swapped {
            let swap = row(
                ui,
                SWAP_ICON,
                SWAP,
                facts.keys.swap.as_deref(),
                Some(swapped),
            );
            if swap.clicked() {
                picked = Some(SheetPick::Swap);
            }
        }
        if !facts.others.is_empty() {
            ui.separator();
        }
        for (place, name) in &facts.others {
            let end = format!("{name}  {}", icons::CARET_RIGHT);
            if row(ui, place.icon(), place.row_label(), Some(&end), None).clicked() {
                picked = Some(SheetPick::Place(*place));
            }
        }
        ui.separator();
    }
    if let Some(on) = facts.tabs
        && row(ui, icons::BROWSERS, TABS, None, Some(on)).clicked()
    {
        picked = Some(SheetPick::SeparateTabs);
    }
    if let Some(closed) = facts.lid
        && facts.tabs != Some(true)
        && row(ui, icons::LAPTOP, LID, None, Some(closed)).clicked()
    {
        picked = Some(SheetPick::Lid);
    }
    if row(
        ui,
        icons::SLIDERS_HORIZONTAL,
        MORE,
        Some(icons::CARET_RIGHT),
        None,
    )
    .clicked()
    {
        picked = Some(SheetPick::More);
    }
    picked
}

pub const EVERY_MODE: &str = "Every mode";
pub const EDIT: &str = "Edit layout";
pub const SWAP: &str = "Swap screens";
pub const SWAP_ICON: &str = icons::ARROWS_DOWN_UP;
pub const LID: &str = "Close the lid";
pub const TABS: &str = "Separate tabs";
pub const MORE: &str = "More screen settings";

fn row(ui: &mut Ui, icon: &str, label: &str, end: Option<&str>, switch: Option<bool>) -> Response {
    menu_row_with(ui, format!("{icon}  {label}"), end, switch)
}

pub fn chip_label(name: &str, icon: &str, tabs: bool) -> String {
    if tabs {
        format!("{}  Tabs  {}", icons::BROWSERS, icons::CARET_DOWN)
    } else {
        format!("{icon}  {name}  {}", icons::CARET_DOWN)
    }
}

pub fn chip_icon(console: LayoutConsole, placement: Placement) -> &'static str {
    match console {
        LayoutConsole::Ds => placement.chip().0,
        LayoutConsole::GameBoy | LayoutConsole::Gba => icons::LAYOUT,
    }
}
