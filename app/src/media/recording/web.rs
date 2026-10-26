use super::mp4::{AudioCodec, AudioFormat, Mp4Layout, VideoFormat};
use super::{
    AUDIO_BITRATE, Availability, CHANNELS, FRAME_TICKS, SOURCE_HEIGHT, SOURCE_WIDTH, StopReason,
    VIDEO_TIMESCALE, fps, upscale_rgba, video_bitrate,
};

type SupportKey = (u32, usize);
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

const WEB_VIDEO_BITRATE: u32 = 1_500_000;
const OPUS_RATE: u32 = 48_000;
const OPUS_PRE_SKIP: u32 = 312;
const MAX_BACKLOG: u32 = 180;
pub const MAX_LENGTH: Option<Duration> = Some(Duration::from_secs(30 * 60));

#[wasm_bindgen(module = "/src/media/recording/webcodecs.js")]
extern "C" {
    #[wasm_bindgen(js_name = probeRecording)]
    fn probe_recording(
        width: u32,
        height: u32,
        fps: f64,
        sample_rate: u32,
        video_bitrate: u32,
        audio_bitrate: u32,
    ) -> js_sys::Promise;

    type WebRecorder;

    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    fn new(
        video_codec: &str,
        audio_codec: &str,
        width: u32,
        height: u32,
        fps: f64,
        sample_rate: u32,
        video_bitrate: u32,
        audio_bitrate: u32,
        on_video: &Closure<dyn FnMut(u32, bool, JsValue)>,
        on_audio: &Closure<dyn FnMut(u32, f64, JsValue)>,
        on_error: &Closure<dyn FnMut(String)>,
    ) -> WebRecorder;

    #[wasm_bindgen(method)]
    fn backlog(this: &WebRecorder) -> u32;

    #[wasm_bindgen(method)]
    fn frame(this: &WebRecorder, rgba: &[u8], timestamp: f64, duration: f64, key_frame: bool);

    #[wasm_bindgen(method)]
    fn sound(this: &WebRecorder, samples: &[f32], timestamp: f64);

    #[wasm_bindgen(method)]
    fn finish(this: &WebRecorder) -> js_sys::Promise;

    #[wasm_bindgen(method)]
    fn blob(this: &WebRecorder, head: &[u8], moov: &[u8]) -> web_sys::Blob;
}

#[derive(Clone)]
enum Support {
    Unknown,
    Checking(SupportKey),
    Ready {
        key: SupportKey,
        video: String,
        audio: String,
    },
    Missing {
        key: SupportKey,
        reason: String,
    },
}

thread_local! {
    static SUPPORT: RefCell<Support> = const { RefCell::new(Support::Unknown) };
}

fn string_field(value: &JsValue, name: &str) -> String {
    js_sys::Reflect::get(value, &JsValue::from_str(name))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

pub fn availability(sample_rate: u32, scale: usize) -> Availability {
    let wanted = (sample_rate, scale);
    let current = SUPPORT.with(|s| s.borrow().clone());
    match current {
        Support::Ready { key, .. } if key == wanted => Availability::Ready,
        Support::Missing { key, reason } if key == wanted => Availability::Missing(reason),
        Support::Checking(key) if key == wanted => Availability::Checking,
        _ => {
            SUPPORT.with(|s| *s.borrow_mut() = Support::Checking(wanted));
            let promise = probe_recording(
                (SOURCE_WIDTH * scale) as u32,
                (SOURCE_HEIGHT * scale) as u32,
                fps(),
                sample_rate,
                video_bitrate(WEB_VIDEO_BITRATE, scale),
                AUDIO_BITRATE,
            );
            wasm_bindgen_futures::spawn_local(async move {
                let result = JsFuture::from(promise).await.unwrap_or(JsValue::NULL);
                let video = string_field(&result, "video");
                let audio = string_field(&result, "audio");
                let reason = string_field(&result, "reason");
                let support = if !video.is_empty() && !audio.is_empty() {
                    Support::Ready {
                        key: wanted,
                        video,
                        audio,
                    }
                } else {
                    Support::Missing {
                        key: wanted,
                        reason: if reason.is_empty() {
                            "This browser can't record video. Chrome and Safari can.".to_owned()
                        } else {
                            reason
                        },
                    }
                };
                SUPPORT.with(|s| *s.borrow_mut() = support);
            });
            Availability::Checking
        }
    }
}

pub struct Output {
    pub blob: web_sys::Blob,
    pub size: u64,
}

struct State {
    layout: Mp4Layout,
    error: Option<String>,
    opus: bool,
    audio_config: Vec<u8>,
    sample_rate: u32,
    result: Option<Result<Output, String>>,
}

impl State {
    fn audio_format(&self, frames: u64) -> AudioFormat {
        if self.opus {
            AudioFormat {
                codec: AudioCodec::Opus,
                sample_rate: OPUS_RATE,
                channels: CHANNELS as u16,
                priming: OPUS_PRE_SKIP,
                frames: frames * OPUS_RATE as u64 / self.sample_rate.max(1) as u64,
            }
        } else {
            AudioFormat {
                codec: AudioCodec::Aac {
                    config: self.audio_config.clone(),
                },
                sample_rate: self.sample_rate,
                channels: CHANNELS as u16,
                priming: 0,
                frames,
            }
        }
    }
}

type Callbacks = (
    Closure<dyn FnMut(u32, bool, JsValue)>,
    Closure<dyn FnMut(u32, f64, JsValue)>,
    Closure<dyn FnMut(String)>,
);

pub struct Worker {
    recorder: WebRecorder,
    state: Rc<RefCell<State>>,
    callbacks: Callbacks,
    upscaled: Vec<u8>,
    scale: usize,
    frames: u64,
    audio_frames: u64,
    sample_rate: u32,
}

fn bytes_of(value: &JsValue) -> Option<Vec<u8>> {
    if value.is_null() || value.is_undefined() {
        None
    } else {
        Some(js_sys::Uint8Array::new(value).to_vec())
    }
}

fn aac_config(sample_rate: u32) -> Vec<u8> {
    const RATES: [u32; 12] = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000,
    ];
    let index = RATES.iter().position(|&r| r == sample_rate).unwrap_or(3) as u8;
    vec![
        (2 << 3) | (index >> 1),
        ((index & 1) << 7) | ((CHANNELS as u8) << 3),
    ]
}

impl Worker {
    pub fn start(sample_rate: u32, scale: usize) -> anyhow::Result<Self> {
        let (width, height) = (SOURCE_WIDTH * scale, SOURCE_HEIGHT * scale);
        let (video_codec, audio_codec) = match SUPPORT.with(|s| s.borrow().clone()) {
            Support::Ready { key, video, audio } if key == (sample_rate, scale) => (video, audio),
            _ => match availability(sample_rate, scale) {
                Availability::Missing(reason) => anyhow::bail!("{reason}"),
                _ => anyhow::bail!("This browser is still checking whether it can record"),
            },
        };
        let opus = audio_codec == "opus";
        let state = Rc::new(RefCell::new(State {
            layout: Mp4Layout::default(),
            error: None,
            opus,
            audio_config: aac_config(sample_rate),
            sample_rate,
            result: None,
        }));
        let initial = state.borrow().audio_format(0);
        state.borrow_mut().layout.set_audio(initial);

        let video_state = state.clone();
        let on_video = Closure::<dyn FnMut(u32, bool, JsValue)>::new(
            move |size: u32, key: bool, description: JsValue| {
                let mut state = video_state.borrow_mut();
                if let Some(config) = bytes_of(&description) {
                    state.layout.set_video(VideoFormat {
                        width: width as u16,
                        height: height as u16,
                        timescale: VIDEO_TIMESCALE,
                        avc_config: config,
                    });
                }
                state.layout.add_video(size as usize, FRAME_TICKS, key);
            },
        );
        let audio_state = state.clone();
        let on_audio = Closure::<dyn FnMut(u32, f64, JsValue)>::new(
            move |size: u32, duration: f64, description: JsValue| {
                let mut state = audio_state.borrow_mut();
                if let Some(config) = bytes_of(&description)
                    && !state.opus
                    && !config.is_empty()
                {
                    state.audio_config = config;
                    let format = state.audio_format(0);
                    state.layout.set_audio(format);
                }
                let ticks = if state.opus {
                    (duration * OPUS_RATE as f64 / 1_000_000.0).round() as u32
                } else {
                    1024
                };
                state.layout.add_audio(size as usize, ticks.max(1));
            },
        );
        let error_state = state.clone();
        let on_error = Closure::<dyn FnMut(String)>::new(move |message: String| {
            error_state.borrow_mut().error = Some(message);
        });
        let recorder = WebRecorder::new(
            &video_codec,
            &audio_codec,
            width as u32,
            height as u32,
            fps(),
            sample_rate,
            video_bitrate(WEB_VIDEO_BITRATE, scale),
            AUDIO_BITRATE,
            &on_video,
            &on_audio,
            &on_error,
        );
        Ok(Self {
            recorder,
            state,
            callbacks: (on_video, on_audio, on_error),
            upscaled: vec![0; width * height * 4],
            scale,
            frames: 0,
            audio_frames: 0,
            sample_rate,
        })
    }

    pub fn frame(&mut self, rgba: &[u8], repeat: u32) {
        upscale_rgba(
            rgba,
            SOURCE_WIDTH,
            SOURCE_HEIGHT,
            self.scale,
            &mut self.upscaled,
        );
        let step = 1_000_000.0 / fps();
        let keyframe_every = (fps().round() as u64) * 2;
        for _ in 0..repeat {
            let timestamp = self.frames as f64 * step;
            self.recorder.frame(
                &self.upscaled,
                timestamp.round(),
                step.round(),
                self.frames.is_multiple_of(keyframe_every),
            );
            self.frames += 1;
        }
    }

    pub fn audio(&mut self, samples: &[f32]) {
        let timestamp = self.audio_frames as f64 * 1_000_000.0 / self.sample_rate.max(1) as f64;
        self.recorder.sound(samples, timestamp.round());
        self.audio_frames += (samples.len() / CHANNELS) as u64;
    }

    pub fn failed_or_full(&self) -> Option<StopReason> {
        if self.state.borrow().error.is_some() {
            Some(StopReason::Failed)
        } else if self.recorder.backlog() > MAX_BACKLOG {
            Some(StopReason::TooSlow)
        } else {
            None
        }
    }

    pub fn finish(self) -> Pending {
        let Worker {
            recorder,
            state,
            callbacks,
            audio_frames,
            ..
        } = self;
        let pending = state.clone();
        let promise = recorder.finish();
        wasm_bindgen_futures::spawn_local(async move {
            let flushed = JsFuture::from(promise)
                .await
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut state = state.borrow_mut();
            let result = match (&state.error, flushed) {
                (Some(error), _) => Err(error.clone()),
                (None, false) => Err("the browser stopped the encoder".to_owned()),
                (None, true) if !state.layout.has_video_format() => {
                    Err("the browser sent no video".to_owned())
                }
                (None, true) => {
                    let format = state.audio_format(audio_frames);
                    state.layout.set_audio(format);
                    let head = state.layout.head();
                    let moov = state.layout.moov();
                    let blob = recorder.blob(&head, &moov);
                    Ok(Output {
                        size: blob.size() as u64,
                        blob,
                    })
                }
            };
            state.result = Some(result);
            drop(callbacks);
        });
        Pending { state: pending }
    }
}

pub struct Pending {
    state: Rc<RefCell<State>>,
}

impl Pending {
    pub fn is_done(&self) -> bool {
        self.state.borrow().result.is_some()
    }

    pub fn take(self) -> anyhow::Result<Output> {
        match self.state.borrow_mut().result.take() {
            Some(Ok(output)) => Ok(output),
            Some(Err(error)) => Err(anyhow::anyhow!("{error}")),
            None => Err(anyhow::anyhow!("the recording isn't finished yet")),
        }
    }
}
