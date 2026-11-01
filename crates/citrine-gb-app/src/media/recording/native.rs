use super::audio::AacEncoder;
use super::h264::H264Encoder;
use super::mp4::{Mp4Writer, VideoFormat, avc_config};
use super::{FRAME_TICKS, SOURCE_HEIGHT, SOURCE_WIDTH, StopReason, VIDEO_TIMESCALE, fps};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;

const BACKLOG_FRAMES: usize = 600;

pub struct Output {
    pub path: PathBuf,
    pub size: u64,
}

enum Message {
    Frame(Vec<u8>, u32),
    Audio(Vec<f32>),
}

pub struct Worker {
    sender: SyncSender<Message>,
    failed: Arc<AtomicBool>,
    full: bool,
    handle: JoinHandle<anyhow::Result<Output>>,
}

impl Worker {
    pub fn start(path: PathBuf, sample_rate: u32, scale: usize) -> anyhow::Result<Self> {
        Self::start_with(super::video_api()?, path, sample_rate, scale)
    }

    pub fn start_with(
        api: openh264::OpenH264API,
        path: PathBuf,
        sample_rate: u32,
        scale: usize,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let video = H264Encoder::new(api, SOURCE_WIDTH, SOURCE_HEIGHT, scale, fps() as f32)?;
        let audio = AacEncoder::new(sample_rate)?;
        let file = File::create(&path)?;
        let (sender, receiver) = sync_channel(BACKLOG_FRAMES * 2);
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        let handle = std::thread::Builder::new()
            .name("screen recording".into())
            .spawn(move || {
                let result = encode(receiver, file, &path, video, audio, scale);
                if result.is_err() {
                    flag.store(true, Ordering::Relaxed);
                    let _ = std::fs::remove_file(&path);
                }
                result
            })?;
        Ok(Self {
            sender,
            failed,
            full: false,
            handle,
        })
    }

    pub fn frame(&mut self, rgba: &[u8], repeat: u32) {
        self.send(Message::Frame(rgba.to_vec(), repeat));
    }

    pub fn audio(&mut self, samples: &[f32]) {
        self.send(Message::Audio(samples.to_vec()));
    }

    fn send(&mut self, message: Message) {
        match self.sender.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(message)) => {
                self.full = true;
                let _ = self.sender.send(message);
            }
            Err(TrySendError::Disconnected(_)) => self.failed.store(true, Ordering::Relaxed),
        }
    }

    pub fn failed_or_full(&self) -> Option<StopReason> {
        if self.failed.load(Ordering::Relaxed) {
            Some(StopReason::Failed)
        } else if self.full {
            Some(StopReason::TooSlow)
        } else {
            None
        }
    }

    pub fn finish(self) -> Pending {
        drop(self.sender);
        Pending {
            handle: Some(self.handle),
        }
    }
}

pub struct Pending {
    handle: Option<JoinHandle<anyhow::Result<Output>>>,
}

impl Pending {
    pub fn is_done(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }

    pub fn wait(&mut self) {
        while !self.is_done() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    pub fn take(mut self) -> anyhow::Result<Output> {
        let handle = self
            .handle
            .take()
            .ok_or_else(|| anyhow::anyhow!("the recording was already saved"))?;
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("the recording stopped unexpectedly"))?
    }
}

fn encode(
    receiver: Receiver<Message>,
    file: File,
    path: &Path,
    mut video: H264Encoder,
    mut audio: AacEncoder,
    scale: usize,
) -> anyhow::Result<Output> {
    let mut mp4 = Mp4Writer::new(BufWriter::with_capacity(1 << 20, file))?;
    mp4.layout().set_audio(audio.format());
    for message in receiver {
        match message {
            Message::Frame(rgba, repeat) => {
                for _ in 0..repeat {
                    let frame = video.encode(&rgba)?;
                    if !mp4.layout().has_video_format() {
                        let (Some(sps), Some(pps)) = (frame.sps, frame.pps) else {
                            anyhow::bail!("the video encoder sent no stream header");
                        };
                        mp4.layout().set_video(VideoFormat {
                            width: (SOURCE_WIDTH * scale) as u16,
                            height: (SOURCE_HEIGHT * scale) as u16,
                            timescale: VIDEO_TIMESCALE,
                            avc_config: avc_config(&sps, &pps),
                        });
                    }
                    mp4.write_video(&frame.sample, FRAME_TICKS, frame.keyframe)?;
                }
            }
            Message::Audio(samples) => {
                for packet in audio.push(&samples)? {
                    mp4.write_audio(&packet.data, packet.duration)?;
                }
            }
        }
    }
    for packet in audio.finish()? {
        mp4.write_audio(&packet.data, packet.duration)?;
    }
    mp4.layout().set_audio(audio.format());
    let writer = mp4.finish()?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    let size = file.metadata()?.len();
    Ok(Output {
        path: path.to_path_buf(),
        size,
    })
}
