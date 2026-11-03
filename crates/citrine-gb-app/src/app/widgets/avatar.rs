use crate::online::Seen;
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

const SEGMENTS: usize = 64;

pub const DOT: Color32 = Color32::from_rgb(0xE6, 0x45, 0x53);
pub const ONLINE: Color32 = Color32::from_rgb(0x40, 0xC0, 0x57);
pub const AWAY: Color32 = Color32::from_rgb(0xF2, 0xB1, 0x2E);
pub const OFFLINE: Color32 = Color32::from_gray(140);

pub fn avatar(ui: &mut Ui, name: &str, seed: u64, size: f32, seen: Option<Seen>) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        paint(ui, rect, name, seed, seen);
    }
    response
}

pub fn paint(ui: &Ui, rect: Rect, name: &str, seed: u64, seen: Option<Seen>) {
    let painter = ui.painter_at(rect.expand(2.0));
    let (from, to) = colors(seed);
    let feather = 1.0 / ui.ctx().pixels_per_point();
    painter.add(gradient_disc(
        rect.center(),
        rect.width() / 2.0,
        from,
        to,
        feather,
    ));

    let size = rect.width();
    let initials = citrine_gb_net::initials(name);
    let font = FontId::proportional(
        size * if initials.chars().count() > 1 {
            0.38
        } else {
            0.46
        },
    );
    let ink = crate::app::ui_state::ui_theme::ink_on(lerp(from, to, 0.5));
    let center = rect.center();
    if ink == Color32::WHITE {
        painter.text(
            center + vec2(0.0, size * 0.025),
            egui::Align2::CENTER_CENTER,
            &initials,
            font.clone(),
            Color32::from_black_alpha(70),
        );
    }
    painter.text(center, egui::Align2::CENTER_CENTER, &initials, font, ink);

    let color = match seen {
        Some(Seen::Online) => ONLINE,
        Some(Seen::Away) => AWAY,
        Some(Seen::Offline { .. }) => OFFLINE,
        None => return,
    };
    let radius = (size * 0.15).max(4.0);
    let dot = rect.right_bottom() - Vec2::splat(radius * 0.9);
    painter.circle_filled(dot, radius + 1.5, ui.visuals().panel_fill);
    painter.circle_filled(dot, radius, color);
}

fn gradient_disc(center: Pos2, radius: f32, from: Color32, to: Color32, feather: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let axis = vec2(1.0, 1.0).normalized();
    let color_at = |point: Pos2| {
        let t = ((point - center).dot(axis) / radius * 0.5 + 0.5).clamp(0.0, 1.0);
        lerp(from, to, t)
    };
    let vertex = |pos: Pos2, color: Color32| Vertex {
        pos,
        uv: WHITE_UV,
        color,
    };
    mesh.vertices.push(vertex(center, color_at(center)));
    let inner = (radius - feather / 2.0).max(0.0);
    let outer = radius + feather / 2.0;
    for i in 0..SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let dir = vec2(angle.cos(), angle.sin());
        let edge = center + dir * inner;
        let color = color_at(edge);
        mesh.vertices.push(vertex(edge, color));
        mesh.vertices
            .push(vertex(center + dir * outer, Color32::TRANSPARENT));
    }
    for i in 0..SEGMENTS as u32 {
        let next = (i + 1) % SEGMENTS as u32;
        let (a, a_out) = (1 + i * 2, 2 + i * 2);
        let (b, b_out) = (1 + next * 2, 2 + next * 2);
        mesh.add_triangle(0, a, b);
        mesh.add_triangle(a, a_out, b_out);
        mesh.add_triangle(a, b_out, b);
    }
    mesh
}

fn lerp(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}

pub fn colors(seed: u64) -> (Color32, Color32) {
    let (first, second) = citrine_gb_net::avatar_hues(seed);
    (hsl(first, 0.72, 0.58), hsl(second, 0.70, 0.46))
}

fn hsl(hue: u16, saturation: f32, lightness: f32) -> Color32 {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = f32::from(hue % 360) / 60.0;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

pub fn status_dot(ui: &Ui, rect: Rect) {
    dot_in(ui, rect, DOT);
}

pub fn dot_in(ui: &Ui, rect: Rect, color: Color32) {
    let radius = 4.0;
    let center = pos2(rect.right() - radius, rect.top() + radius);
    ui.painter()
        .circle_filled(center, radius + 1.5, ui.visuals().panel_fill);
    ui.painter().circle_filled(center, radius, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_matches_css() {
        assert_eq!(hsl(0, 1.0, 0.5), Color32::from_rgb(255, 0, 0));
        assert_eq!(hsl(120, 1.0, 0.25), Color32::from_rgb(0, 128, 0));
        assert_eq!(hsl(210, 0.72, 0.58), Color32::from_rgb(71, 148, 225));
    }

    #[test]
    fn the_same_person_gets_the_same_colours() {
        assert_eq!(colors(0x1234), colors(0x1234));
        assert_ne!(colors(0x1234), colors(0x1235));
    }

    #[test]
    fn the_disc_fades_out_at_its_edge() {
        let mesh = gradient_disc(pos2(10.0, 10.0), 10.0, Color32::RED, Color32::BLUE, 1.0);
        let outer: Vec<_> = mesh.vertices.iter().skip(2).step_by(2).collect();
        assert_eq!(outer.len(), SEGMENTS);
        assert!(outer.iter().all(|v| v.color == Color32::TRANSPARENT));
        assert!(mesh.vertices[1].color.a() == 255);
    }
}
