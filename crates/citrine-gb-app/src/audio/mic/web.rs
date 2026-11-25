use super::{Backend, Capture, CaptureState, MicLine, Microphone, OpenError};
use js_sys::{Array, Object, Reflect};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioProcessingEvent, DomException, MediaDeviceInfo, MediaDeviceKind,
    MediaStream, MediaStreamAudioSourceNode, MediaStreamConstraints, MediaStreamTrack,
    ScriptProcessorNode,
};

const CALLBACK_FRAMES: u32 = 1024;

thread_local! {
    static DEVICES: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

pub fn input_devices() -> Vec<String> {
    DEVICES.with_borrow(|devices| devices.iter().map(|(label, _)| label.clone()).collect())
}

pub struct Web;

impl Backend for Web {
    fn open(&mut self, choice: &Microphone, line: &MicLine) -> Result<Box<dyn Capture>, OpenError> {
        let media = web_sys::window()
            .and_then(|window| window.navigator().media_devices().ok())
            .ok_or(OpenError::NoDevice)?;
        let audio: JsValue = match device_id(choice) {
            Some(id) => {
                let exact = Object::new();
                let _ = Reflect::set(&exact, &"exact".into(), &id.into());
                let wanted = Object::new();
                let _ = Reflect::set(&wanted, &"deviceId".into(), &exact);
                wanted.into()
            }
            None => true.into(),
        };
        let constraints = MediaStreamConstraints::new();
        constraints.set_audio(&audio);
        let promise = media
            .get_user_media_with_constraints(&constraints)
            .map_err(|_| OpenError::Failed)?;
        let shared = Rc::new(RefCell::new(Shared::new()));
        let task = shared.clone();
        let line = line.clone();
        citrine_gb_threads::spawn_local(async move {
            let state = match JsFuture::from(promise).await {
                Ok(stream) if task.borrow().closed => {
                    let stream: MediaStream = stream.unchecked_into();
                    for track in stream.get_tracks().iter() {
                        if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                            track.stop();
                        }
                    }
                    return;
                }
                Ok(stream) => match start(stream.unchecked_into(), &line, &task) {
                    Ok(()) => {
                        list_devices().await;
                        CaptureState::Live
                    }
                    Err(err) => {
                        log::warn!("microphone: {err:?}");
                        CaptureState::Ended(OpenError::Failed)
                    }
                },
                Err(err) => CaptureState::Ended(refusal(&err)),
            };
            let mut shared = task.borrow_mut();
            if shared.state == CaptureState::Starting {
                shared.state = state;
            }
            if shared.closed {
                shared.stop();
            }
        });
        Ok(Box::new(WebCapture(shared)))
    }
}

fn device_id(choice: &Microphone) -> Option<String> {
    let Microphone::Device(name) = choice else {
        return None;
    };
    DEVICES.with_borrow(|devices| {
        devices
            .iter()
            .find(|(label, _)| label == name)
            .map(|(_, id)| id.clone())
    })
}

fn refusal(err: &JsValue) -> OpenError {
    let name = err
        .dyn_ref::<DomException>()
        .map(DomException::name)
        .unwrap_or_default();
    log::warn!("microphone: getUserMedia failed: {name}");
    match name.as_str() {
        "NotAllowedError" | "SecurityError" => OpenError::Refused,
        "NotFoundError" | "OverconstrainedError" => OpenError::NoDevice,
        _ => OpenError::Failed,
    }
}

async fn list_devices() {
    let Some(media) = web_sys::window().and_then(|window| window.navigator().media_devices().ok())
    else {
        return;
    };
    let Ok(promise) = media.enumerate_devices() else {
        return;
    };
    let Ok(list) = JsFuture::from(promise).await else {
        return;
    };
    let devices: Vec<(String, String)> = Array::from(&list)
        .iter()
        .filter_map(|device| device.dyn_into::<MediaDeviceInfo>().ok())
        .filter(|device| device.kind() == MediaDeviceKind::Audioinput)
        .filter(|device| !device.label().is_empty() && device.device_id() != "default")
        .map(|device| (device.label(), device.device_id()))
        .collect();
    DEVICES.with_borrow_mut(|known| *known = devices);
}

struct Shared {
    state: CaptureState,
    closed: bool,
    stream: Option<MediaStream>,
    ctx: Option<AudioContext>,
    source: Option<MediaStreamAudioSourceNode>,
    node: Option<ScriptProcessorNode>,
    on_process: Option<Closure<dyn FnMut(AudioProcessingEvent)>>,
    on_ended: Option<Closure<dyn FnMut()>>,
}

impl Shared {
    fn new() -> Self {
        Self {
            state: CaptureState::Starting,
            closed: false,
            stream: None,
            ctx: None,
            source: None,
            node: None,
            on_process: None,
            on_ended: None,
        }
    }

    fn tracks(&self) -> Vec<MediaStreamTrack> {
        let Some(stream) = &self.stream else {
            return Vec::new();
        };
        stream
            .get_tracks()
            .iter()
            .filter_map(|track| track.dyn_into::<MediaStreamTrack>().ok())
            .collect()
    }

    fn stop(&mut self) {
        for track in self.tracks() {
            track.set_onended(None);
            track.stop();
        }
        if let Some(node) = self.node.take() {
            node.set_onaudioprocess(None);
            let _ = node.disconnect();
        }
        if let Some(source) = self.source.take() {
            let _ = source.disconnect();
        }
        if let Some(ctx) = self.ctx.take() {
            let _ = ctx.close();
        }
        self.stream = None;
        self.on_process = None;
        self.on_ended = None;
    }
}

fn start(stream: MediaStream, line: &MicLine, shared: &Rc<RefCell<Shared>>) -> Result<(), JsValue> {
    let ctx = AudioContext::new()?;
    line.restart(ctx.sample_rate() as u32);
    let source = ctx.create_media_stream_source(&stream)?;
    let node = ctx
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
            CALLBACK_FRAMES,
            1,
            1,
        )?;
    let line = line.clone();
    let mut samples = Vec::new();
    let mut carry = Vec::new();
    let on_process =
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
            let Ok(buffer) = event.input_buffer() else {
                return;
            };
            samples.resize(buffer.length() as usize, 0.0);
            if buffer.copy_from_channel(&mut samples, 0).is_ok() {
                line.push_or_carry(&mut carry, &samples);
            }
        });
    node.set_onaudioprocess(Some(on_process.as_ref().unchecked_ref()));
    source.connect_with_audio_node(&node)?;
    node.connect_with_audio_node(&ctx.destination())?;
    let _ = ctx.resume();
    let ended = Rc::downgrade(shared);
    let on_ended = Closure::<dyn FnMut()>::new(move || {
        if let Some(shared) = ended.upgrade() {
            shared.borrow_mut().state = CaptureState::Gone;
        }
    });
    let mut shared = shared.borrow_mut();
    shared.stream = Some(stream);
    for track in shared.tracks() {
        track.set_onended(Some(on_ended.as_ref().unchecked_ref()));
    }
    shared.ctx = Some(ctx);
    shared.source = Some(source);
    shared.node = Some(node);
    shared.on_process = Some(on_process);
    shared.on_ended = Some(on_ended);
    Ok(())
}

struct WebCapture(Rc<RefCell<Shared>>);

impl Capture for WebCapture {
    fn state(&mut self) -> CaptureState {
        self.0.borrow().state
    }
}

impl Drop for WebCapture {
    fn drop(&mut self) {
        let mut shared = self.0.borrow_mut();
        shared.closed = true;
        shared.stop();
    }
}
