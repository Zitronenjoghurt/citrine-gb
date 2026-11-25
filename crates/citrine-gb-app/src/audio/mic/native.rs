use super::{Backend, Capture, CaptureState, MicLine, Microphone, OpenError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamError};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const LIVE: u8 = 0;
const GONE: u8 = 1;
const BROKEN: u8 = 2;

pub struct Native;

impl Backend for Native {
    fn open(&mut self, choice: &Microphone, line: &MicLine) -> Result<Box<dyn Capture>, OpenError> {
        #[cfg(target_os = "android")]
        {
            crate::platform::android::microphone::ask();
            Ok(Box::new(Asking {
                choice: choice.clone(),
                line: line.clone(),
                stream: None,
            }))
        }
        #[cfg(not(target_os = "android"))]
        open_stream(choice, line).map(|capture| Box::new(capture) as Box<dyn Capture>)
    }
}

pub fn input_devices() -> Vec<String> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    let mut names: Vec<String> = devices.filter_map(|device| label(&device)).collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn label(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
}

fn device(choice: &Microphone) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let named = match choice {
        Microphone::Device(wanted) => host.input_devices().ok().and_then(|mut devices| {
            devices.find(|device| label(device).as_deref() == Some(wanted.as_str()))
        }),
        _ => None,
    };
    named.or_else(|| host.default_input_device())
}

struct Stream {
    _stream: cpal::Stream,
    state: Arc<AtomicU8>,
}

impl Capture for Stream {
    fn state(&mut self) -> CaptureState {
        match self.state.load(Ordering::Relaxed) {
            GONE => CaptureState::Gone,
            BROKEN => CaptureState::Restart,
            _ => access(),
        }
    }
}

fn open_stream(choice: &Microphone, line: &MicLine) -> Result<Stream, OpenError> {
    let device = device(choice).ok_or(OpenError::NoDevice)?;
    let supported = device.default_input_config().map_err(|err| {
        log::warn!("microphone: no input config: {err}");
        OpenError::Failed
    })?;
    let config = supported.config();
    let channels = usize::from(config.channels.max(1));
    line.restart(config.sample_rate);
    let state = Arc::new(AtomicU8::new(LIVE));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, line, &state),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, line, &state),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, line, &state),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, line, &state),
        SampleFormat::I8 => build::<i8>(&device, &config, channels, line, &state),
        SampleFormat::U8 => build::<u8>(&device, &config, channels, line, &state),
        SampleFormat::F64 => build::<f64>(&device, &config, channels, line, &state),
        other => {
            log::warn!("microphone: sample format {other} is not supported");
            return Err(OpenError::Failed);
        }
    }
    .map_err(|err| {
        log::warn!("microphone: could not open the input: {err}");
        OpenError::Failed
    })?;
    stream.play().map_err(|err| {
        log::warn!("microphone: could not start the input: {err}");
        OpenError::Failed
    })?;
    if let CaptureState::Ended(err) = access() {
        return Err(err);
    }
    Ok(Stream {
        _stream: stream,
        state,
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    line: &MicLine,
    state: &Arc<AtomicU8>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let line = line.clone();
    let failed = state.clone();
    let mut mono = Vec::new();
    let mut carry = Vec::new();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            mono.clear();
            mono.extend(data.chunks(channels).map(|frame| {
                frame
                    .iter()
                    .map(|&sample| f32::from_sample(sample))
                    .sum::<f32>()
                    / channels as f32
            }));
            line.push_or_carry(&mut carry, &mono);
        },
        move |err| {
            log::warn!("microphone: {err}");
            match err {
                StreamError::DeviceNotAvailable => failed.store(GONE, Ordering::Relaxed),
                StreamError::StreamInvalidated => failed.store(BROKEN, Ordering::Relaxed),
                _ => {}
            }
        },
        None,
    )
}

#[cfg(target_os = "macos")]
fn access() -> CaptureState {
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {}

    let media = NSString::from_str("soun");
    let status: isize =
        unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*media] };
    match status {
        1 | 2 => CaptureState::Ended(OpenError::Refused),
        3 => CaptureState::Live,
        _ => CaptureState::Starting,
    }
}

#[cfg(not(target_os = "macos"))]
fn access() -> CaptureState {
    CaptureState::Live
}

#[cfg(target_os = "android")]
struct Asking {
    choice: Microphone,
    line: MicLine,
    stream: Option<Stream>,
}

#[cfg(target_os = "android")]
impl Capture for Asking {
    fn state(&mut self) -> CaptureState {
        use crate::platform::android::microphone::{Permission, permission};
        if let Some(stream) = &mut self.stream {
            return stream.state();
        }
        match permission() {
            Permission::Waiting => CaptureState::Starting,
            Permission::Denied => CaptureState::Ended(OpenError::Refused),
            Permission::Granted => match open_stream(&self.choice, &self.line) {
                Ok(stream) => {
                    self.stream = Some(stream);
                    CaptureState::Live
                }
                Err(err) => CaptureState::Ended(err),
            },
        }
    }
}
