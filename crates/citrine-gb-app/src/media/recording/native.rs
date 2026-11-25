use super::audio::AacEncoder;
#[cfg(feature = "nds")]
use super::h264::rgba_to_i420;
use super::h264::{H264Encoder, upscale_to_i420};
use super::mp4::{Mp4Writer, VideoFormat, avc_config};
use super::{Source, StopReason};
#[cfg(feature = "nds")]
use crate::emulator::layout::ScreenLayout;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{
    self, Receiver, Sender, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::time::{Duration, Instant};

const BACKLOG_FRAMES: usize = 600;
const BACKLOG_BYTES: usize = 1 << 30;
const BEHIND_PICTURES: usize = 6;
const READY_PICTURES: usize = 2;

pub struct Output {
    pub path: PathBuf,
    pub size: u64,
}

impl Output {
    pub fn pack(
        self,
        replay: citrine_gb_replay::Replay,
    ) -> Result<(PathBuf, Self), (String, Self)> {
        let packed = self.path.with_extension("replay");
        match write_packed(&self.path, &packed, replay) {
            Ok(()) => Ok((packed, self)),
            Err(err) => {
                let _ = std::fs::remove_file(&packed);
                Err((err, self))
            }
        }
    }
}

fn write_packed(
    mp4: &std::path::Path,
    packed: &std::path::Path,
    mut replay: citrine_gb_replay::Replay,
) -> Result<(), String> {
    replay.video = Some(std::fs::read(mp4).map_err(|err| err.to_string())?);
    let bytes = replay.write().map_err(|err| err.to_string())?;
    std::fs::write(packed, bytes).map_err(|err| err.to_string())
}

enum Message {
    Frame(Vec<u8>, u32),
    #[cfg(feature = "nds")]
    Sharp(Box<super::Sharp>, Option<ScreenLayout>, u32),
    #[cfg(feature = "nds")]
    Again(u32),
    Late(u32),
    Audio(Vec<f32>),
}

enum Ready {
    Picture(Vec<u8>, u32),
    #[cfg(feature = "nds")]
    Again(u32),
    Late(u32),
    Audio(Vec<f32>),
}

impl Message {
    fn bytes(&self) -> usize {
        match self {
            Self::Frame(rgba, _) => rgba.len(),
            #[cfg(feature = "nds")]
            Self::Sharp(sharp, _, _) => sharp.bytes(),
            #[cfg(feature = "nds")]
            Self::Again(_) => 0,
            Self::Late(_) => 0,
            Self::Audio(samples) => samples.len() * 4,
        }
    }

    fn picture(&self) -> Option<u32> {
        match self {
            Self::Frame(_, repeat) => Some(*repeat),
            #[cfg(feature = "nds")]
            Self::Sharp(_, _, repeat) => Some(*repeat),
            #[cfg(feature = "nds")]
            Self::Again(_) => None,
            Self::Late(_) | Self::Audio(_) => None,
        }
    }
}

struct KeepUp {
    waiting: Arc<AtomicUsize>,
    period: Duration,
    pace: Duration,
    last: Option<Instant>,
    kept: bool,
}

impl KeepUp {
    fn new(waiting: Arc<AtomicUsize>, period: Duration) -> Self {
        Self {
            waiting,
            period,
            pace: Duration::ZERO,
            last: None,
            kept: false,
        }
    }

    fn pass(&mut self, message: Message, now: Instant) -> Message {
        let Some(repeat) = message.picture() else {
            return message;
        };
        if let Some(last) = self.last.replace(now) {
            self.pace = self.pace * 7 / 8 + now.saturating_duration_since(last) / 8;
        }
        let behind = self.waiting.fetch_sub(1, Ordering::Relaxed) > BEHIND_PICTURES;
        if behind && self.kept && self.pace > self.period {
            return Message::Late(repeat);
        }
        self.kept = true;
        message
    }
}

pub struct Worker {
    sender: SyncSender<Message>,
    failed: Arc<AtomicBool>,
    queued: Arc<AtomicUsize>,
    pictures: Arc<AtomicUsize>,
    full: bool,
    done: Receiver<anyhow::Result<Output>>,
}

impl Worker {
    pub fn start(
        spawner: &citrine_gb_threads::Spawner,
        path: PathBuf,
        source: Source,
        sample_rate: u32,
        scale: usize,
    ) -> anyhow::Result<Self> {
        Self::start_with(
            spawner,
            super::video_api()?,
            path,
            source,
            sample_rate,
            scale,
        )
    }

    pub fn start_with(
        spawner: &citrine_gb_threads::Spawner,
        api: openh264::OpenH264API,
        path: PathBuf,
        source: Source,
        sample_rate: u32,
        scale: usize,
    ) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let video = H264Encoder::new(api, source.width, source.height, scale, source.fps() as f32)?;
        let audio = AacEncoder::new(sample_rate)?;
        let file = File::create(&path)?;
        let (sender, receiver) = sync_channel(BACKLOG_FRAMES * 2);
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        let queued = Arc::new(AtomicUsize::new(0));
        let taken = queued.clone();
        let pictures = Arc::new(AtomicUsize::new(0));
        let waiting = pictures.clone();
        let (finished, done) = mpsc::channel();
        let (ready_sender, ready) = sync_channel(READY_PICTURES);
        let (spare_sender, spare) = mpsc::channel();
        let period = Duration::from_secs_f64(1.0 / source.fps());
        let size = Size {
            width: source.width,
            height: source.height,
            scale: video.scale(),
        };
        spawner.thread("screen recording", move || {
            let mut keep_up = KeepUp::new(waiting, period);
            let messages = receiver.into_iter().map(|message: Message| {
                taken.fetch_sub(message.bytes(), Ordering::Relaxed);
                keep_up.pass(message, Instant::now())
            });
            prepare(messages, ready_sender, spare, size);
        });
        spawner.thread("video encoding", move || {
            let result = encode(
                ready.into_iter(),
                spare_sender,
                file,
                video,
                audio,
                source,
                scale,
            )
            .map(|size| Output {
                path: path.clone(),
                size,
            });
            if result.is_err() {
                flag.store(true, Ordering::Relaxed);
                let _ = std::fs::remove_file(&path);
            }
            let _ = finished.send(result);
        });
        Ok(Self {
            sender,
            failed,
            queued,
            pictures,
            full: false,
            done,
        })
    }

    pub fn frame(&mut self, rgba: &[u8], repeat: u32) {
        self.send(Message::Frame(rgba.to_vec(), repeat));
    }

    #[cfg(feature = "nds")]
    pub fn sharp(&mut self, sharp: super::Sharp, layout: Option<ScreenLayout>, repeat: u32) {
        self.send(Message::Sharp(Box::new(sharp), layout, repeat));
    }

    #[cfg(feature = "nds")]
    pub fn again(&mut self, repeat: u32) {
        if repeat > 0 {
            self.send(Message::Again(repeat));
        }
    }

    pub(crate) fn pictures_waiting(&self) -> usize {
        self.pictures.load(Ordering::Relaxed)
    }

    pub fn audio(&mut self, samples: &[f32]) {
        self.send(Message::Audio(samples.to_vec()));
    }

    fn send(&mut self, message: Message) {
        let bytes = message.bytes();
        if message.picture().is_some() {
            self.pictures.fetch_add(1, Ordering::Relaxed);
        }
        if self.queued.fetch_add(bytes, Ordering::Relaxed) + bytes > BACKLOG_BYTES {
            self.full = true;
        }
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
            done: self.done,
            result: None,
        }
    }
}

pub struct Pending {
    done: Receiver<anyhow::Result<Output>>,
    result: Option<anyhow::Result<Output>>,
}

impl Pending {
    pub fn is_done(&mut self) -> bool {
        if self.result.is_none() {
            match self.done.try_recv() {
                Ok(result) => self.result = Some(result),
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err(anyhow::anyhow!("the recording stopped unexpectedly")))
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        self.result.is_some()
    }

    pub fn wait(&mut self) {
        while !self.is_done() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    pub fn take(mut self) -> anyhow::Result<Output> {
        self.wait();
        self.result
            .take()
            .unwrap_or_else(|| Err(anyhow::anyhow!("the recording was already saved")))
    }
}

#[derive(Clone, Copy)]
struct Size {
    width: usize,
    height: usize,
    scale: usize,
}

fn prepare(
    messages: impl Iterator<Item = Message>,
    ready: SyncSender<Ready>,
    spare: Receiver<Vec<u8>>,
    size: Size,
) {
    let (width, height) = (size.width * size.scale, size.height * size.scale);
    #[cfg(feature = "nds")]
    let (mut full, mut arranged) = (Vec::new(), Vec::new());
    for message in messages {
        let next = match message {
            Message::Frame(rgba, repeat) => {
                let mut yuv = spare.try_recv().unwrap_or_default();
                yuv.resize(width * height * 3 / 2, 0);
                upscale_to_i420(&rgba, size.width, size.height, size.scale, &mut yuv);
                Ready::Picture(yuv, repeat)
            }
            #[cfg(feature = "nds")]
            Message::Sharp(sharp, layout, repeat) => {
                sharp.draw(size.scale, &mut full);
                let rgba = match layout {
                    Some(layout) => {
                        layout.compose(&full, size.scale, &mut arranged);
                        &arranged
                    }
                    None => &full,
                };
                let mut yuv = spare.try_recv().unwrap_or_default();
                yuv.resize(width * height * 3 / 2, 0);
                rgba_to_i420(rgba, width, height, &mut yuv);
                Ready::Picture(yuv, repeat)
            }
            #[cfg(feature = "nds")]
            Message::Again(repeat) => Ready::Again(repeat),
            Message::Late(repeat) => Ready::Late(repeat),
            Message::Audio(samples) => Ready::Audio(samples),
        };
        if ready.send(next).is_err() {
            return;
        }
    }
}

fn encode(
    ready: impl Iterator<Item = Ready>,
    spare: Sender<Vec<u8>>,
    file: File,
    mut video: H264Encoder,
    mut audio: AacEncoder,
    source: Source,
    scale: usize,
) -> anyhow::Result<u64> {
    let mut mp4 = Mp4Writer::new(BufWriter::with_capacity(1 << 20, file))?;
    if let Some(format) = audio.format() {
        mp4.layout().set_audio(format);
    }
    for next in ready {
        match next {
            Ready::Picture(mut yuv, repeat) => {
                for copy in 0..repeat {
                    let frame = if copy == 0 {
                        video.encode_planes(&mut yuv)?
                    } else {
                        video.encode_again()?
                    };
                    write_video(&mut mp4, frame, source, scale)?;
                }
                let _ = spare.send(yuv);
            }
            #[cfg(feature = "nds")]
            Ready::Again(repeat) => {
                if mp4.layout().has_video_format() {
                    for _ in 0..repeat {
                        let frame = video.encode_again()?;
                        write_video(&mut mp4, frame, source, scale)?;
                    }
                }
            }
            Ready::Late(repeat) => mp4.stretch_video(source.frame_ticks * repeat),
            Ready::Audio(samples) => {
                for packet in audio.push(&samples)? {
                    mp4.write_audio(&packet.data, packet.duration)?;
                }
            }
        }
    }
    for packet in audio.finish()? {
        mp4.write_audio(&packet.data, packet.duration)?;
    }
    if let Some(format) = audio.format() {
        mp4.layout().set_audio(format);
    }
    let writer = mp4.finish()?;
    let file = writer.into_inner().map_err(|e| e.into_error())?;
    file.sync_all()?;
    Ok(file.metadata()?.len())
}

fn write_video(
    mp4: &mut Mp4Writer<BufWriter<File>>,
    frame: super::h264::EncodedFrame,
    source: Source,
    scale: usize,
) -> anyhow::Result<()> {
    if !mp4.layout().has_video_format() {
        let (Some(sps), Some(pps)) = (frame.sps, frame.pps) else {
            anyhow::bail!("the video encoder sent no stream header");
        };
        mp4.layout().set_video(VideoFormat {
            width: (source.width * scale) as u16,
            height: (source.height * scale) as u16,
            timescale: source.timescale,
            avc_config: avc_config(&sps, &pps),
        });
    }
    Ok(mp4.write_video(&frame.sample, source.frame_ticks, frame.keyframe)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_millis(16);

    struct Feed {
        keep_up: KeepUp,
        now: Instant,
    }

    impl Feed {
        fn new(waiting: usize) -> Self {
            Self {
                keep_up: KeepUp::new(Arc::new(AtomicUsize::new(waiting)), PERIOD),
                now: Instant::now(),
            }
        }

        fn waiting(&self) -> usize {
            self.keep_up.waiting.load(Ordering::Relaxed)
        }

        fn pass(&mut self, message: Message, every: Duration) -> Message {
            self.now += every;
            self.keep_up.pass(message, self.now)
        }
    }

    fn picture(repeat: u32) -> Message {
        Message::Frame(vec![0; 4], repeat)
    }

    #[test]
    fn an_encoder_at_half_speed_keeps_about_every_other_picture() {
        let mut feed = Feed::new(BEHIND_PICTURES + 200);
        let mut took = Duration::ZERO;
        let mut kept = 0;
        for _ in 0..160 {
            match feed.pass(picture(2), took) {
                Message::Frame(_, 2) => {
                    kept += 1;
                    took = PERIOD * 2;
                }
                Message::Late(2) => took = Duration::ZERO,
                _ => panic!("a picture stays a picture or a held one"),
            }
        }
        assert!((60..=100).contains(&kept), "{kept} of 160 kept");
    }

    #[test]
    fn a_backlog_the_encoder_can_clear_is_kept_whole() {
        let mut feed = Feed::new(BEHIND_PICTURES + 40);
        for _ in 0..40 {
            assert!(matches!(
                feed.pass(picture(1), PERIOD / 4),
                Message::Frame(_, 1)
            ));
        }
    }

    #[test]
    fn the_newest_picture_is_never_skipped() {
        let mut feed = Feed::new(BEHIND_PICTURES + 20);
        let slow = PERIOD * 3;
        while feed.waiting() > BEHIND_PICTURES {
            feed.pass(picture(1), slow);
        }
        for _ in 0..BEHIND_PICTURES {
            assert!(matches!(feed.pass(picture(1), slow), Message::Frame(_, 1)));
        }
        assert_eq!(feed.waiting(), 0);
    }

    #[test]
    fn the_first_picture_is_always_kept() {
        let mut feed = Feed::new(BEHIND_PICTURES * 2);
        feed.keep_up.pace = PERIOD * 4;
        assert!(matches!(
            feed.pass(picture(3), PERIOD * 4),
            Message::Frame(_, 3)
        ));
    }

    #[test]
    fn sound_never_counts_as_a_picture() {
        let mut feed = Feed::new(BEHIND_PICTURES * 2);
        assert!(matches!(
            feed.pass(Message::Audio(vec![0.0; 2]), PERIOD),
            Message::Audio(_)
        ));
        assert_eq!(feed.waiting(), BEHIND_PICTURES * 2);
    }
}
