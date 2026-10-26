use super::queue::{QueueConfig, QueueReader, QueueStats, QueueWriter, RemoteReport, audio_queue};
use js_sys::{Array, Float32Array, Object, Reflect};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioNode, AudioProcessingEvent, AudioWorkletNode, AudioWorkletNodeOptions, Blob,
    BlobPropertyBag, GainNode, MessageEvent, MessagePort, ScriptProcessorNode, Url,
};

const PROCESSOR: &str = include_str!("worklet.js");
const PROCESSOR_NAME: &str = "citrine-output";
const GAIN_SMOOTHING_SECS: f64 = 0.01;
const FALLBACK_FRAMES: u32 = 2048;
const QUANTUM_FRAMES: usize = 128;

enum Mode {
    Loading(QueueWriter),
    Worklet(MessagePort),
    Fallback(QueueWriter),
    Closed,
}

struct Link {
    mode: Mode,
    node: Option<AudioNode>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_process: Option<Closure<dyn FnMut(AudioProcessingEvent)>>,
    queued: usize,
    reported_at: f64,
    received: u64,
    sent: u64,
    playing: bool,
    paused: bool,
}

pub struct WebOutput {
    ctx: AudioContext,
    gain: GainNode,
    link: Rc<RefCell<Link>>,
}

pub struct PortWriter {
    ctx: AudioContext,
    link: Rc<RefCell<Link>>,
    stats: Arc<QueueStats>,
}

impl WebOutput {
    pub fn open() -> Result<Self, JsValue> {
        let ctx = AudioContext::new()?;
        let gain = ctx.create_gain()?;
        gain.connect_with_audio_node(&ctx.destination())?;
        Ok(Self {
            ctx,
            gain,
            link: Rc::new(RefCell::new(Link {
                mode: Mode::Closed,
                node: None,
                on_message: None,
                on_process: None,
                queued: 0,
                reported_at: 0.0,
                received: 0,
                sent: 0,
                playing: false,
                paused: false,
            })),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.ctx.sample_rate() as u32
    }

    pub fn connect(&self, config: QueueConfig) -> PortWriter {
        let options = processor_options(&config);
        let (writer, reader) = audio_queue(config);
        let stats = writer.stats().clone();
        self.link.borrow_mut().mode = Mode::Loading(writer);

        let ctx = self.ctx.clone();
        let gain = self.gain.clone();
        let link = self.link.clone();
        let report_stats = stats.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(err) = attach_worklet(&ctx, &gain, &link, options, report_stats).await {
                log::warn!("Audio worklet unavailable, playing from the page: {err:?}");
                if let Err(err) = attach_fallback(&ctx, &gain, &link, reader) {
                    log::error!("Failed to start audio: {err:?}");
                }
            }
        });

        PortWriter {
            ctx: self.ctx.clone(),
            link: self.link.clone(),
            stats,
        }
    }

    pub fn callback_frames(&self) -> usize {
        match self.link.borrow().mode {
            Mode::Fallback(_) => FALLBACK_FRAMES as usize,
            _ => QUANTUM_FRAMES,
        }
    }

    pub fn resume(&self) -> Result<(), JsValue> {
        self.ctx.resume().map(|_| ())
    }

    pub fn reset_stats(&self) {
        if let Mode::Worklet(port) = &self.link.borrow().mode {
            post_flag(port, "reset", true);
        }
    }

    pub fn set_gain(&self, gain: f32) {
        let now = self.ctx.current_time();
        let _ = self
            .gain
            .gain()
            .set_target_at_time(gain, now, GAIN_SMOOTHING_SECS);
    }
}

impl Drop for WebOutput {
    fn drop(&mut self) {
        let mut link = self.link.borrow_mut();
        if let Some(node) = link.node.take() {
            if link.on_process.is_some() {
                node.unchecked_ref::<ScriptProcessorNode>()
                    .set_onaudioprocess(None);
            }
            let _ = node.disconnect();
        }
        if let Mode::Worklet(port) = &link.mode {
            port.set_onmessage(None);
        }
        link.mode = Mode::Closed;
        link.on_message = None;
        link.on_process = None;
        let _ = self.ctx.close();
    }
}

impl PortWriter {
    pub fn write(&mut self, samples: &[f32]) {
        let mut guard = self.link.borrow_mut();
        let link = &mut *guard;
        let samples = &samples[..samples.len() & !1];
        match &mut link.mode {
            Mode::Worklet(port) => {
                if samples.is_empty() {
                    return;
                }
                let array = Float32Array::from(samples);
                let transfer = Array::of1(&array.buffer());
                if port
                    .post_message_with_transferable(&array, &transfer)
                    .is_ok()
                {
                    link.sent += (samples.len() / 2) as u64;
                }
            }
            Mode::Fallback(writer) => writer.write(samples),
            Mode::Loading(_) | Mode::Closed => {}
        }
    }

    pub fn queued_frames(&self) -> usize {
        let link = self.link.borrow();
        match &link.mode {
            Mode::Worklet(_) => {
                let mut queued = link.queued;
                if link.playing {
                    let elapsed = (self.ctx.current_time() - link.reported_at).max(0.0);
                    let played = (elapsed * self.ctx.sample_rate() as f64) as usize;
                    queued = queued.saturating_sub(played);
                }
                queued + link.sent.saturating_sub(link.received) as usize
            }
            Mode::Fallback(writer) | Mode::Loading(writer) => writer.queued_frames(),
            Mode::Closed => 0,
        }
    }

    pub fn is_playing(&self) -> bool {
        let link = self.link.borrow();
        match &link.mode {
            Mode::Worklet(_) => link.playing,
            _ => self.stats.is_playing(),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let mut link = self.link.borrow_mut();
        if link.paused == paused {
            return;
        }
        link.paused = paused;
        match &link.mode {
            Mode::Worklet(port) => post_flag(port, "paused", paused),
            Mode::Fallback(writer) | Mode::Loading(writer) => writer.set_paused(paused),
            Mode::Closed => {}
        }
    }

    pub fn stats(&self) -> &Arc<QueueStats> {
        &self.stats
    }
}

fn post_flag(port: &MessagePort, key: &str, value: bool) {
    let message = Object::new();
    let _ = Reflect::set(&message, &key.into(), &value.into());
    let _ = port.post_message(&message);
}

fn processor_options(config: &QueueConfig) -> Object {
    let options = Object::new();
    let fields = [
        ("target", config.target_frames),
        ("excess", config.excess_frames),
        ("capacity", config.capacity_frames),
        ("fade", config.fade_frames()),
    ];
    for (key, value) in fields {
        let _ = Reflect::set(&options, &key.into(), &(value as f64).into());
    }
    options
}

async fn attach_worklet(
    ctx: &AudioContext,
    gain: &GainNode,
    link: &Rc<RefCell<Link>>,
    processor: Object,
    stats: Arc<QueueStats>,
) -> Result<(), JsValue> {
    let parts = Array::of1(&JsValue::from_str(PROCESSOR));
    let kind = BlobPropertyBag::new();
    kind.set_type("text/javascript");
    let blob = Blob::new_with_str_sequence_and_options(&parts, &kind)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let loaded = match ctx
        .audio_worklet()
        .and_then(|worklet| worklet.add_module(&url))
    {
        Ok(promise) => JsFuture::from(promise).await.map(|_| ()),
        Err(err) => Err(err),
    };
    let _ = Url::revoke_object_url(&url);
    loaded?;

    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(0);
    options.set_number_of_outputs(1);
    options.set_output_channel_count(&Array::of1(&2.into()));
    options.set_processor_options(Some(&processor));
    let node = AudioWorkletNode::new_with_options(ctx, PROCESSOR_NAME, &options)?;
    node.connect_with_audio_node(gain)?;
    let port = node.port()?;

    let weak = Rc::downgrade(link);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(link) = weak.upgrade() else {
            return;
        };
        let data = event.data();
        let number = |key: &str| {
            Reflect::get(&data, &key.into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        let report = RemoteReport {
            playing: Reflect::get(&data, &"playing".into())
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            underruns: number("underruns") as u64,
            underrun_frames: number("underrunFrames") as u64,
            overruns: number("overruns") as u64,
            overrun_frames: number("overrunFrames") as u64,
            min_sample: number("min") as f32,
            max_sample: number("max") as f32,
        };
        let mut link = link.borrow_mut();
        link.queued = number("queued") as usize;
        link.reported_at = number("time");
        link.received = number("received") as u64;
        link.playing = report.playing;
        stats.mirror(&report);
    });
    port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let mut link = link.borrow_mut();
    if !matches!(link.mode, Mode::Loading(_)) {
        port.set_onmessage(None);
        let _ = node.disconnect();
        return Ok(());
    }
    if link.paused {
        post_flag(&port, "paused", true);
    }
    link.mode = Mode::Worklet(port);
    link.node = Some(node.into());
    link.on_message = Some(on_message);
    Ok(())
}

fn attach_fallback(
    ctx: &AudioContext,
    gain: &GainNode,
    link: &Rc<RefCell<Link>>,
    mut reader: QueueReader,
) -> Result<(), JsValue> {
    let mut link = link.borrow_mut();
    let writer = match std::mem::replace(&mut link.mode, Mode::Closed) {
        Mode::Loading(writer) => writer,
        other => {
            link.mode = other;
            return Ok(());
        }
    };
    writer.set_paused(link.paused);

    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            FALLBACK_FRAMES,
            0,
            2,
        )?;
    let mut interleaved = Vec::new();
    let mut left = Vec::new();
    let mut right = Vec::new();
    let on_process =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
            let Ok(buffer) = event.output_buffer() else {
                return;
            };
            let frames = buffer.length() as usize;
            interleaved.resize(frames * 2, 0.0);
            reader.read(&mut interleaved, 2, 1.0);
            left.clear();
            right.clear();
            for frame in interleaved.chunks(2) {
                left.push(frame[0]);
                right.push(frame[1]);
            }
            let _ = buffer.copy_to_channel(&left, 0);
            let _ = buffer.copy_to_channel(&right, 1);
        });
    node.set_onaudioprocess(Some(on_process.as_ref().unchecked_ref()));
    node.connect_with_audio_node(gain)?;

    link.mode = Mode::Fallback(writer);
    link.node = Some(node.into());
    link.on_process = Some(on_process);
    Ok(())
}
