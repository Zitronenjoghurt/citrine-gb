use citrine_gb::gb::cartridge::{CAMERA_HEIGHT, CAMERA_PIXELS, CAMERA_WIDTH};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CameraView {
    pub zoom: f32,
    pub x: f32,
    pub y: f32,
}

impl Default for CameraView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            x: 0.5,
            y: 0.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

impl CameraView {
    pub fn max_zoom(width: usize, height: usize) -> f32 {
        let (crop_w, crop_h) = cover(width, height);
        (crop_w as f32 / CAMERA_WIDTH as f32)
            .min(crop_h as f32 / CAMERA_HEIGHT as f32)
            .max(1.0)
    }

    pub fn crop(self, width: usize, height: usize, mirror: bool) -> Crop {
        let (full_w, full_h) = cover(width, height);
        let zoom = if self.zoom.is_finite() {
            self.zoom.clamp(1.0, Self::max_zoom(width, height))
        } else {
            1.0
        };
        let crop_w = ((full_w as f32 / zoom).round() as usize).clamp(1, width.max(1));
        let crop_h = ((full_h as f32 / zoom).round() as usize).clamp(1, height.max(1));
        let x = if mirror { 1.0 - self.x } else { self.x };
        let place = |share: f32, size: usize, crop: usize| {
            let share = if share.is_finite() { share } else { 0.5 };
            let start = (share * size as f32 - crop as f32 / 2.0).floor();
            (start.max(0.0) as usize).min(size - crop)
        };
        Crop {
            left: place(x, width, crop_w),
            top: place(self.y, height, crop_h),
            width: crop_w,
            height: crop_h,
        }
    }

    pub fn shown(self, width: usize, height: usize, mirror: bool) -> [f32; 4] {
        let crop = self.crop(width, height, mirror);
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let left = if mirror {
            width - crop.left - crop.width
        } else {
            crop.left
        };
        [
            left as f32 / w,
            crop.top as f32 / h,
            crop.width as f32 / w,
            crop.height as f32 / h,
        ]
    }

    pub fn clamped(self, width: usize, height: usize, mirror: bool) -> Self {
        let [left, top, w, h] = self.shown(width, height, mirror);
        let zoom = if self.zoom.is_finite() {
            self.zoom.clamp(1.0, Self::max_zoom(width, height))
        } else {
            1.0
        };
        Self {
            zoom,
            x: left + w / 2.0,
            y: top + h / 2.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Feed {
    pub width: usize,
    pub height: usize,
    pub luma: Vec<u8>,
    pub rgba: Vec<u8>,
    pub mirror: bool,
}

impl Feed {
    #[cfg(any(test, target_os = "android"))]
    pub fn from_luma(width: usize, height: usize, luma: Vec<u8>, mirror: bool) -> Self {
        let rgba = luma.iter().flat_map(|y| [*y, *y, *y, 255]).collect();
        Self {
            width,
            height,
            luma,
            rgba,
            mirror,
        }
    }

    pub fn frame(&self, view: CameraView) -> Option<Vec<u8>> {
        from_luma_view(self.width, self.height, &self.luma, self.mirror, view)
    }
}

pub fn from_luma(width: usize, height: usize, luma: &[u8], mirror: bool) -> Option<Vec<u8>> {
    from_luma_view(width, height, luma, mirror, CameraView::default())
}

pub fn from_luma_view(
    width: usize,
    height: usize,
    luma: &[u8],
    mirror: bool,
    view: CameraView,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || luma.len() < width * height {
        return None;
    }
    let Crop {
        left,
        top,
        width: crop_w,
        height: crop_h,
    } = view.crop(width, height, mirror);
    let mut out = Vec::with_capacity(CAMERA_PIXELS);
    for y in 0..CAMERA_HEIGHT {
        let y0 = top + y * crop_h / CAMERA_HEIGHT;
        let y1 = (top + (y + 1) * crop_h / CAMERA_HEIGHT).max(y0 + 1);
        for x in 0..CAMERA_WIDTH {
            let column = if mirror { CAMERA_WIDTH - 1 - x } else { x };
            let x0 = left + column * crop_w / CAMERA_WIDTH;
            let x1 = (left + (column + 1) * crop_w / CAMERA_WIDTH).max(x0 + 1);
            let mut sum = 0u32;
            for row in y0..y1 {
                for value in &luma[row * width + x0..row * width + x1] {
                    sum += u32::from(*value);
                }
            }
            out.push((sum / ((y1 - y0) * (x1 - x0)) as u32) as u8);
        }
    }
    Some(out)
}

pub fn from_rgba(width: usize, height: usize, rgba: &[u8], mirror: bool) -> Option<Vec<u8>> {
    if rgba.len() < width * height * 4 {
        return None;
    }
    let luma: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29) >> 8) as u8)
        .collect();
    from_luma(width, height, &luma, mirror)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn from_picture(bytes: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = picture.dimensions();
    from_rgba(width as usize, height as usize, picture.as_raw(), false)
}

#[cfg(target_arch = "wasm32")]
pub fn from_picture(bytes: &[u8]) -> Option<Vec<u8>> {
    let frame = crate::media::decode_png(bytes)?;
    from_rgba(
        frame.width as usize,
        frame.height as usize,
        &frame.rgba,
        false,
    )
}

fn cover(width: usize, height: usize) -> (usize, usize) {
    if width * CAMERA_HEIGHT > height * CAMERA_WIDTH {
        (height * CAMERA_WIDTH / CAMERA_HEIGHT, height)
    } else {
        (width, width * CAMERA_HEIGHT / CAMERA_WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_view_is_the_centred_full_crop() {
        let crop = CameraView::default().crop(640, 480, false);
        assert_eq!(
            crop,
            Crop {
                left: 46,
                top: 0,
                width: 548,
                height: 480
            }
        );
        assert_eq!(CameraView::default().crop(640, 480, true), crop);
    }

    #[test]
    fn zoom_stops_at_the_game_boy_size() {
        let view = CameraView {
            zoom: 50.0,
            ..CameraView::default()
        };
        let crop = view.crop(640, 480, false);
        assert_eq!((crop.width, crop.height), (CAMERA_WIDTH, CAMERA_HEIGHT));
        assert_eq!(CameraView::max_zoom(100, 80), 1.0);
    }

    #[test]
    fn the_frame_stays_inside_the_picture() {
        let view = CameraView {
            zoom: 2.0,
            x: -3.0,
            y: 9.0,
        };
        let crop = view.crop(640, 480, false);
        assert_eq!((crop.left, crop.top + crop.height), (0, 480));
        let kept = view.clamped(640, 480, false);
        assert_eq!(kept.crop(640, 480, false), crop);
        assert!(kept.x > 0.0 && kept.y < 1.0);
    }

    #[test]
    fn a_mirrored_feed_moves_the_crop_the_way_it_is_shown() {
        let view = CameraView {
            zoom: 2.0,
            x: 0.0,
            y: 0.5,
        };
        let crop = view.crop(640, 480, true);
        assert_eq!(crop.left + crop.width, 640);
        let [left, _, _, _] = view.shown(640, 480, true);
        assert_eq!(left, 0.0);
    }

    #[test]
    fn a_zoomed_view_feeds_only_its_part() {
        let (width, height) = (256, 112);
        let luma: Vec<u8> = (0..width * height)
            .map(|i| if i % width < width / 2 { 0 } else { 200 })
            .collect();
        let left = CameraView {
            zoom: 2.0,
            x: 0.0,
            y: 0.5,
        };
        let frame = from_luma_view(width, height, &luma, false, left).expect("frame");
        assert!(frame.iter().all(|value| *value == 0));
        let whole = from_luma(width, height, &luma, false).expect("frame");
        assert!(whole.contains(&0) && whole.contains(&200));
    }

    #[test]
    fn a_grey_feed_has_a_grey_preview() {
        let feed = Feed::from_luma(2, 1, vec![10, 250], true);
        assert_eq!(feed.rgba, vec![10, 10, 10, 255, 250, 250, 250, 255]);
        assert_eq!(
            feed.frame(CameraView::default()).map(|f| f.len()),
            Some(CAMERA_PIXELS)
        );
    }
}
