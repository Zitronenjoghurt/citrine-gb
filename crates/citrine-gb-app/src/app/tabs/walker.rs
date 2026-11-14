use super::TabViewer;
use crate::app::big_screen::scroll::Following;
use crate::app::mobile::walker as phone_look;
use crate::app::widgets::text::one_line;
use crate::icons;
use crate::walker::Hold;
use citrine_gb_pkm::Gen;
use citrine_gb_pkm::pkm::text;
use citrine_gb_walker::{
    App, Button, FLAG_INIT, Image, LCD_HEIGHT, LCD_WIDTH, Lcd, Screen, Shelf, Walker,
};
use egui::emath::GuiRounding;
use egui::{
    Color32, ColorImage, CornerRadius, Margin, Rect, Sense, TextureHandle, TextureOptions, Ui,
    Vec2, pos2, vec2,
};
use std::collections::HashMap;

const SHADES: [Color32; 4] = [
    Color32::from_rgb(0xD6, 0xDB, 0xC8),
    Color32::from_rgb(0x9C, 0xA3, 0x8E),
    Color32::from_rgb(0x5E, 0x65, 0x55),
    Color32::from_rgb(0x24, 0x29, 0x22),
];
const GAP: f32 = 8.0;
const TILE_MIN: f32 = 120.0;
const ROW: f32 = 28.0;
const HEAD: f32 = 14.0;
const BEZEL_PAD: f32 = 10.0;
const BEZEL_RADIUS: u8 = 14;
const TILE_RADIUS: u8 = 12;
const DESK_MAX_SCALE: f32 = 1.75;
const PHONE_MAX_SCALE: f32 = 3.5;
const SLOTS: usize = 3;
const WEEK: usize = 7;
const QUARTERS_PER_SECOND: f64 = 4.0;
const OPEN_PRESSES: usize = 16;

#[derive(Default)]
pub struct WalkerPane {
    lcd: Option<(Lcd, TextureHandle)>,
    names: HashMap<Vec<u8>, TextureHandle>,
    shown_pass: Option<u64>,
}

struct Entry {
    name: Option<Image>,
    level: Option<u8>,
}

struct View {
    lcd: Lcd,
    paired: bool,
    idle: bool,
    pokemon: Option<(String, u8)>,
    route: String,
    walk_steps: u32,
    watts: u16,
    week: Vec<u32>,
    caught: Vec<Entry>,
    items: Vec<Entry>,
    costs: [u16; 2],
}

impl View {
    fn read(walker: &mut Walker, phase: u32) -> Self {
        let pokemon = walker.walking_pokemon().map(|summary| {
            let name = text::decode(Gen::Four, &walker.walking_nickname());
            (name, summary.level)
        });
        let shelves = walker.shelves();
        let entries = |pick: fn(&Shelf) -> bool| -> Vec<Entry> {
            shelves
                .iter()
                .filter(|shelf| pick(shelf))
                .map(|shelf| Entry {
                    name: walker.shelf_name(*shelf).map(trim),
                    level: walker.shelf_level(*shelf),
                })
                .collect()
        };
        let caught = entries(|shelf| matches!(shelf, Shelf::Caught(_) | Shelf::EventPokemon));
        let items = entries(|shelf| matches!(shelf, Shelf::Item(_) | Shelf::EventItem));
        let today = walker.today_steps();
        let mut week: Vec<u32> = walker.history().into_iter().take(WEEK - 1).collect();
        week.resize(WEEK - 1, 0);
        week.reverse();
        week.push(today);
        let costs = walker.rules().app_costs;
        Self {
            lcd: walker.lcd(phase),
            paired: walker.info().flags() & FLAG_INIT != 0,
            idle: matches!(walker.screen(), Screen::Home | Screen::Menu { .. }),
            pokemon,
            route: text::decode(Gen::Four, &walker.route_name()),
            walk_steps: walker.walk_steps(),
            watts: walker.health().watts(),
            week,
            caught,
            items,
            costs: [costs[App::Radar.index()], costs[App::Dowsing.index()]],
        }
    }
}

enum Action {
    Press(Button),
    Open(App),
    UseHere,
}

pub fn show(viewer: &mut TabViewer<'_>, ui: &mut Ui) {
    let phone = viewer.ui.settings.layout.is_touch(ui.ctx());
    let phase = (ui.input(|i| i.time) * QUARTERS_PER_SECOND) as u32;
    let pass = ui.ctx().cumulative_pass_nr();
    let pane = &mut viewer.ui.walker;
    if pane.shown_pass.is_none_or(|shown| shown + 1 < pass) {
        viewer.emulator.take_walker();
    }
    pane.shown_pass = Some(pass);
    let held = match viewer.emulator.walker_hold() {
        Hold::Elsewhere(device) => Some(device.clone()),
        _ => None,
    };
    let Some(walker) = viewer.emulator.shown_walker() else {
        return;
    };
    let view = View::read(walker, phase);
    let pane = &mut viewer.ui.walker;
    let mut action = None;
    let margin = if phone {
        Margin::ZERO
    } else {
        Margin {
            left: 12,
            right: 12,
            top: 12,
            bottom: 16,
        }
    };
    egui::ScrollArea::vertical()
        .id_salt("walker_pane")
        .auto_shrink([false, false])
        .show_following(ui, |ui| {
            egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                let used = held
                    .as_deref()
                    .is_some_and(|device| held_card(ui, device, phone));
                let mine = ui
                    .add_enabled_ui(held.is_none(), |ui| body(ui, pane, &view, phone))
                    .inner;
                action = mine.or(used.then_some(Action::UseHere));
            });
        });
    match action {
        Some(Action::Press(button)) => viewer.emulator.press_walker(button),
        Some(Action::Open(app)) => open(viewer.emulator, app),
        Some(Action::UseHere) => viewer.emulator.take_walker(),
        None => {}
    }
}

fn open(emulator: &mut crate::emulator::Emulator, app: App) {
    for _ in 0..OPEN_PRESSES {
        let Some(walker) = emulator.walker_mut() else {
            return;
        };
        if walker.asleep() {
            emulator.press_walker(Button::Middle);
            continue;
        }
        let press = match (walker.screen(), app) {
            (Screen::Collected { .. }, _) => Button::Middle,
            (Screen::Home, App::Connect) => Button::Middle,
            (Screen::Home, _) => Button::Right,
            (Screen::Menu { app: shown, .. }, wanted) if shown == wanted => {
                emulator.press_walker(Button::Middle);
                return;
            }
            (Screen::Menu { .. }, _) => Button::Right,
            _ => return,
        };
        emulator.press_walker(press);
    }
}

fn body(ui: &mut Ui, pane: &mut WalkerPane, view: &View, phone: bool) -> Option<Action> {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut action = None;
    let full = ui.available_width();
    let right = (full * 0.36).max(TILE_MIN);
    let left = full - GAP - right;
    if !phone && left >= LCD_WIDTH as f32 + 20.0 {
        let height = device_height(ui, left, DESK_MAX_SCALE, false);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let down = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(vec2(left, height), down, |ui| {
                if let Some(button) = device(ui, pane, view, left, DESK_MAX_SCALE, false) {
                    action = Some(Action::Press(button));
                }
            });
            ui.allocate_ui_with_layout(vec2(right, height), down, |ui| {
                ui.spacing_mut().item_spacing.y = GAP;
                let tile = (height - GAP * 2.0) / 3.0;
                for (app, label, detail, ready) in tiles(view) {
                    if tile_at(ui, label, &detail, vec2(right, tile), ready, false) {
                        action = Some(Action::Open(app));
                    }
                }
            });
        });
    } else {
        if let Some(button) = device(ui, pane, view, full, PHONE_MAX_SCALE, true) {
            action = Some(Action::Press(button));
        }
        ui.add_space(GAP);
        let width = (full - GAP * 2.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (app, label, detail, ready) in tiles(view) {
                if tile_at(ui, label, &detail, vec2(width, 46.0), ready, phone) {
                    action = Some(Action::Open(app));
                }
            }
        });
    }
    ui.add_space(GAP);
    walk_card(ui, view, phone);
    ui.add_space(GAP);
    card(ui, phone, |ui| {
        ui.spacing_mut().item_spacing.x = 28.0;
        ui.columns(2, |cols| {
            shelf_list(&mut cols[0], pane, "Caught", &view.caught);
            shelf_list(&mut cols[1], pane, "Items", &view.items);
        });
    });
    ui.add_space(GAP);
    card(ui, phone, |ui| week_chart(ui, &view.week));
    action
}

fn tiles(view: &View) -> [(App, &'static str, String, bool); 3] {
    [
        (App::Radar, "Poké Radar", view.costs[0]),
        (App::Dowsing, "Dowsing", view.costs[1]),
        (App::Connect, "Connect", 0),
    ]
    .map(|(app, label, cost)| match blocked(view, app, cost) {
        Some(reason) => (app, label, reason, false),
        None if app == App::Connect => (app, label, "Game or friend".to_owned(), true),
        None => (app, label, format!("Costs {}", watts(cost)), true),
    })
}

fn blocked(view: &View, app: App, cost: u16) -> Option<String> {
    if !view.idle {
        return Some("Walker busy".to_owned());
    }
    if app == App::Connect {
        return None;
    }
    if !view.paired {
        return Some("Connect first".to_owned());
    }
    if view.pokemon.is_none() {
        return Some("Needs a Pokémon".to_owned());
    }
    if view.watts < cost {
        return Some(format!("Needs {}", watts(cost)));
    }
    None
}

pub fn watts(count: u16) -> String {
    match count {
        1 => "1 watt".to_owned(),
        count => format!("{} watts", format_steps(count.into())),
    }
}

fn button_size(touch: bool) -> f32 {
    if touch { 44.0 } else { 30.0 }
}

fn lcd_scale(ui: &Ui, width: f32, max_scale: f32) -> f32 {
    let ppp = ui.ctx().pixels_per_point();
    let room = ((width - BEZEL_PAD * 2.0) / LCD_WIDTH as f32).clamp(1.0, max_scale);
    let pixels = (room * ppp).floor().max(1.0);
    pixels / ppp
}

fn device_height(ui: &Ui, width: f32, max_scale: f32, touch: bool) -> f32 {
    let button = button_size(touch);
    LCD_HEIGHT as f32 * lcd_scale(ui, width, max_scale) + BEZEL_PAD * 3.0 + button * 1.45
}

fn device(
    ui: &mut Ui,
    pane: &mut WalkerPane,
    view: &View,
    width: f32,
    max_scale: f32,
    touch: bool,
) -> Option<Button> {
    let scale = lcd_scale(ui, width, max_scale);
    let lcd = vec2(LCD_WIDTH as f32 * scale, LCD_HEIGHT as f32 * scale);
    let button = button_size(touch);
    let size = vec2(width, device_height(ui, width, max_scale, touch));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let dark = ui.visuals().dark_mode;
    let bezel = if dark {
        Color32::from_rgb(0x3A, 0x3F, 0x46)
    } else {
        Color32::from_rgb(0xE4, 0xE7, 0xEB)
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(BEZEL_RADIUS), bezel);
    let ppp = ui.pixels_per_point();
    let left = (rect.center().x - lcd.x / 2.0).round_to_pixel_center(ppp);
    let top = (rect.top() + BEZEL_PAD).round_to_pixel_center(ppp);
    let screen = Rect::from_min_size(pos2(left, top), lcd).round_to_pixels(ppp);
    let texture = lcd_texture(ui, pane, &view.lcd);
    ui.painter().image(
        texture.id(),
        screen,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let (fill, ink) = if dark {
        (
            Color32::from_rgb(0x6B, 0x72, 0x7C),
            Color32::from_rgb(0xF2, 0xF3, 0xF5),
        )
    } else {
        (
            Color32::from_rgb(0xA9, 0xB0, 0xB9),
            Color32::from_rgb(0x1E, 0x22, 0x27),
        )
    };
    let y = screen.bottom() + BEZEL_PAD + button / 2.0;
    let spread = (lcd.x * 0.3).min(110.0).max(button * 1.4);
    let mut pressed = None;
    for (dx, dy, glyph, which) in [
        (-spread, 0.0, icons::CARET_LEFT, Button::Left),
        (0.0, button * 0.45, icons::CIRCLE, Button::Middle),
        (spread, 0.0, icons::CARET_RIGHT, Button::Right),
    ] {
        let at = pos2(rect.center().x + dx, y + dy);
        let hit = Rect::from_center_size(at, Vec2::splat(button));
        let response = ui.interact(hit, ui.id().with(("walker_button", glyph)), Sense::click());
        let shade = if response.is_pointer_button_down_on() {
            fill.lerp_to_gamma(ink, 0.25)
        } else {
            fill
        };
        ui.painter().circle_filled(at, button / 2.0, shade);
        ui.painter().text(
            at,
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(button * 0.42),
            ink,
        );
        if response.clicked() {
            pressed = Some(which);
        }
    }
    pressed
}

fn lcd_texture(ui: &Ui, pane: &mut WalkerPane, lcd: &Lcd) -> TextureHandle {
    let image = || {
        let pixels = lcd
            .shades()
            .iter()
            .map(|&s| SHADES[usize::from(s)])
            .collect();
        ColorImage::new([LCD_WIDTH, LCD_HEIGHT], pixels)
    };
    match &mut pane.lcd {
        Some((shown, texture)) => {
            if shown != lcd {
                texture.set(image(), TextureOptions::NEAREST);
                *shown = lcd.clone();
            }
            texture.clone()
        }
        None => {
            let texture = ui
                .ctx()
                .load_texture("walker_lcd", image(), TextureOptions::NEAREST);
            pane.lcd = Some((lcd.clone(), texture.clone()));
            texture
        }
    }
}

fn name_texture(ui: &Ui, pane: &mut WalkerPane, name: &Image) -> TextureHandle {
    pane.names
        .entry(name.shades.clone())
        .or_insert_with(|| {
            let pixels = name
                .shades
                .iter()
                .map(|&s| Color32::from_white_alpha(s * 85))
                .collect();
            let image = ColorImage::new([name.width, name.height], pixels);
            ui.ctx()
                .load_texture("walker_name", image, TextureOptions::NEAREST)
        })
        .clone()
}

fn trim(image: Image) -> Image {
    let used = (0..image.width)
        .rev()
        .find(|&x| (0..image.height).any(|y| image.shade(x, y) != 0))
        .map_or(1, |x| x + 1);
    let mut shades = Vec::with_capacity(used * image.height);
    for y in 0..image.height {
        for x in 0..used {
            shades.push(image.shade(x, y));
        }
    }
    Image {
        width: used,
        height: image.height,
        shades,
    }
}

fn tile_at(ui: &mut Ui, label: &str, detail: &str, size: Vec2, enabled: bool, phone: bool) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let visuals = ui.visuals();
    let (fill, radius, stroke) = if phone {
        let fill = phone_look::tile_fill(visuals, response.hovered() && enabled);
        (fill, CornerRadius::same(TILE_RADIUS), egui::Stroke::NONE)
    } else {
        let look = ui.style().interact(&response);
        (look.weak_bg_fill, look.corner_radius, look.bg_stroke)
    };
    let fade = if enabled { 1.0 } else { 0.45 };
    let text = visuals.strong_text_color().gamma_multiply(fade);
    let muted = visuals.text_color().gamma_multiply(fade);
    let icon = match label {
        "Poké Radar" => icons::CROSSHAIR,
        "Dowsing" => icons::TREE,
        _ => icons::BROADCAST,
    };
    let name = one_line(ui, label, 14.0, text, size.x - 40.0);
    let painter = ui.painter_at(rect);
    painter.rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);
    let line = 20.0 + 4.0 + name.size().x;
    let left = rect.center().x - line / 2.0;
    let top = rect.center().y - 8.0;
    painter.text(
        pos2(left + 10.0, top),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(17.0),
        text,
    );
    painter.galley(pos2(left + 24.0, top - name.size().y / 2.0), name, text);
    painter.text(
        pos2(rect.center().x, top + 18.0),
        egui::Align2::CENTER_CENTER,
        detail,
        egui::FontId::proportional(12.5),
        muted,
    );
    response.clicked()
}

fn pair(ui: &mut Ui, height: f32, left: (&str, f32, bool), right: (&str, f32, bool)) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let visuals = ui.visuals();
    let pick = |strong: bool| {
        if strong {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        }
    };
    let (lc, rc) = (pick(left.2), pick(right.2));
    let wanted = one_line(ui, right.0, right.1, rc, f32::INFINITY).size().x;
    let room = rect.width() - wanted.min(rect.width() * 0.4) - 10.0;
    let label = one_line(ui, left.0, left.1, lc, room.max(1.0));
    let value_room = (rect.width() - label.size().x - 10.0).max(1.0);
    let value = one_line(ui, right.0, right.1, rc, value_room);
    let cut = value.elided || label.elided;
    let painter = ui.painter();
    let label_y = rect.center().y - label.size().y / 2.0;
    painter.galley(pos2(rect.left(), label_y), label, lc);
    painter.galley(
        pos2(
            rect.right() - value.size().x,
            rect.center().y - value.size().y / 2.0,
        ),
        value,
        rc,
    );
    if cut {
        response.on_hover_text(format!("{} {}", left.0, right.0));
    }
}

fn card<R>(ui: &mut Ui, phone: bool, body: impl FnOnce(&mut Ui) -> R) -> R {
    let frame = if phone {
        phone_look::card(ui)
    } else {
        egui::Frame::group(ui.style())
    };
    frame
        .inner_margin(Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
        .inner
}

fn held_card(ui: &mut Ui, device: &str, phone: bool) -> bool {
    let used = card(ui, phone, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(ROW + 4.0);
            let icon = if citrine_gb_net::is_phone(device) {
                icons::DEVICE_MOBILE
            } else {
                icons::DESKTOP
            };
            ui.label(egui::RichText::new(icon).size(18.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let used = if phone {
                    phone_look::use_here(ui)
                } else {
                    ui.button("Use here").clicked()
                };
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(format!("In use on {device}")).truncate());
                });
                used
            })
            .inner
        })
        .inner
    });
    ui.add_space(GAP);
    used
}

fn walk_card(ui: &mut Ui, view: &View, phone: bool) {
    card(ui, phone, |ui| match &view.pokemon {
        Some((name, level)) => {
            pair(
                ui,
                ROW,
                (name, 16.0, true),
                (&format!("Lv {level}"), 14.0, false),
            );
            pair(ui, ROW, ("Route", 14.0, false), (&view.route, 14.0, true));
            let steps = format_steps(view.walk_steps);
            pair(
                ui,
                ROW,
                ("Steps since sent out", 14.0, false),
                (&steps, 14.0, true),
            );
        }
        None => {
            pair(
                ui,
                ROW,
                ("No Pokémon on a walk", 16.0, true),
                ("", 14.0, false),
            );
            let hint = "Send one from HeartGold or SoulSilver";
            pair(ui, ROW, (hint, 14.0, false), ("", 14.0, false));
        }
    });
}

fn shelf_list(ui: &mut Ui, pane: &mut WalkerPane, title: &str, entries: &[Entry]) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space((ROW - HEAD) / 2.0);
    pair(ui, HEAD, (title, 12.5, false), ("", 12.5, false));
    let empty = pixel_text("Empty");
    let rows = if entries.is_empty() { 1 } else { SLOTS };
    for slot in 0..rows {
        match entries.get(slot) {
            Some(entry) => {
                let level = entry.level.map(|level| pixel_text(&format!("Lv {level}")));
                pixel_row(ui, pane, entry.name.as_ref(), level.as_ref());
            }
            None => pixel_row(ui, pane, Some(&empty), None),
        }
    }
}

fn pixel_row(ui: &mut Ui, pane: &mut WalkerPane, left: Option<&Image>, right: Option<&Image>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    let ppp = ui.pixels_per_point();
    let pixel = ppp.floor().max(1.0) / ppp;
    let ink = ui.visuals().strong_text_color();
    let mut paint = |image: &Image, x: f32, shown: f32| {
        let height = image.height as f32 * pixel;
        let top = (rect.center().y - height / 2.0).round_to_pixels(ppp);
        let area = Rect::from_min_size(pos2(x.round_to_pixels(ppp), top), vec2(shown, height));
        let uv = Rect::from_min_max(
            pos2(0.0, 0.0),
            pos2(shown / (image.width as f32 * pixel), 1.0),
        );
        let texture = name_texture(ui, pane, image);
        ui.painter().image(texture.id(), area, uv, ink);
    };
    let right_width = right.map_or(0.0, |image| {
        let shown = image.width as f32 * pixel;
        paint(image, rect.right() - shown, shown);
        shown + 8.0
    });
    if let Some(image) = left {
        let room = (rect.width() - right_width).max(1.0);
        paint(image, rect.left(), (image.width as f32 * pixel).min(room));
    }
}

const TEXT_TOP: usize = 4;
const TEXT_LEFT: usize = 2;
const GLYPHS: [(char, &str); 17] = [
    ('L', "##... ##... ##... ##... ##... ##... ##### #####"),
    ('E', "##### ##### ##... ####. ####. ##... ##### #####"),
    (
        'v',
        "...... ...... ##..## ##..## ##..## .####. .####. ..##..",
    ),
    (
        'm',
        "........ ........ #######. ######## ##.##.## ##.##.## ##.##.## ##.##.##",
    ),
    (
        'p',
        "...... ...... #####. ##..## ##..## ##..## ##..## #####. ##.... ##....",
    ),
    ('t', "..... .##.. ##### .##.. .##.. .##.. .##.. ..###"),
    (
        'y',
        "...... ...... ##..## ##..## ##..## ##..## ##..## .##### ....## .####.",
    ),
    ('0', ".###. ##.## ##.## ##.## ##.## ##.## ##.## .###."),
    ('1', "..##. .###. ####. ..##. ..##. ..##. ..##. #####"),
    ('2', ".###. ##.## ...## ..##. .##.. ##... ##... #####"),
    ('3', "####. ...## ...## .###. ...## ...## ...## ####."),
    ('4', "...## ..### .#### ##.## ##.## ##### ...## ...##"),
    ('5', "##### ##... ##... ####. ...## ...## ##.## .###."),
    ('6', ".###. ##... ##... ####. ##.## ##.## ##.## .###."),
    ('7', "##### ...## ...## ..##. ..##. .##.. .##.. .##.."),
    ('8', ".###. ##.## ##.## .###. ##.## ##.## ##.## .###."),
    ('9', ".###. ##.## ##.## ##.## .#### ...## ...## .###."),
];

fn pixel_text(text: &str) -> Image {
    let glyphs: Vec<Option<&str>> = text
        .chars()
        .map(|c| GLYPHS.iter().find(|(g, _)| *g == c).map(|(_, rows)| *rows))
        .collect();
    let advance =
        |glyph: &Option<&str>| glyph.map_or(3, |rows| rows.find(' ').unwrap_or(rows.len()));
    let width = glyphs.iter().map(advance).sum::<usize>() + glyphs.len().saturating_sub(1);
    let mut image = Image::blank(TEXT_LEFT + width.max(1), 16);
    let mut x = TEXT_LEFT;
    for glyph in &glyphs {
        for (dy, row) in glyph.unwrap_or_default().split(' ').enumerate() {
            for (dx, cell) in row.bytes().enumerate() {
                if cell == b'#' {
                    image.shades[(TEXT_TOP + dy) * image.width + x + dx] = 3;
                }
            }
        }
        x += advance(glyph) + 1;
    }
    image
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn week_chart(ui: &mut Ui, week: &[u32]) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let total: u32 = week.iter().sum();
    let total = format!("{} steps", format_steps(total));
    ui.add_space((ROW - HEAD) / 2.0);
    pair(ui, HEAD, ("This week", 12.5, false), (&total, 12.5, false));
    ui.add_space(6.0);
    let height = 40.0;
    let size = vec2(ui.available_width(), height + 23.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let max = week.iter().copied().max().unwrap_or(0).max(1) as f32;
    let slot = rect.width() / week.len() as f32;
    let accent = ui.visuals().selection.bg_fill;
    let muted = ui.visuals().text_color();
    let today = crate::stats::today();
    for (i, steps) in week.iter().enumerate() {
        let back = (week.len() - 1 - i) as i64;
        let day = today
            .checked_sub(jiff::Span::new().days(back))
            .unwrap_or(today);
        let name = WEEKDAYS[day.weekday().to_monday_zero_offset() as usize];
        let h = (height * *steps as f32 / max).max(3.0);
        let x = rect.left() + slot * i as f32 + slot * 0.25;
        let bar = Rect::from_min_max(
            pos2(x, rect.top() + height - h),
            pos2(x + slot * 0.5, rect.top() + height),
        );
        let fill = if i == week.len() - 1 {
            accent
        } else {
            muted.gamma_multiply(0.45)
        };
        ui.painter().rect_filled(bar, CornerRadius::same(3), fill);
        ui.painter().text(
            pos2(bar.center().x, rect.top() + height + 9.0),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(11.5),
            muted,
        );
        let column = Rect::from_min_max(
            pos2(rect.left() + slot * i as f32, rect.top()),
            pos2(rect.left() + slot * (i + 1) as f32, rect.bottom()),
        );
        ui.interact(column, ui.id().with(("walker_week", i)), Sense::hover())
            .on_hover_text(format!("{name}: {} steps", format_steps(*steps)));
    }
}

pub fn format_steps(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_group_by_thousands() {
        assert_eq!(format_steps(0), "0");
        assert_eq!(format_steps(999), "999");
        assert_eq!(format_steps(46_422), "46,422");
        assert_eq!(format_steps(9_999_999), "9,999,999");
    }

    #[test]
    fn one_watt_is_singular() {
        assert_eq!(watts(1), "1 watt");
        assert_eq!(watts(10), "10 watts");
        assert_eq!(watts(1207), "1,207 watts");
    }

    #[test]
    fn trim_drops_empty_columns_on_the_right() {
        let mut image = Image::blank(8, 8);
        image.shades[2] = 3;
        let trimmed = trim(image);
        assert_eq!(trimmed.width, 3);
        assert_eq!(trimmed.shade(2, 0), 3);
    }

    #[test]
    fn an_unpaired_walker_only_offers_connect() {
        let mut walker = Walker::new(1);
        let view = View::read(&mut walker, 0);
        let ready = tiles(&view).map(|(_, _, detail, ready)| (detail, ready));
        assert_eq!(ready[0], ("Connect first".to_owned(), false));
        assert_eq!(ready[1], ("Connect first".to_owned(), false));
        assert_eq!(ready[2], ("Game or friend".to_owned(), true));
        assert!(view.pokemon.is_none());
        assert_eq!(view.week.len(), WEEK);
    }

    #[test]
    fn a_blocked_tile_says_why() {
        let mut walker = Walker::new(1);
        let mut view = View::read(&mut walker, 0);
        view.paired = true;
        assert_eq!(
            blocked(&view, App::Radar, 10).as_deref(),
            Some("Needs a Pokémon")
        );
        view.pokemon = Some(("Meowth".to_owned(), 12));
        view.watts = 9;
        assert_eq!(
            blocked(&view, App::Radar, 10).as_deref(),
            Some("Needs 10 watts")
        );
        view.watts = 10;
        assert_eq!(blocked(&view, App::Radar, 10), None);
        view.idle = false;
        assert_eq!(
            blocked(&view, App::Connect, 0).as_deref(),
            Some("Walker busy")
        );
    }

    #[test]
    fn pixel_text_sits_on_the_name_baseline() {
        let image = pixel_text("Lv 8");
        let rows: Vec<usize> = (0..image.height)
            .filter(|&y| (0..image.width).any(|x| image.shade(x, y) != 0))
            .collect();
        assert_eq!(rows.first(), Some(&TEXT_TOP));
        assert_eq!(rows.last(), Some(&(TEXT_TOP + 7)));
        assert_eq!(image.width, TEXT_LEFT + 5 + 1 + 6 + 1 + 3 + 1 + 5);
    }

    #[test]
    fn pixel_glyphs_have_even_rows() {
        for (c, rows) in GLYPHS {
            let widths: Vec<usize> = rows.split(' ').map(str::len).collect();
            assert!(widths.iter().all(|&w| w == widths[0]), "{c}");
            assert!(widths.len() <= 10, "{c}");
        }
        let empty = pixel_text("Empty");
        assert_eq!(empty.shade(TEXT_LEFT - 1, TEXT_TOP), 0);
        assert_eq!(empty.shade(TEXT_LEFT, TEXT_TOP), 3);
        assert_eq!(pixel_text("?").width, TEXT_LEFT + 3);
    }
}
