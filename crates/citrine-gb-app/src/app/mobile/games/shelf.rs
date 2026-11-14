use super::RecentGame;
use crate::app::Citrine;
use crate::app::big_screen::Cover;
use crate::app::mobile::kit;
use crate::app::widgets::text::one_line;
use crate::icons;
use egui::{Align2, CornerRadius, FontId, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

const CARD_MIN_WIDTH: f32 = 150.0;
const GAP: f32 = 16.0;
const PICTURE_SHAPE: f32 = 0.9;
const ICON_SIDE: f32 = 64.0;
const BOX_MARGIN: f32 = 10.0;
const PLAIN_ICON: f32 = 40.0;
const PLAIN_LIFT: f32 = 12.0;
const PLAIN_GAP: f32 = 6.0;

enum Picture {
    Cover(Cover),
    Icon(egui::TextureId),
    Plain,
}

impl Citrine {
    pub(super) fn recent_shelf(&mut self, ui: &mut Ui, recent: &[RecentGame]) -> Option<usize> {
        let ctx = ui.ctx().clone();
        let playing = self.emulator.rom_key().map(str::to_owned);
        let boxes = self.ui.settings.covers;
        let mut pictures = Vec::with_capacity(recent.len());
        for game in recent {
            let cover = self.ui.recent.game_key(&game.path).and_then(|key| {
                self.big
                    .covers
                    .get(&ctx, &self.emulator.store, key, playing.as_deref(), boxes)
            });
            pictures.push(match cover {
                Some(cover) => Picture::Cover(cover),
                None => match self.recent_picture(&ctx, &game.path) {
                    Some(icon) => Picture::Icon(icon),
                    None => Picture::Plain,
                },
            });
        }
        let width = ui.available_width();
        let columns = ((width + GAP) / (CARD_MIN_WIDTH + GAP)).floor().max(1.0) as usize;
        let card = (width - GAP * (columns - 1) as f32) / columns as f32;
        let mut picked = None;
        self.big.missing.clear();
        let spacing = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = GAP;
        for (row, games) in recent.chunks(columns).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (column, game) in games.iter().enumerate() {
                    let index = row * columns + column;
                    let response = shelf_card(ui, game, &pictures[index], card);
                    if game.gone {
                        self.big.missing.push(response.id);
                    }
                    if response.clicked() {
                        picked = Some(index);
                    }
                }
            });
        }
        ui.spacing_mut().item_spacing.y = spacing;
        picked
    }
}

fn shelf_card(ui: &mut Ui, game: &RecentGame, picture: &Picture, width: f32) -> Response {
    let picture_height = width * PICTURE_SHAPE;
    let line = 22.0;
    let lines = if game.gone { 2.0 } else { 1.0 };
    let size = vec2(width, picture_height + 8.0 + line * lines);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let frame = Rect::from_min_size(rect.min, vec2(width, picture_height));
        let radius = CornerRadius::same(12);
        crate::app::big_screen::ring::mark(
            &response,
            crate::app::big_screen::ring::Mark::Ring {
                rect: frame,
                radius: 12.0,
            },
        );
        ui.painter()
            .rect_filled(frame, radius, kit::card_fill(visuals));
        let tint = if game.gone {
            egui::Color32::from_white_alpha(110)
        } else {
            egui::Color32::WHITE
        };
        match picture {
            Picture::Cover(cover) if cover.boxed => {
                egui::Image::new((cover.id(), cover.size))
                    .corner_radius(CornerRadius::same(6))
                    .tint(tint)
                    .paint_at(ui, kit::fit_inside(cover.size, frame.shrink(BOX_MARGIN)));
            }
            Picture::Cover(cover) => {
                egui::Image::new((cover.id(), frame.size()))
                    .uv(fill_uv(cover.uv, cover.size, frame.size()))
                    .corner_radius(radius)
                    .tint(tint)
                    .paint_at(ui, frame);
            }
            Picture::Icon(icon) => {
                let side = ICON_SIDE.min(frame.height() * 0.6);
                ui.painter().image(
                    *icon,
                    Rect::from_center_size(frame.center(), Vec2::splat(side)),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
            }
            Picture::Plain => {
                let lift = if game.console.is_some() {
                    PLAIN_LIFT
                } else {
                    0.0
                };
                ui.painter().text(
                    frame.center() - vec2(0.0, lift),
                    Align2::CENTER_CENTER,
                    icons::GAME_CONTROLLER,
                    FontId::proportional(PLAIN_ICON),
                    visuals.weak_text_color(),
                );
                if let Some(console) = game.console {
                    ui.painter().text(
                        frame.center() + vec2(0.0, PLAIN_ICON * 0.5 + PLAIN_GAP),
                        Align2::CENTER_CENTER,
                        console,
                        FontId::proportional(14.0),
                        visuals.weak_text_color(),
                    );
                }
            }
        }
        if response.is_pointer_button_down_on() || response.hovered() {
            ui.painter().rect_filled(
                frame,
                radius,
                visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.25),
            );
        }
        let strong = visuals.strong_text_color();
        let name = one_line(ui, &game.name, 16.0, strong, width);
        let top = frame.bottom() + 8.0;
        ui.painter().galley(pos2(rect.left(), top), name, strong);
        if game.gone {
            let weak = kit::value_color(visuals);
            let state = one_line(ui, &game.detail, 14.0, weak, width);
            ui.painter()
                .galley(pos2(rect.left(), top + line), state, weak);
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &game.name)
    });
    response
}

fn fill_uv(uv: Rect, image: Vec2, frame: Vec2) -> Rect {
    let image_shape = image.x / image.y;
    let frame_shape = frame.x / frame.y;
    let (keep_x, keep_y) = if image_shape > frame_shape {
        (frame_shape / image_shape, 1.0)
    } else {
        (1.0, image_shape / frame_shape)
    };
    let center = uv.center();
    Rect::from_center_size(center, vec2(uv.width() * keep_x, uv.height() * keep_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_picture_keeps_its_middle() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let uv = fill_uv(full, vec2(240.0, 160.0), vec2(100.0, 90.0));
        assert!((uv.center().x - 0.5).abs() < 1e-6);
        assert_eq!(uv.height(), 1.0);
        assert!((uv.width() - (100.0 / 90.0) / 1.5).abs() < 1e-6);
    }

    #[test]
    fn a_ds_top_screen_stays_on_the_top_half() {
        let top = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 0.5));
        let uv = fill_uv(top, vec2(256.0, 192.0), vec2(100.0, 90.0));
        assert!(uv.max.y <= 0.5 + 1e-6);
        assert!(uv.min.y >= 0.0);
    }
}
