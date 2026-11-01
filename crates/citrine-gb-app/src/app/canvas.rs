#[cfg(any(test, target_arch = "wasm32"))]
use egui::Vec2;
use egui::{Context, RawInput};

#[cfg(any(test, target_arch = "wasm32"))]
fn backing_size(css: Vec2, pixel_ratio: f32, current: [u32; 2]) -> Option<[u32; 2]> {
    let wanted = [
        (css.x * pixel_ratio).round() as u32,
        (css.y * pixel_ratio).round() as u32,
    ];
    let off = |have: u32, want: u32| have.abs_diff(want) as f32 > (want as f32 * 0.02).max(2.0);
    (off(current[0], wanted[0]) || off(current[1], wanted[1])).then_some(wanted)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn screen_in_points(backing: [u32; 2], pixels_per_point: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        Vec2::new(backing[0] as f32, backing[1] as f32) / pixels_per_point,
    )
}

#[cfg(target_arch = "wasm32")]
pub fn fit(ctx: &Context, raw_input: &mut RawInput) {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(canvas) = window
        .document()
        .and_then(|document| document.get_element_by_id("app_canvas"))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return;
    };
    let css = Vec2::new(canvas.client_width() as f32, canvas.client_height() as f32);
    let pixel_ratio = window.device_pixel_ratio() as f32;
    if css.min_elem() <= 0.0 || !pixel_ratio.is_finite() || pixel_ratio <= 0.0 {
        return;
    }
    let Some(backing) = backing_size(css, pixel_ratio, [canvas.width(), canvas.height()]) else {
        return;
    };
    canvas.set_width(backing[0]);
    canvas.set_height(backing[1]);
    raw_input.screen_rect = Some(screen_in_points(backing, ctx.zoom_factor() * pixel_ratio));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn fit(_ctx: &Context, _raw_input: &mut RawInput) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canvas_drawn_at_css_size_gets_its_device_pixels() {
        let phone = Vec2::new(412.0, 915.0);
        assert_eq!(backing_size(phone, 2.625, [412, 915]), Some([1082, 2402]));
        assert_eq!(backing_size(phone, 3.5, [412, 915]), Some([1442, 3203]));
        assert_eq!(
            backing_size(phone, 1.0, [1082, 2402]),
            Some([412, 915]),
            "a backing store finer than the pixel ratio is shrunk too"
        );
    }

    #[test]
    fn a_canvas_already_at_device_pixels_is_left_alone() {
        let phone = Vec2::new(412.0, 915.0);
        assert_eq!(backing_size(phone, 1.0, [412, 915]), None);
        assert_eq!(backing_size(phone, 2.625, [1082, 2402]), None);
        assert_eq!(
            backing_size(phone, 2.625, [1081, 2401]),
            None,
            "the browser's own rounding of device pixels stays"
        );
        assert_eq!(
            backing_size(Vec2::new(1280.0, 800.0), 2.0, [2560, 1600]),
            None
        );
    }

    #[test]
    fn the_screen_in_points_follows_the_new_backing_store() {
        let rect = screen_in_points([1082, 2402], 2.625);
        assert!((rect.width() - 412.2).abs() < 0.1);
        assert!((rect.height() - 915.0).abs() < 0.1);
    }
}
