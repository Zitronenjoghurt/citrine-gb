use crate::emulator::gimmicks::camera::{CameraView, Feed};
use crate::emulator::gimmicks::{CameraSource, Cart, GimmickSettings, Motion};
use crate::pad_input::PadSource;

pub mod motion;

#[cfg(target_os = "android")]
const CAMERA_DENIED: &str = "Citrine may not use the camera. Allow it in the phone's app settings.";
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod webcam;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wanted {
    pub motion: bool,
    pub light: bool,
    pub camera: bool,
}

impl Wanted {
    pub fn of(cart: Cart, settings: &GimmickSettings, running: bool) -> Self {
        Self {
            motion: running && cart.motion(),
            light: running && cart.light,
            camera: running && cart.camera && settings.camera == CameraSource::Camera,
        }
    }
}

#[derive(Default)]
pub struct Senses {
    wanted: Wanted,
    level: motion::Level,
    pad_level: motion::Level,
    rumble_step: u8,
    picture: Option<Vec<u8>>,
    picture_fed: bool,
    camera_error: Option<String>,
    feed: Option<Feed>,
    feed_count: u64,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    webcam: Option<webcam::Webcam>,
}

impl Senses {
    pub fn follow(&mut self, wanted: Wanted) {
        if wanted == self.wanted {
            return;
        }
        if wanted.motion && !self.wanted.motion {
            self.level.centre();
            self.pad_level.centre();
        }
        if !wanted.camera {
            self.camera_error = None;
            self.feed = None;
        }
        self.wanted = wanted;
        #[cfg(target_os = "android")]
        crate::platform::android::sensors::want(wanted);
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        if wanted.camera {
            if self.webcam.is_none() {
                match webcam::Webcam::open() {
                    Ok(webcam) => self.webcam = Some(webcam),
                    Err(err) => self.camera_error = Some(err),
                }
            }
        } else {
            self.webcam = None;
        }
    }

    pub fn centre(&mut self) {
        self.level.centre();
        self.pad_level.centre();
    }

    pub fn motion(&mut self, pads: &mut dyn PadSource) -> Option<Motion> {
        if !self.wanted.motion {
            return None;
        }
        #[cfg(target_os = "android")]
        if let Some((gravity, gyro)) = crate::platform::android::sensors::motion() {
            return self.level.motion(gravity, gyro);
        }
        let pad = pads.motion()?;
        self.pad_level.motion(pad.accel, pad.gyro)
    }

    pub fn has_light_sensor(&self) -> bool {
        cfg!(target_os = "android")
    }

    pub fn has_live_camera(&self) -> bool {
        cfg!(not(target_arch = "wasm32"))
    }

    pub fn light(&self) -> Option<f32> {
        if !self.wanted.light {
            return None;
        }
        #[cfg(target_os = "android")]
        return crate::platform::android::sensors::light()
            .map(crate::emulator::gimmicks::settings::lux_to_light);
        #[cfg(not(target_os = "android"))]
        None
    }

    pub fn set_picture(&mut self, frame: Vec<u8>) {
        self.picture = Some(frame);
        self.picture_fed = false;
    }

    pub fn camera_error(&self) -> Option<&str> {
        #[cfg(target_os = "android")]
        if self.wanted.camera && crate::platform::android::sensors::camera_denied() {
            return Some(CAMERA_DENIED);
        }
        self.camera_error.as_deref()
    }

    pub fn camera_feed(&self) -> Option<(&Feed, u64)> {
        self.feed.as_ref().map(|feed| (feed, self.feed_count))
    }

    pub fn take_camera_frame(&mut self, source: CameraSource, view: CameraView) -> Option<Vec<u8>> {
        match source {
            CameraSource::Picture => {
                if self.picture_fed {
                    return None;
                }
                self.picture_fed = true;
                self.picture.clone()
            }
            CameraSource::Camera => {
                self.picture_fed = false;
                let feed = self.take_feed()?;
                let frame = feed.frame(view);
                self.feed = Some(feed);
                self.feed_count = self.feed_count.wrapping_add(1);
                frame
            }
        }
    }

    fn take_feed(&mut self) -> Option<Feed> {
        #[cfg(target_os = "android")]
        return crate::platform::android::sensors::take_camera_feed();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        return match self.webcam.as_mut().and_then(webcam::Webcam::take_frame) {
            Some(Ok(feed)) => Some(feed),
            Some(Err(err)) => {
                self.camera_error = Some(err);
                None
            }
            None => None,
        };
        #[cfg(target_arch = "wasm32")]
        None
    }

    pub fn rumble(&mut self, pads: &mut dyn PadSource, step: u8) {
        if step == self.rumble_step {
            return;
        }
        self.rumble_step = step;
        let strength = f32::from(step) / 4.0;
        pads.rumble(strength);
        #[cfg(target_os = "android")]
        crate::platform::android::sensors::vibrate(strength);
    }
}
