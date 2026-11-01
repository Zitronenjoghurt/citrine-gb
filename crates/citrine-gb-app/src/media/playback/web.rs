use super::OpenError;
use crate::media::Frame;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/media/playback/video.js")]
extern "C" {
    type ClipPlayer;

    #[wasm_bindgen(constructor)]
    fn new(bytes: &[u8]) -> ClipPlayer;

    #[wasm_bindgen(static_method_of = ClipPlayer, js_name = canPlay)]
    fn can_play() -> bool;

    #[wasm_bindgen(method)]
    fn status(this: &ClipPlayer) -> u32;

    #[wasm_bindgen(method)]
    fn frame(this: &ClipPlayer) -> Option<Vec<u8>>;

    #[wasm_bindgen(method)]
    fn width(this: &ClipPlayer) -> u32;

    #[wasm_bindgen(method)]
    fn height(this: &ClipPlayer) -> u32;

    #[wasm_bindgen(method)]
    fn play(this: &ClipPlayer);

    #[wasm_bindgen(method)]
    fn pause(this: &ClipPlayer);

    #[wasm_bindgen(method)]
    fn seek(this: &ClipPlayer, seconds: f64);

    #[wasm_bindgen(method)]
    fn position(this: &ClipPlayer) -> f64;

    #[wasm_bindgen(method)]
    fn length(this: &ClipPlayer) -> f64;

    #[wasm_bindgen(method)]
    fn playing(this: &ClipPlayer) -> bool;

    #[wasm_bindgen(method, js_name = setMuted)]
    fn set_muted(this: &ClipPlayer, muted: bool);

    #[wasm_bindgen(method, js_name = setVolume)]
    fn set_volume(this: &ClipPlayer, volume: f64);

    #[wasm_bindgen(method)]
    fn close(this: &ClipPlayer);
}

pub struct Player {
    clip: ClipPlayer,
    frame: Option<Frame>,
    fresh: bool,
}

impl Player {
    pub fn open(bytes: Vec<u8>) -> Result<Self, OpenError> {
        if bytes.is_empty() {
            return Err(OpenError::Unreadable);
        }
        if !ClipPlayer::can_play() {
            return Err(OpenError::NeedsCodec);
        }
        Ok(Self {
            clip: ClipPlayer::new(&bytes),
            frame: None,
            fresh: false,
        })
    }

    pub fn update(&mut self) {
        if let Some(rgba) = self.clip.frame() {
            self.frame = Some(Frame {
                width: self.clip.width(),
                height: self.clip.height(),
                rgba,
            });
            self.fresh = true;
        }
    }

    pub fn take_frame(&mut self) -> Option<&Frame> {
        if !std::mem::take(&mut self.fresh) {
            return None;
        }
        self.frame.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.clip.playing()
    }

    pub fn is_ready(&self) -> bool {
        self.clip.status() == 1
    }

    pub fn failed(&self) -> bool {
        self.clip.status() == 2
    }

    pub fn play(&mut self) {
        if self.position() >= self.length() - 0.05 {
            self.clip.seek(0.0);
        }
        self.clip.play();
    }

    pub fn pause(&mut self) {
        self.clip.pause();
    }

    pub fn toggle(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn position(&self) -> f64 {
        self.clip.position()
    }

    pub fn length(&self) -> f64 {
        self.clip.length()
    }

    pub fn seek(&mut self, seconds: f64) {
        self.clip.seek(seconds.clamp(0.0, self.length()));
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.clip.set_muted(muted);
    }

    pub fn set_volume(&mut self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.clip.set_volume((volume * volume) as f64);
    }

    pub fn is_silent_here(&self) -> bool {
        false
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.clip.close();
    }
}
