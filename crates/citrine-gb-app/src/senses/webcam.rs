use crate::emulator::gimmicks::camera::Feed;
use nokhwa::Camera;
use nokhwa::pixel_format::LumaFormat;
use nokhwa::utils::{CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Shared = Arc<Mutex<Option<Result<Feed, String>>>>;

pub struct Webcam {
    latest: Shared,
    stop: Arc<AtomicBool>,
}

impl Webcam {
    pub fn open() -> Result<Self, String> {
        let latest: Shared = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, stopped) = (latest.clone(), stop.clone());
        std::thread::Builder::new()
            .name("webcam".into())
            .spawn(move || {
                if let Err(err) = capture(&shared, &stopped) {
                    log::warn!("webcam stopped: {err}");
                    *shared.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(err));
                }
            })
            .map_err(|err| err.to_string())?;
        Ok(Self { latest, stop })
    }

    pub fn take_frame(&mut self) -> Option<Result<Feed, String>> {
        self.latest.lock().ok()?.take()
    }
}

impl Drop for Webcam {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

const UNREADABLE_FRAMES: usize = 30;
const MIN_WIDTH: u32 = 128;
const MIN_HEIGHT: u32 = 112;
const SMOOTH_FPS: u32 = 15;

fn capture(shared: &Shared, stop: &AtomicBool) -> Result<(), String> {
    allowed()?;
    let mut camera = open_any()?;
    let mut modes = camera
        .compatible_camera_formats()
        .map(|formats| ranked(&formats, cfg!(target_os = "macos")))
        .unwrap_or_default();
    if modes.is_empty() {
        modes.push(camera.camera_format());
    }
    let mut reason = String::new();
    for mode in modes {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut delivered = false;
        match stream(&mut camera, mode, shared, stop, &mut delivered) {
            Ok(()) => return Ok(()),
            Err(err) if delivered => return Err(err),
            Err(err) => {
                log::info!("webcam mode {mode} failed: {err}");
                reason = err;
            }
        }
    }
    Err(reason)
}

fn open_any() -> Result<Camera, String> {
    let mut reason = String::new();
    for request in [
        RequestedFormatType::AbsoluteHighestFrameRate,
        RequestedFormatType::AbsoluteHighestResolution,
        RequestedFormatType::None,
    ] {
        match Camera::new(CameraIndex::Index(0), any_of(request)) {
            Ok(camera) => return Ok(camera),
            Err(err) => reason = err.to_string(),
        }
    }
    Err(reason)
}

fn any_of(request: RequestedFormatType) -> RequestedFormat<'static> {
    RequestedFormat::new::<LumaFormat>(request)
}

fn stream(
    camera: &mut Camera,
    mode: CameraFormat,
    shared: &Shared,
    stop: &AtomicBool,
    delivered: &mut bool,
) -> Result<(), String> {
    camera
        .set_camera_requset(any_of(RequestedFormatType::Exact(mode)))
        .map_err(|err| err.to_string())?;
    camera.open_stream().map_err(|err| err.to_string())?;
    let result = frames(camera, shared, stop, delivered);
    let _ = camera.stop_stream();
    result
}

fn frames(
    camera: &mut Camera,
    shared: &Shared,
    stop: &AtomicBool,
    delivered: &mut bool,
) -> Result<(), String> {
    let mut misses = 0;
    while !stop.load(Ordering::Relaxed) {
        let buffer = camera.frame().map_err(|err| err.to_string())?;
        let size = buffer.resolution();
        let (width, height) = (size.width() as usize, size.height() as usize);
        let format = buffer.source_frame_format();
        let Some(frame) = decode(format, width, height, buffer.buffer()) else {
            misses += 1;
            if misses == UNREADABLE_FRAMES && !*delivered {
                return Err(format!(
                    "the camera's {format} {size} pictures could not be read"
                ));
            }
            continue;
        };
        misses = 0;
        *delivered = true;
        *shared.lock().map_err(|err| err.to_string())? = Some(Ok(frame));
    }
    Ok(())
}

fn ranked(formats: &[CameraFormat], last_per_size: bool) -> Vec<CameraFormat> {
    let usable = formats.iter().enumerate().filter(|(index, format)| {
        format.frame_rate() > 0
            && !(last_per_size
                && formats[index + 1..]
                    .iter()
                    .any(|later| later.resolution() == format.resolution()))
    });
    let mut best: Vec<CameraFormat> = Vec::new();
    for (_, format) in usable {
        let same = |kept: &CameraFormat| {
            kept.format() == format.format() && kept.resolution() == format.resolution()
        };
        match best.iter_mut().find(|kept| same(kept)) {
            Some(kept) if kept.frame_rate() < format.frame_rate() => *kept = *format,
            Some(_) => {}
            None => best.push(*format),
        }
    }
    best.sort_by_key(|format| {
        let size = format.resolution();
        let area = u64::from(size.width()) * u64::from(size.height());
        let small = size.width() < MIN_WIDTH || size.height() < MIN_HEIGHT;
        (
            small,
            format.frame_rate() < SMOOTH_FPS,
            format.format() == FrameFormat::MJPEG,
            if small { u64::MAX - area } else { area },
            u32::MAX - format.frame_rate(),
        )
    });
    best
}

fn decode(format: FrameFormat, width: usize, height: usize, data: &[u8]) -> Option<Feed> {
    let pixels = width * height;
    let mut luma = Vec::with_capacity(pixels);
    let mut rgba = Vec::with_capacity(pixels * 4);
    let mut put = |y: u8, colour: [u8; 4]| {
        luma.push(y);
        rgba.extend(colour);
    };
    match format {
        FrameFormat::YUYV => {
            for row in rows(data, width * 2, height, height)? {
                for pair in row.chunks(4) {
                    let u = pair.get(1).copied().unwrap_or(128);
                    let v = pair.get(3).copied().unwrap_or(128);
                    for y in [pair.first(), pair.get(2)].into_iter().flatten() {
                        put(*y, yuv(*y, u, v));
                    }
                }
            }
        }
        FrameFormat::NV12 => {
            let stored = height + height.div_ceil(2);
            let all: Vec<&[u8]> = rows(data, width, stored, stored)?.collect();
            for (y_row, row) in all[..height].iter().enumerate() {
                let chroma = all[height + y_row / 2];
                for (x, y) in row.iter().enumerate() {
                    let pair = x / 2 * 2;
                    let u = chroma.get(pair).copied().unwrap_or(128);
                    let v = chroma.get(pair + 1).copied().unwrap_or(128);
                    put(*y, yuv(*y, u, v));
                }
            }
        }
        FrameFormat::GRAY => {
            for y in rows(data, width, height, height)?.flatten() {
                put(*y, [*y, *y, *y, 255]);
            }
        }
        FrameFormat::RAWRGB | FrameFormat::RAWBGR => {
            let bgr = format == FrameFormat::RAWBGR;
            for row in rows(data, width * 3, height, height)? {
                for p in row.as_chunks::<3>().0 {
                    let [r, g, b] = if bgr { [p[2], p[1], p[0]] } else { *p };
                    put(weigh(r, g, b), [r, g, b, 255]);
                }
            }
        }
        FrameFormat::MJPEG => {
            let picture = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
                .ok()?
                .into_rgb8();
            if picture.width() as usize != width || picture.height() as usize != height {
                return None;
            }
            for p in picture.pixels() {
                let [r, g, b] = p.0;
                put(weigh(r, g, b), [r, g, b, 255]);
            }
        }
    }
    (luma.len() == pixels).then_some(Feed {
        width,
        height,
        luma,
        rgba,
        mirror: true,
    })
}

fn weigh(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 77 + u32::from(g) * 150 + u32::from(b) * 29) >> 8) as u8
}

fn yuv(y: u8, u: u8, v: u8) -> [u8; 4] {
    let (y, u, v) = (f32::from(y), f32::from(u) - 128.0, f32::from(v) - 128.0);
    let channel = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    [
        channel(y + 1.402 * v),
        channel(y - 0.344_136 * u - 0.714_136 * v),
        channel(y + 1.772 * u),
        255,
    ]
}

fn rows(
    data: &[u8],
    row_bytes: usize,
    height: usize,
    stored_rows: usize,
) -> Option<impl Iterator<Item = &[u8]>> {
    if row_bytes == 0 || height == 0 || data.len() < row_bytes * stored_rows {
        return None;
    }
    let even = data.len() / stored_rows;
    let stride = if data.len().is_multiple_of(stored_rows) {
        even
    } else {
        row_bytes
    };
    Some(
        data.chunks(stride)
            .take(height)
            .map(move |row| &row[..row_bytes]),
    )
}

#[cfg(target_os = "macos")]
fn allowed() -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    nokhwa::nokhwa_initialize(move |granted| {
        let _ = tx.send(granted);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(true) => Ok(()),
        _ => Err("Citrine may not use the camera. Allow it in System Settings, Privacy and Security, Camera.".into()),
    }
}

#[cfg(not(target_os = "macos"))]
fn allowed() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nokhwa::utils::Resolution;

    const WIDTH: usize = 6;
    const HEIGHT: usize = 4;

    fn picture() -> Vec<u8> {
        (0..WIDTH * HEIGHT).map(|i| (i * 10 + 3) as u8).collect()
    }

    fn yuyv(luma: &[u8], width: usize, stride: usize) -> Vec<u8> {
        luma.chunks(width)
            .flat_map(|row| {
                let mut bytes: Vec<u8> = row.iter().flat_map(|y| [*y, 128]).collect();
                bytes.resize(stride, 0);
                bytes
            })
            .collect()
    }

    fn nv12(luma: &[u8], width: usize, stride: usize) -> Vec<u8> {
        let mut bytes: Vec<u8> = luma
            .chunks(width)
            .flat_map(|row| {
                let mut row = row.to_vec();
                row.resize(stride, 0);
                row
            })
            .collect();
        bytes.extend(std::iter::repeat_n(
            128,
            stride * luma.len().div_ceil(width).div_ceil(2),
        ));
        bytes
    }

    fn rgb(luma: &[u8]) -> Vec<u8> {
        luma.iter().flat_map(|y| [*y, *y, *y]).collect()
    }

    fn mode(width: u32, height: u32, format: FrameFormat, fps: u32) -> CameraFormat {
        CameraFormat::new(Resolution::new(width, height), format, fps)
    }

    fn macbook_air() -> Vec<CameraFormat> {
        let sizes = [
            (640, 480),
            (1280, 720),
            (1760, 1328),
            (1328, 1760),
            (1552, 1552),
            (1920, 1080),
            (1080, 1920),
        ];
        sizes
            .into_iter()
            .flat_map(|(width, height)| {
                [15, 30, 15, 30].map(|fps| mode(width, height, FrameFormat::YUYV, fps))
            })
            .collect()
    }

    #[test]
    fn every_format_gives_the_same_luma() {
        let luma = picture();
        let expected = Some(luma.clone());
        let bgr = rgb(&luma);
        assert_eq!(
            luma_of(FrameFormat::YUYV, &yuyv(&luma, WIDTH, WIDTH * 2)),
            expected
        );
        assert_eq!(
            luma_of(FrameFormat::NV12, &nv12(&luma, WIDTH, WIDTH)),
            expected
        );
        assert_eq!(luma_of(FrameFormat::GRAY, &luma), expected);
        assert_eq!(luma_of(FrameFormat::RAWRGB, &rgb(&luma)), expected);
        assert_eq!(luma_of(FrameFormat::RAWBGR, &bgr), expected);
    }

    #[test]
    fn mjpeg_gives_the_same_luma() {
        let luma = picture();
        let gray = image::GrayImage::from_raw(WIDTH as u32, HEIGHT as u32, luma.clone()).unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode_image(&gray)
            .unwrap();
        let decoded = luma_of(FrameFormat::MJPEG, &jpeg).expect("decoded");
        for (got, want) in decoded.iter().zip(&luma) {
            assert!(got.abs_diff(*want) <= 2, "{got} vs {want}");
        }
    }

    #[test]
    fn padded_rows_are_skipped() {
        let luma = picture();
        let expected = Some(luma.clone());
        assert_eq!(
            luma_of(FrameFormat::YUYV, &yuyv(&luma, WIDTH, 16)),
            expected
        );
        assert_eq!(luma_of(FrameFormat::NV12, &nv12(&luma, WIDTH, 8)), expected);
    }

    #[test]
    fn short_buffers_give_nothing() {
        let luma = picture();
        assert_eq!(luma_of(FrameFormat::GRAY, &luma[1..]), None);
        assert_eq!(luma_of(FrameFormat::YUYV, &luma), None);
        assert_eq!(luma_of(FrameFormat::MJPEG, &luma), None);
    }

    #[test]
    fn colours_weigh_like_bt601() {
        let red = luma(FrameFormat::RAWRGB, 1, 1, &[255, 0, 0]);
        let blue = luma(FrameFormat::RAWBGR, 1, 1, &[255, 0, 0]);
        assert_eq!(red, Some(vec![76]));
        assert_eq!(blue, Some(vec![28]));
    }

    #[test]
    fn the_old_request_fails_on_a_macbook_air() {
        let old = any_of(RequestedFormatType::Closest(mode(
            320,
            240,
            FrameFormat::YUYV,
            30,
        )));
        assert_eq!(old.fulfill(&macbook_air()), None);
    }

    #[test]
    fn a_macbook_air_gets_its_smallest_mode_at_full_speed() {
        let modes = ranked(&macbook_air(), true);
        assert_eq!(modes[0], mode(640, 480, FrameFormat::YUYV, 30));
        assert_eq!(modes.len(), 7);
        assert!(modes.iter().all(|m| m.frame_rate() == 30));
    }

    #[test]
    fn apple_modes_are_the_last_listed_for_their_size() {
        let logitech = [
            mode(1280, 960, FrameFormat::YUYV, 5),
            mode(1280, 960, FrameFormat::YUYV, 30),
            mode(1280, 960, FrameFormat::MJPEG, 30),
        ];
        assert_eq!(ranked(&logitech, true), vec![logitech[2]]);
        assert_eq!(ranked(&logitech, false), vec![logitech[1], logitech[2]]);
    }

    #[test]
    fn modes_prefer_big_enough_smooth_and_uncompressed() {
        let webcam = [
            mode(1280, 720, FrameFormat::YUYV, 10),
            mode(640, 480, FrameFormat::YUYV, 30),
            mode(120, 90, FrameFormat::YUYV, 30),
            mode(1280, 720, FrameFormat::MJPEG, 30),
            mode(320, 240, FrameFormat::MJPEG, 30),
            mode(640, 480, FrameFormat::YUYV, 0),
        ];
        assert_eq!(
            ranked(&webcam, false),
            vec![webcam[1], webcam[4], webcam[3], webcam[0], webcam[2]]
        );
    }

    fn luma(format: FrameFormat, width: usize, height: usize, data: &[u8]) -> Option<Vec<u8>> {
        decode(format, width, height, data).map(|feed| feed.luma)
    }

    fn luma_of(format: FrameFormat, data: &[u8]) -> Option<Vec<u8>> {
        luma(format, WIDTH, HEIGHT, data)
    }

    #[test]
    fn colours_match_between_formats() {
        let yuyv = [90, 54, 90, 240];
        let nv12 = [90, 90, 54, 240];
        let from_yuyv = decode(FrameFormat::YUYV, 2, 1, &yuyv).expect("yuyv");
        let from_nv12 = decode(FrameFormat::NV12, 2, 1, &nv12).expect("nv12");
        assert_eq!(from_yuyv.rgba, from_nv12.rgba);
        let [r, g, b, a] = from_yuyv.rgba[..4] else {
            panic!("short");
        };
        assert!(r > 200 && g < 60 && b < 60 && a == 255, "{r} {g} {b}");
    }

    #[test]
    fn rgb_and_bgr_keep_their_colours() {
        let rgb = decode(FrameFormat::RAWRGB, 1, 1, &[200, 10, 20]).expect("rgb");
        let bgr = decode(FrameFormat::RAWBGR, 1, 1, &[20, 10, 200]).expect("bgr");
        assert_eq!(rgb.rgba, vec![200, 10, 20, 255]);
        assert_eq!(bgr.rgba, rgb.rgba);
        assert_eq!(bgr.luma, rgb.luma);
    }
}
