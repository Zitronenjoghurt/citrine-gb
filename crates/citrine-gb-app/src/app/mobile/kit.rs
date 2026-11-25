use crate::app::big_screen::scroll::Following;
use crate::app::widgets::settings::spotlight;
use crate::app::widgets::text::one_line;
use crate::icons;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, vec2,
};

pub const SIDE: f32 = 14.0;
pub const ROW_HEIGHT: f32 = 50.0;
pub const RADIUS: u8 = 14;
pub const CARD_PAD: i8 = 2;
pub const VALUE_EDGE: f32 = 16.0;
const ROW_EDGE: f32 = 12.0;
const TRAILING_COLUMN: f32 = 22.0;
pub const TRAILING_VALUE_END: f32 = ROW_EDGE + TRAILING_COLUMN;
pub const TITLE_GAP: f32 = 4.0;
pub const LABEL_SIZE: f32 = 16.0;
pub const DETAIL_SIZE: f32 = 13.0;
const ICON_COLUMN: f32 = 50.0;
const ICON_TILE: f32 = 36.0;
const ROW_FADE: f32 = 0.4;
const ROW_BADGE: f32 = 15.0;
const ROW_BADGE_GAP: f32 = 6.0;
const PICTURE_SIZE: f32 = 32.0;
const CUT_FADE: f32 = 28.0;

pub fn page_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.panel_fill
    } else {
        visuals
            .panel_fill
            .lerp_to_gamma(Color32::from_gray(232), 0.6)
    }
}

pub fn card_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.panel_fill.lerp_to_gamma(Color32::WHITE, 0.07)
    } else {
        visuals.extreme_bg_color.lerp_to_gamma(Color32::WHITE, 0.8)
    }
}

fn row_ring(ui: &Ui, response: &Response, rect: Rect) -> bool {
    crate::app::big_screen::ring::row(response, rect.shrink2(vec2(4.0, 1.0)), 10.0, ui.clip_rect())
}

pub fn icon_tile_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_white_alpha(18)
    } else {
        Color32::from_black_alpha(14)
    }
}

pub fn bar_track(visuals: &egui::Visuals) -> Color32 {
    use crate::app::ui_state::ui_theme::{TRACK_CONTRAST, contrast};
    let ground = card_fill(visuals);
    let ink = |alpha| {
        if visuals.dark_mode {
            Color32::from_white_alpha(alpha)
        } else {
            Color32::from_black_alpha(alpha)
        }
    };
    (0..=u8::MAX)
        .map(ink)
        .find(|track| contrast(ground.blend(*track), ground) >= TRACK_CONTRAST)
        .unwrap_or(ink(u8::MAX))
}

pub fn place<R>(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    let room = crate::app::toasts::strip_room(ui.ctx());
    let bottom = (SIDE - room).max(0.0);
    let scroll = egui::ScrollArea::vertical().id_salt(id).auto_shrink(false);
    let output = show_clear(ui, scroll, |ui| {
        Frame::new()
            .inner_margin(Margin {
                left: SIDE as i8,
                right: SIDE as i8,
                top: 0,
                bottom: bottom.round() as i8,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                body(ui)
            })
            .inner
    });
    keep_end(ui.ctx(), &output, room);
    output.inner
}

pub fn clear_of_toasts<R>(
    ui: &mut Ui,
    scroll: egui::ScrollArea,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    show_clear(ui, scroll, body).inner
}

fn show_clear<R>(
    ui: &mut Ui,
    scroll: egui::ScrollArea,
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let output = scroll.show_following(ui, |ui| {
        let inner = body(ui);
        super::sheet::lift(ui);
        inner
    });
    fade_cut(ui, &output);
    output
}

fn keep_end<R>(ctx: &egui::Context, output: &egui::scroll_area::ScrollAreaOutput<R>, room: f32) {
    let key = output.id.with("page_end");
    let end = (output.content_size.y - output.inner_rect.height()).max(0.0);
    let mut state = output.state;
    let last = ctx.data(|d| d.get_temp::<(f32, bool)>(key));
    if let Some((last_room, true)) = last
        && last_room != room
        && state.offset.y < end
    {
        state.offset.y = end;
        state.store(ctx, output.id);
        ctx.request_discard("the page keeps its end as the toast room changes");
    }
    let at_end = state.offset.y > 0.0 && state.offset.y >= end - 0.5;
    ctx.data_mut(|d| d.insert_temp(key, (room, at_end)));
}

fn fade_cut<R>(ui: &Ui, output: &egui::scroll_area::ScrollAreaOutput<R>) {
    if crate::app::toasts::strip_room(ui.ctx()) <= 0.0 {
        return;
    }
    let rect = output.inner_rect;
    let hidden = output.content_size.y - output.state.offset.y - rect.height();
    if hidden <= 0.5 {
        return;
    }
    let band = Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - CUT_FADE.min(rect.height())),
        rect.right_bottom(),
    );
    let fill = page_fill(ui.visuals());
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(band.right_top(), Color32::TRANSPARENT);
    mesh.colored_vertex(band.left_bottom(), fill);
    mesh.colored_vertex(band.right_bottom(), fill);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    ui.painter().add(egui::Shape::mesh(mesh));
}

pub fn hairline(visuals: &egui::Visuals) -> Stroke {
    Stroke::new(
        1.0_f32,
        visuals
            .widgets
            .noninteractive
            .bg_stroke
            .color
            .gamma_multiply(0.6),
    )
}

pub fn place_title(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    if crate::app::big_screen::tab_names(ui.ctx(), title) {
        let rect = crate::app::big_screen::tab_actions_rect(ui.ctx());
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        bar.set_clip_rect(rect);
        actions(&mut bar);
        return;
    }
    ui.horizontal(|ui| {
        ui.set_min_height(ui.spacing().interact_size.y);
        ui.add_space(4.0);
        ui.label(RichText::new(title).size(24.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(TITLE_GAP);
}

pub fn place_heading(ui: &mut Ui, title: &str) {
    if crate::app::big_screen::tab_names(ui.ctx(), title) {
        ui.add_space(6.0);
    } else {
        place_title(ui, title, |_| {});
    }
}

pub fn page_title(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) -> bool {
    let side = ui.spacing().interact_size.y;
    let mut back = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        back = ui
            .add(header_button(icons::ARROW_LEFT, side))
            .on_hover_text("Back")
            .clicked();
        ui.add(egui::Label::new(RichText::new(title).size(20.0).strong()).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(TITLE_GAP);
    back
}

pub fn header_button(icon: &str, side: f32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(icon).size(20.0))
        .frame(false)
        .min_size(Vec2::splat(side))
}

pub fn section_color(visuals: &egui::Visuals) -> Color32 {
    visuals.text_color()
}

pub fn value_color(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals
            .text_color()
            .lerp_to_gamma(visuals.strong_text_color(), 0.08)
    } else {
        visuals.text_color().lerp_to_gamma(card_fill(visuals), 0.25)
    }
}

const SCRIM_ALPHA: u8 = 61;

pub fn scrim(_visuals: &egui::Visuals) -> Color32 {
    Color32::from_black_alpha(SCRIM_ALPHA)
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    section_title(ui, text, None);
    ui.add_space(2.0);
}

pub fn section_title(ui: &mut Ui, text: &str, hint: Option<&str>) -> f32 {
    let label = ui.fonts_mut(|fonts| fonts.row_height(&FontId::proportional(13.0)));
    let mark = crate::app::mobile::touch_square(ui).max(ui.spacing().interact_size.y);
    let overhang = hint.map_or(0.0, |_| ((mark - label) / 2.0).max(0.0));
    ui.add_space(-overhang);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(section_color(ui.visuals())),
        );
        if let Some(hint) = hint {
            crate::app::widgets::info_mark::info_mark(ui, hint);
        }
    });
    overhang
}

pub fn section_label_button(ui: &mut Ui, text: &str, button: &str, hint: &str) -> bool {
    let mut clicked = false;
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(section_color(ui.visuals())),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            clicked = ui
                .add(
                    egui::Button::new(button)
                        .frame(false)
                        .min_size(egui::vec2(crate::app::mobile::touch_square(ui), 0.0)),
                )
                .on_hover_text(hint)
                .clicked();
        });
    });
    ui.add_space(2.0);
    clicked
}

pub fn section_label_total(
    ui: &mut Ui,
    text: &str,
    total: &str,
    button: Option<(&str, &str)>,
) -> bool {
    let mut clicked = false;
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(section_color(ui.visuals())),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((icon, hint)) = button {
                clicked = ui
                    .add(
                        egui::Button::new(icon)
                            .frame(false)
                            .min_size(egui::vec2(crate::app::mobile::touch_square(ui), 0.0)),
                    )
                    .on_hover_text(hint)
                    .clicked();
            } else {
                ui.add_space(VALUE_EDGE);
            }
            ui.label(RichText::new(total).size(13.0).strong());
        });
    });
    ui.add_space(2.0);
    clicked
}

pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(card_fill(ui.visuals()))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(0, CARD_PAD))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
        .inner
}

pub fn padded<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui)
        })
        .inner
}

#[cfg(feature = "nds")]
fn picture_rect(ctx: &egui::Context, center: egui::Pos2) -> Rect {
    crate::ds_banner::icon_rect(ctx, center, PICTURE_SIZE)
}

#[cfg(not(feature = "nds"))]
fn picture_rect(_ctx: &egui::Context, center: egui::Pos2) -> Rect {
    Rect::from_center_size(center, Vec2::splat(PICTURE_SIZE))
}

pub fn fit_inside(image: Vec2, room: Rect) -> Rect {
    if image.x <= 0.0 || image.y <= 0.0 {
        return room;
    }
    let scale = (room.width() / image.x).min(room.height() / image.y);
    Rect::from_center_size(room.center(), image * scale)
}

pub fn divider(ui: &mut Ui) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter()
        .hline(rect.left()..=rect.right(), y, hairline(ui.visuals()));
}

pub struct Row<'a> {
    icon: Option<&'a str>,
    picture: Option<egui::TextureId>,
    cover: Option<(egui::TextureId, Vec2)>,
    badge: Option<&'a str>,
    faded: bool,
    label: &'a str,
    detail: Option<&'a str>,
    info: Option<&'a str>,
    value: Option<String>,
    fallback: Option<String>,
    dot: Option<egui::Color32>,
    chevron: bool,
    trailing: &'a str,
    trailing_off: bool,
    trailing_accent: bool,
    checked: Option<bool>,
    danger: bool,
    accent: bool,
    enabled: bool,
    stepper: bool,
    whole_value: bool,
    press: Option<&'static str>,
}

impl<'a> Row<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            icon: None,
            picture: None,
            cover: None,
            badge: None,
            faded: false,
            label,
            detail: None,
            info: None,
            value: None,
            fallback: None,
            dot: None,
            chevron: true,
            trailing: icons::CARET_RIGHT,
            trailing_off: false,
            trailing_accent: false,
            checked: None,
            danger: false,
            accent: false,
            enabled: true,
            stepper: false,
            whole_value: false,
            press: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn picture(mut self, picture: Option<egui::TextureId>) -> Self {
        self.picture = picture;
        self
    }

    pub fn cover(mut self, cover: Option<(egui::TextureId, Vec2)>) -> Self {
        self.cover = cover;
        self
    }

    pub fn badge(mut self, badge: Option<&'a str>) -> Self {
        self.badge = badge;
        self
    }

    pub fn faded(mut self, faded: bool) -> Self {
        self.faded = faded;
        self
    }

    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = Some(detail).filter(|d| !d.is_empty());
        self
    }

    pub fn info(mut self, info: &'a str) -> Self {
        self.info = Some(info);
        self
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.value = (!value.is_empty()).then_some(value);
        self
    }

    pub fn fallback(mut self, value: impl Into<String>) -> Self {
        self.fallback = Some(value.into());
        self
    }

    pub fn no_chevron(mut self) -> Self {
        self.chevron = false;
        self
    }

    pub fn whole_value(mut self) -> Self {
        self.whole_value = true;
        self
    }

    pub fn dot(mut self, dot: Option<egui::Color32>) -> Self {
        self.dot = dot;
        self
    }

    pub fn trailing(mut self, icon: &'a str) -> Self {
        self.trailing = icon;
        self.chevron = true;
        self
    }

    pub fn trailing_space(self) -> Self {
        self.trailing("")
    }

    pub fn trailing_off(mut self) -> Self {
        self.trailing_off = true;
        self
    }

    pub fn trailing_accent(mut self, accent: bool) -> Self {
        self.trailing_accent = accent;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self.chevron = false;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn stepper(mut self) -> Self {
        self.stepper = true;
        self
    }

    pub fn press(mut self, label: &'static str) -> Self {
        self.press = Some(label);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response {
        if !self.enabled {
            self.enabled = true;
            return ui.add_enabled_ui(false, |ui| self.show(ui)).inner;
        }
        let whole = match &self.value {
            Some(value) if self.whole_value => {
                let label = one_line(ui, self.label, 16.0, Color32::PLACEHOLDER, f32::INFINITY);
                let room = ui.available_width() - 12.0 - VALUE_EDGE - label.size().x - VALUE_GAP;
                Some(one_line(ui, value, 15.0, Color32::PLACEHOLDER, room))
            }
            _ => None,
        };
        let height = row_height(ui, self.detail.is_some());
        let sense = if self.trailing_off {
            Sense::hover()
        } else {
            Sense::click()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
        crate::app::widgets::settings::page::mark(
            crate::app::widgets::settings::page::Mark::Row,
            rect,
        );
        if self.stepper {
            crate::app::big_screen::offer_steps(ui.ctx(), &response);
        }
        if let Some(press) = self.press {
            crate::app::big_screen::offer_press(ui.ctx(), &response, press);
        }
        if !ui.is_rect_visible(rect) {
            if self.info.is_some() {
                ui.skip_ahead_auto_ids(1);
            }
            return response;
        }
        focus_ring_at(&response, row_focus_rect(rect), 10.0);
        let ringed = row_ring(ui, &response, rect);
        let visuals = ui.visuals();
        if response.is_pointer_button_down_on() || (response.has_focus() && !ringed) {
            ui.painter().rect_filled(
                rect.shrink2(vec2(4.0, 1.0)),
                CornerRadius::same(10),
                visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
            );
        }
        let strong = if self.danger {
            crate::app::ui_state::ui_theme::danger_text(visuals)
        } else if self.faded {
            visuals.weak_text_color()
        } else {
            visuals.strong_text_color()
        };
        let tint = if self.faded {
            Color32::WHITE.gamma_multiply(ROW_FADE)
        } else {
            Color32::WHITE
        };
        let weak = visuals.weak_text_color();
        let secondary = value_color(visuals);
        let mark = crate::app::ui_state::ui_theme::brand_mark(visuals);
        let mut left = rect.left() + 12.0;
        if let Some(icon) = self.icon {
            let tile = Rect::from_center_size(
                egui::pos2(left + ICON_TILE / 2.0, rect.center().y),
                Vec2::splat(ICON_TILE),
            );
            ui.painter()
                .rect_filled(tile, CornerRadius::same(10), icon_tile_fill(visuals));
            match (self.cover, self.picture) {
                (Some((cover, size)), _) => {
                    egui::Image::new((cover, size))
                        .corner_radius(CornerRadius::same(4))
                        .tint(tint)
                        .paint_at(ui, fit_inside(size, tile.shrink(2.0)));
                }
                (None, Some(picture)) => {
                    ui.painter().image(
                        picture,
                        picture_rect(ui.ctx(), tile.center()),
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        tint,
                    );
                }
                (None, None) => {
                    ui.painter().text(
                        tile.center(),
                        Align2::CENTER_CENTER,
                        icon,
                        FontId::proportional(20.0),
                        if self.danger {
                            strong
                        } else {
                            visuals.text_color()
                        },
                    );
                }
            }
            if let Some(dot) = self.dot {
                crate::app::widgets::avatar::dot_in(ui, tile.expand(3.0), dot);
            }
            left += ICON_COLUMN;
        }
        let mut right = rect.right() - ROW_EDGE;
        if let Some(checked) = self.checked {
            if checked {
                ui.painter().text(
                    egui::pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    icons::CHECK,
                    FontId::proportional(18.0),
                    mark,
                );
            }
            right -= 28.0;
        }
        if self.chevron {
            ui.painter().text(
                egui::pos2(right, rect.center().y),
                Align2::RIGHT_CENTER,
                self.trailing,
                FontId::proportional(15.0),
                if self.trailing_off {
                    visuals.disable(weak)
                } else if self.trailing_accent {
                    crate::app::ui_state::ui_theme::brand_mark(visuals)
                } else {
                    weak
                },
            );
            right -= TRAILING_COLUMN;
        }
        if !self.chevron && self.checked.is_none() {
            right = rect.right() - VALUE_EDGE;
        }
        if let Some(galley) = whole {
            let pos = egui::pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            let spot = Rect::from_min_size(pos, galley.size());
            let cut = galley.elided;
            ui.painter().galley(pos, galley, secondary);
            if cut && let Some(value) = &self.value {
                crate::app::widgets::info_mark::tap_hint(&response, value, spot);
            }
            right -= spot.width() + 10.0;
        } else if let Some(value) = &self.value {
            let color = if self.accent { mark } else { secondary };
            let room = right - left;
            let fits = |text: &str| {
                let label = one_line(ui, self.label, LABEL_SIZE, strong, f32::INFINITY);
                let info = if self.info.is_some() {
                    crate::app::widgets::info_mark::width(ui) + 6.0
                } else {
                    0.0
                };
                let badge = if self.badge.is_some() {
                    ROW_BADGE_GAP + ROW_BADGE
                } else {
                    0.0
                };
                let width = one_line(ui, text, 15.0, color, f32::INFINITY).size().x;
                width <= room * 0.55 && label.size().x + info + badge + 10.0 + width <= room
            };
            let value = match &self.fallback {
                Some(short) if !fits(value) => short,
                _ => value,
            };
            let galley = one_line(ui, value, 15.0, color, (right - left) * 0.55);
            let width = galley.size().x.min((right - left) * 0.55);
            let pos = egui::pos2(right - width, rect.center().y - galley.size().y / 2.0);
            ui.painter()
                .with_clip_rect(Rect::from_min_max(
                    egui::pos2(right - width, rect.top()),
                    egui::pos2(right, rect.bottom()),
                ))
                .galley(pos, galley, color);
            right -= width + 10.0;
            if self.stepper && crate::app::big_screen::ring::shown(&response) {
                ui.painter().text(
                    egui::pos2(right + 4.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    icons::CARET_LEFT,
                    FontId::proportional(15.0),
                    weak,
                );
                right -= 22.0;
            }
        }
        let text_rect = Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(right, rect.bottom()),
        );
        let info_width = if self.info.is_some() {
            crate::app::widgets::info_mark::width(ui) + 6.0
        } else {
            0.0
        };
        let badge_width = if self.badge.is_some() {
            ROW_BADGE_GAP + ROW_BADGE
        } else {
            0.0
        };
        let label = one_line(
            ui,
            self.label,
            LABEL_SIZE,
            strong,
            text_rect.width() - info_width - badge_width,
        );
        let label_size = label.size();
        let top = match self.detail {
            Some(detail) => {
                let detail = one_line(ui, detail, DETAIL_SIZE, secondary, text_rect.width());
                let top = rect.center().y - (label.size().y + detail.size().y + 2.0) / 2.0;
                let detail_top = top + label.size().y + 2.0;
                ui.painter().galley(egui::pos2(left, top), label, strong);
                ui.painter()
                    .galley(egui::pos2(left, detail_top), detail, secondary);
                top
            }
            None => {
                let top = rect.center().y - label.size().y / 2.0;
                ui.painter().galley(egui::pos2(left, top), label, strong);
                top
            }
        };
        let line = top + label_size.y / 2.0;
        if let Some(badge) = self.badge {
            ui.painter().text(
                egui::pos2(left + label_size.x + ROW_BADGE_GAP, line),
                Align2::LEFT_CENTER,
                badge,
                FontId::proportional(ROW_BADGE),
                secondary,
            );
        }
        if let Some(info) = self.info {
            let after = left + label_size.x + badge_width;
            let mark = Rect::from_min_max(
                egui::pos2(after + 6.0, line - height / 2.0),
                egui::pos2(after + info_width, line + height / 2.0),
            );
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(mark)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            let shown = crate::app::big_screen::details_shown(&response);
            crate::app::widgets::info_mark::info_mark_shown(&mut cell, info, shown);
        }
        response.widget_info(|| match self.checked {
            Some(checked) => egui::WidgetInfo::selected(
                egui::WidgetType::RadioButton,
                ui.is_enabled(),
                checked,
                self.label,
            ),
            None => {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label)
            }
        });
        response
    }
}

const VALUE_GAP: f32 = 16.0;

fn row_height(ui: &Ui, detail: bool) -> f32 {
    let base = ROW_HEIGHT.max(ui.spacing().interact_size.y + 6.0);
    if detail { base + 8.0 } else { base }
}

pub fn rows(ui: &mut Ui, rows: Vec<Row<'_>>) -> Option<usize> {
    let mut clicked = None;
    card(ui, |ui| {
        let count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            if row.show(ui).clicked() {
                clicked = Some(index);
            }
            if index + 1 < count {
                divider(ui);
            }
        }
    });
    clicked
}

pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let height = ui.spacing().interact_size.y.max(26.0);
    let (cell, mut response) = ui.allocate_exact_size(vec2(52.0, height), Sense::click());
    let rect = Rect::from_center_size(cell.center(), vec2(44.0, 26.0));
    crate::app::big_screen::ring::mark(
        &response,
        crate::app::big_screen::ring::Mark::Ring {
            rect,
            radius: rect.height() / 2.0,
        },
    );
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_rect_visible(cell) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        paint_switch(ui, rect, t);
    }
    response
}

pub fn paint_switch(ui: &Ui, rect: Rect, t: f32) {
    paint_switch_with(ui.painter(), ui.visuals(), rect, t);
}

pub fn switch_edge(visuals: &egui::Visuals) -> Option<Color32> {
    (!visuals.dark_mode).then(|| {
        crate::app::ui_state::ui_theme::box_edge(visuals, visuals.widgets.inactive.bg_fill)
    })
}

pub fn switch_knob(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.weak_text_color()
    } else {
        Color32::WHITE
    }
}

pub fn paint_locked_switch(ui: &Ui, rect: Rect, on: bool) {
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(ui.visuals().disabled_alpha());
    paint_switch_with(&painter, ui.visuals(), rect, if on { 1.0 } else { 0.0 });
}

fn paint_switch_with(painter: &egui::Painter, visuals: &egui::Visuals, rect: Rect, t: f32) {
    let off = visuals.widgets.inactive.bg_fill;
    let fill = off.lerp_to_gamma(visuals.selection.bg_fill, t);
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, CornerRadius::same(radius as u8), fill);
    if let Some(edge) = switch_edge(visuals) {
        painter.rect_stroke(
            rect,
            CornerRadius::same(radius as u8),
            Stroke::new(1.0_f32, edge.lerp_to_gamma(fill, t)),
            egui::StrokeKind::Inside,
        );
    }
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = switch_knob(visuals).lerp_to_gamma(Color32::WHITE, t);
    painter.circle_filled(
        egui::pos2(x, rect.center().y),
        radius - (radius * 0.23).round(),
        knob,
    );
}

pub struct SwitchRow<'a> {
    label: &'a str,
    icon: Option<&'a str>,
    detail: Option<&'a str>,
    value: Option<&'a str>,
    note: Option<&'a str>,
    spoken: Option<&'a str>,
    info: Option<&'a str>,
    on: &'a mut bool,
    enabled: bool,
    locked: bool,
}

impl<'a> SwitchRow<'a> {
    pub fn new(label: &'a str, on: &'a mut bool) -> Self {
        Self {
            label,
            icon: None,
            detail: None,
            value: None,
            note: None,
            spoken: None,
            info: None,
            on,
            enabled: true,
            locked: false,
        }
    }

    pub fn note(mut self, note: Option<&'a str>) -> Self {
        self.note = note.filter(|n| !n.is_empty());
        self
    }

    pub fn spoken(mut self, name: &'a str) -> Self {
        self.spoken = Some(name);
        self
    }

    pub fn locked(mut self, locked: bool) -> Self {
        self.locked = locked;
        self
    }

    pub fn detail(mut self, detail: Option<&'a str>) -> Self {
        self.detail = detail.filter(|d| !d.is_empty());
        self
    }

    pub fn value(mut self, value: Option<&'a str>) -> Self {
        self.value = value;
        self
    }

    pub fn info(mut self, info: &'a str) -> Self {
        self.info = Some(info);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> bool {
        let height = row_height(ui, self.detail.is_some());
        let icon_room = if self.icon.is_some() {
            ICON_COLUMN
        } else {
            0.0
        };
        let left = ui.available_rect_before_wrap().left() + 12.0 + icon_room;
        let note = self.note.map(|note| {
            let width = ui.available_rect_before_wrap().right() - 12.0 - left;
            let color = ui.visuals().text_color();
            ui.fonts_mut(|f| {
                f.layout(
                    note.to_owned(),
                    FontId::proportional(DETAIL_SIZE),
                    color,
                    width,
                )
            })
        });
        let note_height = note.as_ref().map_or(0.0, |note| note.size().y + 2.0);
        let (rect, row) = ui.allocate_exact_size(
            vec2(ui.available_width(), height + note_height),
            Sense::click(),
        );
        crate::app::widgets::settings::page::mark(
            crate::app::widgets::settings::page::Mark::Row,
            rect,
        );
        let lines = Rect::from_min_size(rect.min, vec2(rect.width(), height));
        let enabled = self.enabled && !self.locked && ui.is_enabled();
        let spoken = self.spoken.unwrap_or(self.label);
        row.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, *self.on, spoken)
        });
        let mut changed = false;
        if enabled && row.clicked() {
            *self.on = !*self.on;
            changed = true;
        }
        row_ring(ui, &row, rect);
        focus_ring_at(&row, row_focus_rect(rect), 10.0);
        let switch_width = 52.0;
        let switch_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 12.0 - switch_width / 2.0, lines.center().y),
            vec2(switch_width, height),
        );
        if self.locked {
            if ui.is_rect_visible(switch_rect) {
                let track = Rect::from_center_size(switch_rect.center(), vec2(44.0, 26.0));
                paint_locked_switch(ui, track, *self.on);
            }
        } else {
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(switch_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            cell.add_enabled_ui(self.enabled, |ui| {
                changed |= switch(ui, self.on).changed();
            });
        }
        if !ui.is_rect_visible(rect) {
            if self.info.is_some() {
                ui.skip_ahead_auto_ids(1);
            }
            return changed;
        }
        let visuals = ui.visuals();
        let strong = if self.enabled {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        let secondary = value_color(visuals);
        if let Some(icon) = self.icon {
            let tile = Rect::from_center_size(
                egui::pos2(rect.left() + 12.0 + ICON_TILE / 2.0, lines.center().y),
                Vec2::splat(ICON_TILE),
            );
            ui.painter()
                .rect_filled(tile, CornerRadius::same(10), icon_tile_fill(visuals));
            ui.painter().text(
                tile.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(20.0),
                visuals.text_color(),
            );
        }
        let info_width = if self.info.is_some() {
            crate::app::widgets::info_mark::width(ui)
        } else {
            0.0
        };
        let mut right = switch_rect.left() - 8.0;
        if let Some(value) = self.value {
            let galley = one_line(ui, value, 15.0, secondary, (right - left) * 0.45);
            let pos = egui::pos2(
                right - galley.size().x,
                lines.center().y - galley.size().y / 2.0,
            );
            right = pos.x - 10.0;
            ui.painter().galley(pos, galley, secondary);
        }
        let room = right - left - info_width;
        let label = one_line(ui, self.label, LABEL_SIZE, strong, room);
        let detail = self
            .detail
            .map(|detail| one_line(ui, detail, DETAIL_SIZE, secondary, room));
        let block = label.size().y + detail.as_ref().map_or(0.0, |d| d.size().y + 2.0);
        let top = lines.center().y - block / 2.0;
        let label_width = label.size().x;
        let label_height = label.size().y;
        ui.painter().galley(egui::pos2(left, top), label, strong);
        if let Some(detail) = detail {
            ui.painter().galley(
                egui::pos2(left, top + label_height + 2.0),
                detail,
                secondary,
            );
        }
        if let Some(note) = note {
            let color = visuals.text_color();
            ui.painter()
                .galley(egui::pos2(left, top + block + 2.0), note, color);
        }
        if let Some(info) = self.info {
            let line = top + label_height / 2.0;
            let mark = Rect::from_min_max(
                egui::pos2(left + label_width + 6.0, line - height / 2.0),
                egui::pos2(left + label_width + 6.0 + info_width, line + height / 2.0),
            );
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(mark)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            let shown = crate::app::big_screen::details_shown(&row);
            crate::app::widgets::info_mark::info_mark_shown(&mut cell, info, shown);
        }
        changed
    }
}

pub struct SliderRow<'a> {
    label: &'a str,
    icon: Option<&'a str>,
    value: &'a mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f64,
}

impl<'a> SliderRow<'a> {
    pub fn new(
        label: &'a str,
        value: &'a mut f32,
        range: std::ops::RangeInclusive<f32>,
        step: f64,
    ) -> Self {
        Self {
            label,
            icon: None,
            value,
            range,
            step,
        }
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let height = row_height(ui, false);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        crate::app::widgets::settings::page::mark(
            crate::app::widgets::settings::page::Mark::Row,
            rect,
        );
        let mut left = rect.left() + 12.0;
        if self.icon.is_some() {
            left += ICON_COLUMN;
        }
        let right = rect.right() - VALUE_EDGE;
        let width = (right - left) * 0.55;
        let cell = Rect::from_min_max(
            egui::pos2(right - width, rect.top()),
            egui::pos2(right, rect.bottom()),
        );
        let mut cell = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(cell)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let response =
            crate::app::mobile::sheet::slider(&mut cell, self.value, self.range, self.step, width);
        row_ring(ui, &response, rect);
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let visuals = ui.visuals();
        let strong = visuals.strong_text_color();
        if let Some(icon) = self.icon {
            let tile = Rect::from_center_size(
                egui::pos2(rect.left() + 12.0 + ICON_TILE / 2.0, rect.center().y),
                Vec2::splat(ICON_TILE),
            );
            ui.painter()
                .rect_filled(tile, CornerRadius::same(10), icon_tile_fill(visuals));
            ui.painter().text(
                tile.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(20.0),
                visuals.text_color(),
            );
        }
        let room = right - width - 8.0 - left;
        let label = one_line(ui, self.label, 16.0, strong, room);
        let top = rect.center().y - label.size().y / 2.0;
        ui.painter().galley(egui::pos2(left, top), label, strong);
        response
    }
}

pub struct List<'u> {
    ui: &'u mut Ui,
    count: usize,
    edges: Vec<f32>,
}

impl List<'_> {
    fn gap(&mut self) {
        if self.count > 0 {
            self.edges.push(self.ui.cursor().top());
            divider(self.ui);
        }
        self.count += 1;
    }

    pub fn edges(&self) -> &[f32] {
        &self.edges
    }

    pub fn row(&mut self, row: Row<'_>) -> Response {
        self.gap();
        let label = row.label;
        let response = row.show(self.ui);
        spotlight::mark(self.ui, label, response.rect);
        response
    }

    pub fn switch(&mut self, row: SwitchRow<'_>) -> bool {
        self.gap();
        let label = row.label;
        let top = self.ui.cursor().top();
        let changed = row.show(self.ui);
        let rect = Rect::from_x_y_ranges(
            self.ui.max_rect().x_range(),
            top..=self.ui.min_rect().bottom(),
        );
        spotlight::mark(self.ui, label, rect);
        changed
    }

    pub fn slider(&mut self, row: SliderRow<'_>) -> Response {
        self.gap();
        let label = row.label;
        let top = self.ui.cursor().top();
        let response = row.show(self.ui);
        let rect = Rect::from_x_y_ranges(
            self.ui.max_rect().x_range(),
            top..=self.ui.min_rect().bottom(),
        );
        spotlight::mark(self.ui, label, rect);
        response
    }

    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }

    pub fn custom<R>(&mut self, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.gap();
        let top = self.ui.cursor().top();
        let inner = body(self.ui);
        crate::app::widgets::settings::page::mark(
            crate::app::widgets::settings::page::Mark::Row,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
        inner
    }
}

pub fn list<R>(ui: &mut Ui, body: impl FnOnce(&mut List<'_>) -> R) -> R {
    card(ui, |ui| {
        body(&mut List {
            ui,
            count: 0,
            edges: Vec::new(),
        })
    })
}

pub const COMPACT_ROW: f32 = 44.0;
pub(super) const COMPACT_ICON: f32 = 20.0;
const COMPACT_ICON_COLUMN: f32 = 32.0;
const COMPACT_EDGE: f32 = 4.0;
const COMPACT_SWITCH: Vec2 = vec2(40.0, 24.0);

enum CompactEnd {
    Nothing,
    Chevron(Option<String>),
    Switch(bool),
}

pub struct CompactRow<'a> {
    icon: &'a str,
    turned: bool,
    label: &'a str,
    end: CompactEnd,
}

impl<'a> CompactRow<'a> {
    pub fn new(icon: &'a str, label: &'a str) -> Self {
        Self {
            icon,
            turned: false,
            label,
            end: CompactEnd::Nothing,
        }
    }

    pub fn turned(mut self, turned: bool) -> Self {
        self.turned = turned;
        self
    }

    pub fn chevron(mut self) -> Self {
        self.end = CompactEnd::Chevron(None);
        self
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.end = CompactEnd::Chevron((!value.is_empty()).then_some(value));
        self
    }

    pub fn switch(mut self, on: bool) -> Self {
        self.end = CompactEnd::Switch(on);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let height = COMPACT_ROW.max(ui.spacing().interact_size.y);
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let label = self.label;
        response.widget_info(|| match self.end {
            CompactEnd::Switch(on) => {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, label)
            }
            _ => egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label),
        });
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let ringed = crate::app::big_screen::ring::row(&response, rect, 10.0, ui.clip_rect());
        let visuals = ui.visuals();
        if response.is_pointer_button_down_on() || (response.has_focus() && !ringed) {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(10),
                visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
            );
        }
        let strong = visuals.strong_text_color();
        let weak = visuals.weak_text_color();
        let left = rect.left() + COMPACT_EDGE;
        let icon_at = egui::pos2(left + COMPACT_ICON / 2.0, rect.center().y);
        let icon_font = FontId::proportional(COMPACT_ICON);
        if self.turned {
            let ink = visuals.text_color();
            let galley = ui
                .painter()
                .layout_no_wrap(self.icon.to_owned(), icon_font, ink);
            let corner = icon_at - galley.size() / 2.0;
            ui.painter().add(
                egui::epaint::TextShape::new(corner, galley, ink)
                    .with_angle_and_anchor(std::f32::consts::FRAC_PI_2, Align2::CENTER_CENTER),
            );
        } else {
            ui.painter().text(
                icon_at,
                Align2::CENTER_CENTER,
                self.icon,
                icon_font,
                visuals.text_color(),
            );
        }
        let text_left = left + COMPACT_ICON_COLUMN;
        let mut right = rect.right() - COMPACT_EDGE;
        match &self.end {
            CompactEnd::Nothing => {}
            CompactEnd::Switch(on) => {
                let t = ui.ctx().animate_bool_responsive(response.id, *on);
                let switch = Rect::from_min_size(
                    egui::pos2(
                        right - COMPACT_SWITCH.x,
                        rect.center().y - COMPACT_SWITCH.y / 2.0,
                    ),
                    COMPACT_SWITCH,
                );
                paint_switch(ui, switch, t);
                right = switch.left() - 12.0;
            }
            CompactEnd::Chevron(value) => {
                ui.painter().text(
                    egui::pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    icons::CARET_RIGHT,
                    FontId::proportional(14.0),
                    weak,
                );
                right -= 22.0;
                if let Some(value) = value {
                    let color = value_color(visuals);
                    let room = (right - text_left) * 0.55;
                    let galley = one_line(ui, value, 14.0, color, room);
                    let width = galley.size().x.min(room);
                    let at = egui::pos2(right - width, rect.center().y - galley.size().y / 2.0);
                    ui.painter().galley(at, galley, color);
                    right -= width + 12.0;
                }
            }
        }
        let galley = one_line(ui, label, 15.0, strong, right - text_left);
        let at = egui::pos2(text_left, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, strong);
        response
    }
}

pub struct Groups<'u> {
    ui: &'u mut Ui,
    line: bool,
}

impl Groups<'_> {
    pub fn group<R>(&mut self, body: impl FnOnce(&mut Ui) -> R) -> R {
        if self.line {
            let (rect, _) = self
                .ui
                .allocate_exact_size(vec2(self.ui.available_width(), 1.0), Sense::hover());
            self.ui
                .painter()
                .hline(rect.x_range(), rect.center().y, hairline(self.ui.visuals()));
        }
        self.line = true;
        body(self.ui)
    }
}

pub fn groups<R>(ui: &mut Ui, lead_line: bool, body: impl FnOnce(&mut Groups<'_>) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        body(&mut Groups {
            ui,
            line: lead_line,
        })
    })
    .inner
}

fn pill_colors(ui: &Ui, primary: bool) -> (Color32, Color32) {
    let visuals = ui.visuals();
    if primary && ui.is_enabled() {
        let fill = crate::app::ui_state::ui_theme::brand(visuals);
        (fill, crate::app::ui_state::ui_theme::ink_on(fill))
    } else {
        (icon_tile_fill(visuals), visuals.strong_text_color())
    }
}

pub fn pill(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let (fill, ink) = pill_colors(ui, primary);
    let height = ui.spacing().interact_size.y.max(36.0);
    let response = ui.add(
        egui::Button::new(RichText::new(text).size(15.0).color(ink))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same((height / 2.0) as u8))
            .min_size(vec2(height * 1.8, height)),
    );
    crate::app::big_screen::ring::mark(
        &response,
        crate::app::big_screen::ring::Mark::Ring {
            rect: response.rect,
            radius: response.rect.height() / 2.0,
        },
    );
    focus_ring(&response, height / 2.0);
    response
}

fn row_focus_rect(rect: Rect) -> Rect {
    rect.shrink(crate::app::big_screen::ring::MARGIN)
}

fn focus_ring(response: &Response, radius: f32) {
    focus_ring_at(response, response.rect, radius);
}

fn focus_ring_at(response: &Response, rect: Rect, radius: f32) {
    use crate::app::big_screen::ring;
    if !response.has_focus() || ring::shown(response) {
        return;
    }
    let color = crate::app::ui_state::ui_theme::focus_ring(&response.ctx.style().visuals);
    let corner = (radius + ring::GAP).round().clamp(0.0, u8::MAX as f32) as u8;
    response
        .ctx
        .layer_painter(response.layer_id)
        .with_clip_rect(response.interact_rect.expand(ring::MARGIN + 1.0))
        .rect_stroke(
            rect.expand(ring::GAP),
            CornerRadius::same(corner),
            Stroke::new(ring::WIDTH, color),
            egui::StrokeKind::Outside,
        );
}

pub fn pill_width(ui: &Ui, text: &str) -> f32 {
    let height = ui.spacing().interact_size.y.max(36.0);
    let text = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(15.0), Color32::WHITE)
        .size()
        .x;
    (text + 2.0 * ui.spacing().button_padding.x).max(height * 1.8)
}

pub fn icon_pill_width(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y.max(36.0)
}

pub fn pill_height(ui: &Ui) -> f32 {
    ui.spacing().interact_size.y.max(36.0)
}

pub fn labeled(response: Response, label: &str) -> Response {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

pub fn icon_pill(ui: &mut Ui, icon: &str) -> Response {
    let (fill, ink) = pill_colors(ui, false);
    let height = ui.spacing().interact_size.y.max(36.0);
    ui.add(
        egui::Button::new(RichText::new(icon).size(17.0).color(ink))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same((height / 2.0) as u8))
            .min_size(vec2(height, height)),
    )
}

pub fn wide_button(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let colors = pill_colors(ui, primary);
    wide(ui, text, colors)
}

pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    let colors = if ui.is_enabled() {
        (
            crate::app::ui_state::ui_theme::danger(ui.visuals()),
            crate::app::ui_state::ui_theme::ON_DANGER,
        )
    } else {
        pill_colors(ui, false)
    };
    wide(ui, text, colors)
}

fn wide(ui: &mut Ui, text: &str, (fill, ink): (Color32, Color32)) -> Response {
    let height = ROW_HEIGHT.max(ui.spacing().interact_size.y);
    ui.vertical_centered(|ui| {
        let response = ui.add(
            egui::Button::new(RichText::new(text).size(16.0).color(ink))
                .fill(fill)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(RADIUS))
                .min_size(vec2(ui.available_width(), height)),
        );
        focus_ring(&response, f32::from(RADIUS));
        response
    })
    .inner
}

pub fn field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.corner_radius = CornerRadius::same(10);
        }
        let height = ui.spacing().interact_size.y.max(40.0);
        let width = ui.available_width();
        crate::app::widgets::field::framed(
            ui,
            edit.font(FontId::proportional(16.0))
                .vertical_align(Align::Center)
                .margin(Margin::symmetric(10, 0))
                .desired_width(width)
                .min_size(vec2(0.0, height)),
        )
        .response
    })
    .inner
}

pub fn primary_button(ui: &mut Ui, text: &str) -> bool {
    wide_button(ui, text, true).clicked()
}

pub fn segmented_look(ui: &Ui) -> crate::app::widgets::segmented::Look {
    let height = 38.0_f32.max(ui.spacing().interact_size.y);
    let visuals = ui.visuals();
    crate::app::widgets::segmented::Look {
        height,
        radius: height / 2.0,
        font: 15.0,
        track: icon_tile_fill(visuals),
        thumb: crate::app::widgets::segmented::raised(visuals),
        divider: Color32::TRANSPARENT,
        full_width: true,
    }
}

pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, options: &[(T, &str)], current: T) -> Option<T> {
    let look = segmented_look(ui);
    crate::app::widgets::segmented::segmented_with(ui, &look, options, current)
}

#[cfg(test)]
mod tests {
    use crate::app::ui_state::ui_theme::UiTheme;
    use crate::app::widgets::segmented::Look;
    use egui::Color32;
    use strum::IntoEnumIterator;

    #[test]
    fn a_phone_track_stands_out_from_its_card_in_both_themes() {
        use crate::app::ui_state::ui_theme::{TRACK_CONTRAST, contrast};
        for theme in UiTheme::iter() {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let mut fills = Vec::new();
            let shapes = ctx
                .run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.spacing_mut().interact_size.y = 48.0;
                        crate::app::widgets::progress::bar(ui, Some(0.5));
                    });
                })
                .shapes;
            for clipped in shapes {
                if let egui::Shape::Rect(rect) = clipped.shape
                    && (rect.rect.height() - crate::app::widgets::progress::BAR_HEIGHT).abs() < 0.01
                {
                    fills.push(rect.fill);
                }
            }
            let card = super::card_fill(&ctx.style().visuals);
            let seen = card.blend(fills[0]);
            assert!(
                contrast(seen, card) >= TRACK_CONTRAST,
                "{theme}: track {seen:?} on {card:?}"
            );
        }
    }

    #[test]
    fn a_row_shows_its_fallback_value_when_the_whole_value_would_crowd_its_label() {
        let shown = |width: f32| {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 200.0),
                )),
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    super::Row::new("Achievement badges")
                        .value("1,234 screenshots · 40.0 MB")
                        .fallback("40.0 MB")
                        .show(ui);
                });
            })
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
        };
        let wide = shown(900.0);
        assert!(
            wide.iter()
                .any(|text| text == "1,234 screenshots · 40.0 MB"),
            "{wide:?}"
        );
        let narrow = shown(360.0);
        assert!(narrow.iter().any(|text| text == "40.0 MB"), "{narrow:?}");
        assert!(
            !narrow.iter().any(|text| text.contains(" · ")),
            "{narrow:?}"
        );
    }

    fn oklch_lightness(color: Color32) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (channel(color.r()), channel(color.g()), channel(color.b()));
        let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
        let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
        let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s
    }

    #[test]
    fn the_scrim_keeps_yellow_under_it_yellow() {
        for visuals in [egui::Visuals::light(), egui::Visuals::dark()] {
            let brand = crate::app::ui_state::ui_theme::brand(&visuals);
            let scrim = super::scrim(&visuals);
            let keep = 1.0 - f32::from(scrim.a()) / 255.0;
            let under = Color32::from_rgb(
                (f32::from(brand.r()) * keep).round() as u8,
                (f32::from(brand.g()) * keep).round() as u8,
                (f32::from(brand.b()) * keep).round() as u8,
            );
            let lightness = oklch_lightness(under);
            assert!(
                lightness >= 0.65,
                "{brand:?} under the scrim is {under:?}, OKLCH L {lightness:.3}, olive below 0.6"
            );
            assert!(scrim.a() >= 50, "the scrim still dims the page");
        }
    }

    fn grey(colour: Color32) -> f32 {
        let [r, g, b, _] = colour.to_array();
        0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
    }

    fn over(top: Color32, ground: Color32) -> Color32 {
        let [r, g, b, a] = top.to_array();
        let keep = 1.0 - a as f32 / 255.0;
        let [gr, gg, gb, _] = ground.to_array();
        let mix = |t: u8, u: u8| (t as f32 + u as f32 * keep).round().min(255.0) as u8;
        Color32::from_rgb(mix(r, gr), mix(g, gg), mix(b, gb))
    }

    fn fills(theme: UiTheme, paint: impl Fn(&mut egui::Ui, egui::Rect)) -> Vec<Color32> {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let track =
                        egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(52.0, 32.0));
                    paint(ui, track);
                });
        })
        .shapes
        .into_iter()
        .filter_map(|clipped| match clipped.shape {
            egui::Shape::Rect(rect) if rect.fill != Color32::TRANSPARENT => Some(rect.fill),
            egui::Shape::Circle(circle) => Some(circle.fill),
            _ => None,
        })
        .collect()
    }

    #[test]
    fn a_locked_switch_draws_like_a_disabled_switch_in_both_themes() {
        for theme in [UiTheme::Light, UiTheme::Dark] {
            for on in [true, false] {
                let t = if on { 1.0 } else { 0.0 };
                let locked = fills(theme, |ui, track| super::paint_locked_switch(ui, track, on));
                let disabled = fills(theme, |ui, track| {
                    ui.disable();
                    super::paint_switch(ui, track, t);
                });
                let usable = fills(theme, |ui, track| super::paint_switch(ui, track, t));
                assert_eq!(locked.len(), 2, "{theme:?} on {on}: track and knob");
                assert_eq!(locked, disabled, "{theme:?} on {on}");
                assert_ne!(locked, usable, "{theme:?} on {on}: locked reads faded");
            }
        }
    }

    fn assert_picked_stands_out(name: &str, look: &Look, visuals: &egui::Visuals) {
        let track = over(look.track, visuals.panel_fill);
        let lift = grey(look.thumb) - grey(track);
        assert!(
            lift >= 18.0,
            "{name}: the picked segment is {lift} lighter than its track"
        );
    }

    const NOTICE_GAP: f32 = 8.0;

    fn page(
        ctx: &egui::Context,
        toasts: &mut crate::app::toasts::Toasts,
        body: impl FnOnce(&mut egui::Ui),
    ) {
        let (need, room) = toasts.page_strip(ctx);
        if room > 0.0 {
            egui::TopBottomPanel::bottom("toast_room")
                .exact_height(room)
                .show_separator_line(false)
                .show(ctx, |ui| {
                    toasts.place_strip(ui.max_rect(), need.unwrap_or(room), super::SIDE);
                });
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            if room == 0.0 {
                toasts.place_page(ui.max_rect());
            }
            body(ui);
        });
    }

    #[test]
    fn a_tab_page_scrolls_its_last_row_clear_of_a_toast_with_undo() {
        use crate::app::events::AppEvent;
        use crate::app::tabs::Tab;
        use crate::app::toasts::Toasts;
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            ..Default::default()
        };
        let mut toasts = Toasts::default();
        toasts
            .success("You're playing Mika's save. Yours is kept in Saves.")
            .button("Undo", AppEvent::OpenTab { tab: Tab::Media });
        let mut last = egui::Rect::NOTHING;
        for _ in 0..80 {
            let _ = ctx.run(input(), |ctx| {
                page(ctx, &mut toasts, |ui| {
                    let scroll = egui::ScrollArea::vertical()
                        .id_salt("tab_page")
                        .auto_shrink(false);
                    super::clear_of_toasts(ui, scroll, |ui| {
                        for row in 0..40 {
                            last = ui.label(format!("Row {row}")).rect;
                        }
                        ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    });
                });
                toasts.show(ctx);
            });
        }
        let toast = toasts.rects(&ctx)[0];
        assert!(
            last.bottom() <= toast.top() - NOTICE_GAP + 0.5,
            "{last:?} touches {toast:?}"
        );
    }

    #[test]
    fn no_row_sits_under_a_notice_waiting_for_an_answer_at_any_scroll() {
        use crate::app::events::AppEvent;
        use crate::app::tabs::Tab;
        use crate::app::toasts::Toasts;
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            ..Default::default()
        };
        let mut toasts = Toasts::default();
        toasts
            .info("Mika's saves didn't replace yours.")
            .button("Use", AppEvent::OpenTab { tab: Tab::Media })
            .or("Delete", AppEvent::OpenTab { tab: Tab::Games })
            .needs_action();
        for end in [false, true] {
            let mut rows = Vec::new();
            for _ in 0..80 {
                let _ = ctx.run(input(), |ctx| {
                    page(ctx, &mut toasts, |ui| {
                        rows.clear();
                        super::place(ui, "rows", |ui| {
                            for row in 0..30 {
                                let rect = ui.label(format!("Row {row}")).rect;
                                rows.push(rect.intersect(ui.clip_rect()));
                            }
                            if end {
                                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                            }
                        });
                    });
                    toasts.show(ctx);
                });
            }
            let notice = toasts.rects(&ctx)[0];
            let shown: Vec<_> = rows.iter().filter(|row| row.is_positive()).collect();
            assert!(shown.len() > 5, "end {end}: {} rows shown", shown.len());
            for row in shown {
                assert!(
                    row.bottom() <= notice.top() - NOTICE_GAP + 0.5,
                    "end {end}: {row:?} touches {notice:?}"
                );
            }
        }
    }

    #[test]
    fn a_switch_row_note_starts_on_the_name_edge() {
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::list(ui, |list| {
                    let mut on = true;
                    list.switch(
                        super::SwitchRow::new("Clara's laptop", &mut on)
                            .value(Some("Not syncing"))
                            .note(Some("On that device, turn on \"Sync saves\"")),
                    );
                });
            });
        });
        let left = |wanted: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.pos.x + text.galley.rect.left())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {wanted}"))
        };
        let name = left("Clara's laptop");
        let note = left("On that device, turn on \"Sync saves\"");
        assert!((name - note).abs() < 0.5, "name {name}, note {note}");
    }

    #[test]
    fn the_picked_segment_is_lighter_than_its_track_in_every_theme() {
        for theme in UiTheme::iter().filter(|theme| *theme != UiTheme::System) {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let desktop = Look::desktop(ui);
                    assert_picked_stands_out(&format!("desktop {theme:?}"), &desktop, ui.visuals());
                    let phone = super::segmented_look(ui);
                    assert_picked_stands_out(&format!("phone {theme:?}"), &phone, ui.visuals());
                });
            });
        }
    }

    #[test]
    fn an_off_switch_reads_as_off_in_every_theme() {
        use crate::app::ui_state::ui_theme::contrast;
        for theme in UiTheme::iter() {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let visuals = ctx.style().visuals.clone();
            let edge = super::switch_edge(&visuals);
            let knob = super::switch_knob(&visuals);
            if visuals.dark_mode {
                assert_eq!(edge, None, "{theme:?}");
                let track = visuals.widgets.inactive.bg_fill;
                let ratio = contrast(knob, track);
                assert!(
                    ratio >= 3.0 && knob != Color32::WHITE,
                    "{theme:?} off knob {knob:?} on {track:?}: {ratio}"
                );
                continue;
            }
            assert_eq!(knob, Color32::WHITE, "{theme:?}");
            let edge = edge.unwrap();
            for ground in [
                Color32::WHITE,
                super::card_fill(&visuals),
                visuals.panel_fill,
            ] {
                let ratio = contrast(edge, ground);
                assert!(
                    ratio >= 3.0,
                    "{theme:?} edge {edge:?} on {ground:?}: {ratio}"
                );
            }
        }
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_row_badge_sits_after_the_name_and_off_the_cover() {
        let name = "Pokemon - Crystal Version";
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 400.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..2 {
            output = Some(ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let row = super::Row::new(name)
                        .icon(crate::icons::GAME_CONTROLLER)
                        .badge(Some(crate::icons::CLOUD_CHECK))
                        .no_chevron();
                    super::rows(ui, vec![row]);
                });
            }));
        }
        let texts = texts(&output.unwrap());
        let find = |wanted: &str| {
            texts
                .iter()
                .find(|(text, _)| text == wanted)
                .map(|(_, rect)| *rect)
                .unwrap_or_else(|| panic!("no {wanted:?} in {texts:?}"))
        };
        let label = find(name);
        let badge = find(crate::icons::CLOUD_CHECK);
        let tile = find(crate::icons::GAME_CONTROLLER);
        assert!(
            badge.left() >= label.right(),
            "badge {badge:?} over name {label:?}"
        );
        assert!(
            badge.left() > tile.right() + super::ICON_TILE / 2.0,
            "badge {badge:?} on the cover {tile:?}"
        );
        assert!(
            (badge.center().y - label.center().y).abs() < 4.0,
            "badge {badge:?} off the name's line {label:?}"
        );
    }
}
