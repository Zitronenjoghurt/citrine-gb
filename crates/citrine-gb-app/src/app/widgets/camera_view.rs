use crate::emulator::gimmicks::camera::{CameraView, Feed};
use egui::{
    Color32, ColorImage, CornerRadius, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};

pub const WAITING: &str = "Waiting for the camera";
pub const CENTRE: &str = "Centre the frame";
pub const PHONE_HINT: &str = "Pinch to zoom.";
#[cfg(target_os = "macos")]
pub const DESKTOP_HINT: &str = "Pinch, or hold Cmd and scroll, to zoom.";
#[cfg(not(target_os = "macos"))]
pub const DESKTOP_HINT: &str = "Pinch, or hold Ctrl and scroll, to zoom.";

const DIM: Color32 = Color32::from_black_alpha(140);
const HANDLE: f32 = 10.0;
const TOUCH_HANDLE: f32 = 14.0;

#[derive(Clone)]
struct Shown {
    count: u64,
    texture: TextureHandle,
}

#[derive(Clone, Copy)]
enum Grip {
    Move,
    Corner,
}

pub fn show(ui: &mut Ui, feed: Option<(&Feed, u64)>, view: &mut CameraView) -> bool {
    let Some((feed, count)) = feed.filter(|(feed, _)| feed.width > 0 && feed.height > 0) else {
        ui.label(RichText::new(WAITING).weak());
        return false;
    };
    let id = ui.id().with("camera_view");
    let texture = texture(ui, id, feed, count);
    let width = ui.available_width();
    let size = vec2(width, width * feed.height as f32 / feed.width as f32);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let (w, h) = (feed.width, feed.height);
    let mut changed = false;

    let frame = frame_rect(rect, view.shown(w, h, feed.mirror));
    let touch = crate::app::mobile::touch_square(ui) > 0.0;
    let handle = if touch { TOUCH_HANDLE } else { HANDLE };
    let grab = frame.expand(handle);
    let response = ui.interact(grab, id.with("frame"), Sense::drag());
    let mut settled = response.drag_stopped();
    if response.drag_started()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let grip = if corner(frame, pointer, handle).is_some() {
            Grip::Corner
        } else {
            Grip::Move
        };
        ui.ctx().data_mut(|d| d.insert_temp(id.with("grip"), grip));
    }
    if response.dragged() {
        let grip = ui
            .ctx()
            .data(|d| d.get_temp::<Grip>(id.with("grip")))
            .unwrap_or(Grip::Move);
        match grip {
            Grip::Move => {
                let delta = response.drag_delta();
                view.x += delta.x / rect.width();
                view.y += delta.y / rect.height();
            }
            Grip::Corner => {
                if let Some(pointer) = response.interact_pointer_pos() {
                    let reach = (pointer - frame.center()).abs();
                    let full = view_size(rect, *view, w, h, feed.mirror);
                    let wanted = (2.0 * reach.x / full.x).max(2.0 * reach.y / full.y);
                    if wanted > 0.0 {
                        view.zoom = 1.0 / wanted;
                    }
                }
            }
        }
        changed = true;
    }
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 {
            view.zoom *= zoom;
            changed = true;
            settled = true;
        }
    }
    if changed {
        *view = view.clamped(w, h, feed.mirror);
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if response.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }

    if ui.is_rect_visible(rect) {
        let uv = if feed.mirror {
            Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0))
        } else {
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0))
        };
        let painter = ui.painter_at(rect);
        painter.image(texture.id(), rect, uv, Color32::WHITE);
        let frame = frame_rect(rect, view.shown(w, h, feed.mirror));
        for dim in [
            Rect::from_min_max(rect.min, pos2(rect.max.x, frame.min.y)),
            Rect::from_min_max(pos2(rect.min.x, frame.max.y), rect.max),
            Rect::from_min_max(
                pos2(rect.min.x, frame.min.y),
                pos2(frame.min.x, frame.max.y),
            ),
            Rect::from_min_max(
                pos2(frame.max.x, frame.min.y),
                pos2(rect.max.x, frame.max.y),
            ),
        ] {
            painter.rect_filled(dim, CornerRadius::ZERO, DIM);
        }
        let accent = ui.visuals().selection.bg_fill;
        let outline = Stroke::new(1.0_f32, Color32::from_black_alpha(110));
        painter.rect_stroke(
            frame,
            CornerRadius::ZERO,
            Stroke::new(4.0_f32, outline.color),
            StrokeKind::Middle,
        );
        painter.rect_stroke(
            frame,
            CornerRadius::ZERO,
            Stroke::new(2.0_f32, accent),
            StrokeKind::Middle,
        );
        for point in [
            frame.left_top(),
            frame.right_top(),
            frame.left_bottom(),
            frame.right_bottom(),
        ] {
            let square = Rect::from_center_size(point, Vec2::splat(handle));
            ui.painter().rect_filled(square, CornerRadius::ZERO, accent);
            ui.painter()
                .rect_stroke(square, CornerRadius::ZERO, outline, StrokeKind::Inside);
        }
    }
    settled
}

fn texture(ui: &Ui, id: Id, feed: &Feed, count: u64) -> TextureHandle {
    let key = id.with("texture");
    let image = || ColorImage::from_rgba_unmultiplied([feed.width, feed.height], &feed.rgba);
    let mut shown = ui.ctx().data(|d| d.get_temp::<Shown>(key));
    match &mut shown {
        Some(shown) if shown.count == count => {}
        Some(shown) if shown.texture.size() == [feed.width, feed.height] => {
            shown.texture.set(image(), TextureOptions::LINEAR);
            shown.count = count;
        }
        _ => {
            shown = Some(Shown {
                count,
                texture: ui
                    .ctx()
                    .load_texture("camera feed", image(), TextureOptions::LINEAR),
            });
        }
    }
    let shown = shown.expect("set above");
    ui.ctx().data_mut(|d| d.insert_temp(key, shown.clone()));
    shown.texture
}

fn frame_rect(rect: Rect, [left, top, w, h]: [f32; 4]) -> Rect {
    Rect::from_min_size(
        pos2(
            rect.left() + left * rect.width(),
            rect.top() + top * rect.height(),
        ),
        vec2(w * rect.width(), h * rect.height()),
    )
}

fn view_size(rect: Rect, view: CameraView, width: usize, height: usize, mirror: bool) -> Vec2 {
    let whole = CameraView { zoom: 1.0, ..view };
    frame_rect(rect, whole.shown(width, height, mirror)).size()
}

fn corner(frame: Rect, pointer: Pos2, handle: f32) -> Option<Pos2> {
    [
        frame.left_top(),
        frame.right_top(),
        frame.left_bottom(),
        frame.right_bottom(),
    ]
    .into_iter()
    .find(|point| (*point - pointer).abs().max_elem() <= handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(feed: Option<&Feed>, view: &mut CameraView) -> egui::FullOutput {
        let ctx = egui::Context::default();
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, feed.map(|feed| (feed, 1)), view);
            });
        })
    }

    fn says(output: &egui::FullOutput, wanted: &str) -> bool {
        output.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == wanted)
        })
    }

    #[test]
    fn the_preview_waits_for_the_first_frame() {
        let mut view = CameraView::default();
        assert!(says(&run(None, &mut view), WAITING));
        let feed = Feed::from_luma(160, 120, vec![128; 160 * 120], true);
        assert!(!says(&run(Some(&feed), &mut view), WAITING));
        assert_eq!(view, CameraView::default());
    }

    #[test]
    fn the_frame_maps_onto_the_preview() {
        let rect = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 150.0));
        let frame = frame_rect(rect, [0.25, 0.5, 0.5, 0.25]);
        assert_eq!(
            frame,
            Rect::from_min_size(pos2(60.0, 95.0), vec2(100.0, 37.5))
        );
    }

    #[test]
    fn corners_are_found_near_the_pointer() {
        let frame = Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 70.0));
        assert_eq!(corner(frame, pos2(78.0, 3.0), 10.0), Some(pos2(80.0, 0.0)));
        assert_eq!(corner(frame, pos2(40.0, 35.0), 10.0), None);
    }
}
