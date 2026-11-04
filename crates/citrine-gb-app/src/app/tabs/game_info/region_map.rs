use super::parts::card;
use citrine_gb_gamedata::{Marker, RegionMap};
use egui::{Color32, Rect, RichText, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use std::collections::HashMap;

const MAX_SCALE: f32 = 3.0;
const MARKER_RADIUS: f32 = 7.0;
const PIN_HEIGHT: f32 = 13.0;
const SHARPEN: usize = 4;

type MapCache = HashMap<String, TextureHandle>;

fn map_texture(ctx: &egui::Context, key: &str, map: &RegionMap) -> TextureHandle {
    let id = egui::Id::new("game_info_maps");
    let cache_key = key.to_owned();
    if let Some(found) = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<MapCache>(id)
            .get(&cache_key)
            .cloned()
    }) {
        return found;
    }
    let (width, height) = (map.width * SHARPEN, map.height * SHARPEN);
    let pixels: Vec<Color32> = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width / SHARPEN, i / width / SHARPEN);
            let [r, g, b, a] = map.pixels[y * map.width + x];
            Color32::from_rgba_unmultiplied(r, g, b, a)
        })
        .collect();
    let image = egui::ColorImage::new([width, height], pixels);
    let texture = ctx.load_texture(
        format!("game_info_map_{cache_key}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<MapCache>(id)
            .insert(cache_key, texture.clone())
    });
    texture
}

fn map_size(width: f32, pixels_per_point: f32, map: &RegionMap) -> egui::Vec2 {
    let native = vec2(map.width as f32, map.height as f32);
    let fit = (width / native.x).min(MAX_SCALE);
    let size = native * fit * pixels_per_point;
    vec2(size.x.floor(), size.y.floor()) / pixels_per_point
}

pub(super) fn place_card(ui: &mut Ui, name: &str, line: &str, map: Option<(&str, &Marker)>) {
    card(ui, false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(name).size(19.0).strong()).truncate());
        if !line.is_empty() {
            ui.label(RichText::new(line).weak());
        }
        if let Some((key, marker)) = map {
            ui.add_space(6.0);
            region_map(ui, key, marker);
        }
    });
}

fn region_map(ui: &mut Ui, key: &str, marker: &Marker) -> Rect {
    let texture = map_texture(ui.ctx(), key, marker.map);
    let size = map_size(
        ui.available_width(),
        ui.ctx().pixels_per_point(),
        marker.map,
    );
    let (row, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), size.y), Sense::hover());
    let rect = Rect::from_center_size(row.center(), size);
    let painter = ui.painter_at(rect);
    let pin_painter = ui
        .painter()
        .with_clip_rect(rect.expand(PIN_HEIGHT + MARKER_RADIUS));
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let scale = rect.width() / marker.map.width as f32;
    let at = rect.min + vec2(f32::from(marker.x) + 0.5, f32::from(marker.y) + 0.5) * scale;
    paint_marker(&pin_painter, at);
    response.on_hover_text(marker.name);
    rect
}

fn paint_marker(painter: &egui::Painter, at: egui::Pos2) {
    let head = at - vec2(0.0, PIN_HEIGHT);
    let pin = |grow: f32, color: Color32| {
        let radius = MARKER_RADIUS + grow;
        painter.add(egui::Shape::convex_polygon(
            vec![
                head + vec2(-radius * 0.8, radius * 0.6),
                at + vec2(0.0, grow * 1.5),
                head + vec2(radius * 0.8, radius * 0.6),
            ],
            color,
            Stroke::NONE,
        ));
        painter.circle_filled(head, radius, color);
    };
    painter.circle_filled(at, 3.0, Color32::from_black_alpha(70));
    pin(2.5, Color32::from_gray(30));
    pin(1.5, Color32::WHITE);
    pin(0.0, Color32::from_rgb(0xe0, 0x30, 0x40));
    painter.circle_filled(head, MARKER_RADIUS * 0.4, Color32::WHITE);
}
