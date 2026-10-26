#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Seek, SeekFrom, Write};

pub struct VideoFormat {
    pub width: u16,
    pub height: u16,
    pub timescale: u32,
    pub avc_config: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn avc_config(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(sps.get(1..4).unwrap_or(&[66, 0xC0, 31]));
    out.push(0xFF);
    out.push(0xE1);
    u16s(&mut out, &[sps.len() as u16]);
    out.extend_from_slice(sps);
    out.push(1);
    u16s(&mut out, &[pps.len() as u16]);
    out.extend_from_slice(pps);
    out
}

pub enum AudioCodec {
    Aac {
        config: Vec<u8>,
    },
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Opus,
}

pub struct AudioFormat {
    pub codec: AudioCodec,
    pub sample_rate: u32,
    pub channels: u16,
    pub priming: u32,
    pub frames: u64,
}

#[derive(Default)]
struct Track {
    sizes: Vec<u32>,
    durations: Vec<(u32, u32)>,
    sync: Vec<u32>,
    all_sync: bool,
    chunks: Vec<(u64, u32)>,
    total: u64,
}

impl Track {
    fn push(&mut self, size: u32, duration: u32, sync: bool, offset: u64, continues: bool) {
        self.sizes.push(size);
        if sync {
            self.sync.push(self.sizes.len() as u32);
        }
        match self.durations.last_mut() {
            Some((count, delta)) if *delta == duration => *count += 1,
            _ => self.durations.push((1, duration)),
        }
        self.total += duration as u64;
        match self.chunks.last_mut() {
            Some((_, count)) if continues => *count += 1,
            _ => self.chunks.push((offset, 1)),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Last {
    None,
    Video,
    Audio,
}

pub struct Mp4Layout {
    pos: u64,
    video: Option<VideoFormat>,
    audio: Option<AudioFormat>,
    video_track: Track,
    audio_track: Track,
    last: Last,
}

const FTYP: &[u8] = b"isom\0\0\x02\0isomiso2avc1mp41";
const HEAD_LEN: u64 = 8 + FTYP.len() as u64 + 16;

impl Default for Mp4Layout {
    fn default() -> Self {
        Self {
            pos: HEAD_LEN,
            video: None,
            audio: None,
            video_track: Track::default(),
            audio_track: Track {
                all_sync: true,
                ..Track::default()
            },
            last: Last::None,
        }
    }
}

impl Mp4Layout {
    pub fn set_video(&mut self, format: VideoFormat) {
        self.video = Some(format);
    }

    pub fn set_audio(&mut self, format: AudioFormat) {
        self.audio = Some(format);
    }

    pub fn has_video_format(&self) -> bool {
        self.video.is_some()
    }

    pub fn add_video(&mut self, size: usize, duration: u32, sync: bool) {
        let continues = self.last == Last::Video;
        self.video_track
            .push(size as u32, duration, sync, self.pos, continues);
        self.last = Last::Video;
        self.pos += size as u64;
    }

    pub fn add_audio(&mut self, size: usize, duration: u32) {
        let continues = self.last == Last::Audio;
        self.audio_track
            .push(size as u32, duration, true, self.pos, continues);
        self.last = Last::Audio;
        self.pos += size as u64;
    }

    pub fn head(&self) -> Vec<u8> {
        let mut head = Vec::new();
        boxed(&mut head, b"ftyp", |b| b.extend_from_slice(FTYP));
        head.extend_from_slice(&1u32.to_be_bytes());
        head.extend_from_slice(b"mdat");
        head.extend_from_slice(&(self.pos - head.len() as u64 + 8).to_be_bytes());
        head
    }

    pub fn moov(&self) -> Vec<u8> {
        let movie_scale = 1000u32;
        let to_movie = |track: &Track, scale: u32| -> u64 {
            (track.total * movie_scale as u64).div_ceil(scale.max(1) as u64)
        };
        let video_len = self
            .video
            .as_ref()
            .map_or(0, |v| to_movie(&self.video_track, v.timescale));
        let audio_len = self.audio.as_ref().map_or(0, |a| {
            let full = self.audio_track.total.saturating_sub(a.priming as u64);
            let kept = if a.frames > 0 {
                a.frames.min(full)
            } else {
                full
            };
            (kept * movie_scale as u64).div_ceil(a.sample_rate.max(1) as u64)
        });
        let mut next_id = 1;
        let mut out = Vec::new();
        boxed(&mut out, b"moov", |b| {
            full_box(b, b"mvhd", 0, 0, |b| {
                u32s(b, &[0, 0, movie_scale, video_len.max(audio_len) as u32]);
                u32s(b, &[0x0001_0000]);
                b.extend_from_slice(&[1, 0]);
                b.extend_from_slice(&[0; 10]);
                matrix(b);
                b.extend_from_slice(&[0; 24]);
                u32s(b, &[3]);
            });
            if let Some(video) = &self.video {
                self.video_trak(b, video, next_id, video_len);
                next_id += 1;
            }
            if let Some(audio) = &self.audio {
                self.audio_trak(b, audio, next_id, audio_len);
            }
        });
        out
    }

    fn video_trak(&self, b: &mut Vec<u8>, video: &VideoFormat, id: u32, movie_len: u64) {
        boxed(b, b"trak", |b| {
            tkhd(b, id, movie_len, false, video.width, video.height);
            boxed(b, b"mdia", |b| {
                mdhd(b, video.timescale, self.video_track.total);
                hdlr(b, b"vide", b"VideoHandler");
                boxed(b, b"minf", |b| {
                    full_box(b, b"vmhd", 0, 1, |b| b.extend_from_slice(&[0; 8]));
                    dinf(b);
                    boxed(b, b"stbl", |b| {
                        full_box(b, b"stsd", 0, 0, |b| {
                            u32s(b, &[1]);
                            avc1(b, video);
                        });
                        sample_tables(b, &self.video_track);
                    });
                });
            });
        });
    }

    fn audio_trak(&self, b: &mut Vec<u8>, audio: &AudioFormat, id: u32, movie_len: u64) {
        boxed(b, b"trak", |b| {
            tkhd(b, id, movie_len, true, 0, 0);
            if audio.priming > 0 || audio.frames > 0 {
                boxed(b, b"edts", |b| {
                    full_box(b, b"elst", 0, 0, |b| {
                        u32s(b, &[1, movie_len as u32, audio.priming, 0x0001_0000]);
                    });
                });
            }
            boxed(b, b"mdia", |b| {
                mdhd(b, audio.sample_rate, self.audio_track.total);
                hdlr(b, b"soun", b"SoundHandler");
                boxed(b, b"minf", |b| {
                    full_box(b, b"smhd", 0, 0, |b| b.extend_from_slice(&[0; 4]));
                    dinf(b);
                    boxed(b, b"stbl", |b| {
                        full_box(b, b"stsd", 0, 0, |b| {
                            u32s(b, &[1]);
                            audio_entry(b, audio);
                        });
                        sample_tables(b, &self.audio_track);
                    });
                });
            });
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Mp4Writer<W: Write + Seek> {
    out: W,
    layout: Mp4Layout,
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write + Seek> Mp4Writer<W> {
    pub fn new(mut out: W) -> io::Result<Self> {
        let layout = Mp4Layout::default();
        out.write_all(&layout.head())?;
        Ok(Self { out, layout })
    }

    pub fn layout(&mut self) -> &mut Mp4Layout {
        &mut self.layout
    }

    pub fn write_video(&mut self, data: &[u8], duration: u32, sync: bool) -> io::Result<()> {
        self.layout.add_video(data.len(), duration, sync);
        self.out.write_all(data)
    }

    pub fn write_audio(&mut self, data: &[u8], duration: u32) -> io::Result<()> {
        self.layout.add_audio(data.len(), duration);
        self.out.write_all(data)
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.write_all(&self.layout.moov())?;
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&self.layout.head())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

fn boxed(out: &mut Vec<u8>, kind: &[u8; 4], body: impl FnOnce(&mut Vec<u8>)) {
    let start = out.len();
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(kind);
    body(out);
    let size = (out.len() - start) as u32;
    out[start..start + 4].copy_from_slice(&size.to_be_bytes());
}

fn full_box(
    out: &mut Vec<u8>,
    kind: &[u8; 4],
    version: u8,
    flags: u32,
    body: impl FnOnce(&mut Vec<u8>),
) {
    boxed(out, kind, |b| {
        b.push(version);
        b.extend_from_slice(&flags.to_be_bytes()[1..]);
        body(b);
    });
}

fn u32s(out: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        out.extend_from_slice(&v.to_be_bytes());
    }
}

fn u16s(out: &mut Vec<u8>, values: &[u16]) {
    for v in values {
        out.extend_from_slice(&v.to_be_bytes());
    }
}

fn matrix(out: &mut Vec<u8>) {
    u32s(
        out,
        &[0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000],
    );
}

fn tkhd(out: &mut Vec<u8>, id: u32, duration: u64, audio: bool, width: u16, height: u16) {
    full_box(out, b"tkhd", 0, 3, |b| {
        u32s(b, &[0, 0, id, 0, duration as u32, 0, 0]);
        u16s(b, &[0, if audio { 1 } else { 0 }]);
        u16s(b, &[if audio { 0x0100 } else { 0 }, 0]);
        matrix(b);
        u32s(b, &[(width as u32) << 16, (height as u32) << 16]);
    });
}

fn mdhd(out: &mut Vec<u8>, timescale: u32, duration: u64) {
    full_box(out, b"mdhd", 0, 0, |b| {
        u32s(b, &[0, 0, timescale, duration as u32]);
        u16s(b, &[0x55C4, 0]);
    });
}

fn hdlr(out: &mut Vec<u8>, kind: &[u8; 4], name: &[u8]) {
    full_box(out, b"hdlr", 0, 0, |b| {
        u32s(b, &[0]);
        b.extend_from_slice(kind);
        u32s(b, &[0, 0, 0]);
        b.extend_from_slice(name);
        b.push(0);
    });
}

fn dinf(out: &mut Vec<u8>) {
    boxed(out, b"dinf", |b| {
        full_box(b, b"dref", 0, 0, |b| {
            u32s(b, &[1]);
            full_box(b, b"url ", 0, 1, |_| {});
        });
    });
}

fn avc1(out: &mut Vec<u8>, video: &VideoFormat) {
    boxed(out, b"avc1", |b| {
        b.extend_from_slice(&[0; 6]);
        u16s(b, &[1, 0, 0]);
        u32s(b, &[0, 0, 0]);
        u16s(b, &[video.width, video.height]);
        u32s(b, &[0x0048_0000, 0x0048_0000, 0]);
        u16s(b, &[1]);
        b.extend_from_slice(&[0; 32]);
        u16s(b, &[0x0018, 0xFFFF]);
        boxed(b, b"avcC", |b| b.extend_from_slice(&video.avc_config));
        boxed(b, b"colr", |b| {
            b.extend_from_slice(b"nclx");
            u16s(b, &[1, 1, 6]);
            b.push(0);
        });
        boxed(b, b"pasp", |b| u32s(b, &[1, 1]));
    });
}

fn audio_entry(out: &mut Vec<u8>, audio: &AudioFormat) {
    let kind = match audio.codec {
        AudioCodec::Aac { .. } => b"mp4a",
        AudioCodec::Opus => b"Opus",
    };
    boxed(out, kind, |b| {
        b.extend_from_slice(&[0; 6]);
        u16s(b, &[1]);
        u32s(b, &[0, 0]);
        u16s(b, &[audio.channels, 16, 0, 0]);
        let rate = if audio.sample_rate > 0xFFFF {
            0
        } else {
            audio.sample_rate
        };
        u32s(b, &[rate << 16]);
        match &audio.codec {
            AudioCodec::Aac { config } => esds(b, config),
            AudioCodec::Opus => boxed(b, b"dOps", |b| {
                b.push(0);
                b.push(audio.channels as u8);
                u16s(b, &[audio.priming as u16]);
                u32s(b, &[audio.sample_rate]);
                u16s(b, &[0]);
                b.push(0);
            }),
        }
    });
}

fn descriptor(out: &mut Vec<u8>, tag: u8, body: &[u8]) {
    out.push(tag);
    let len = body.len() as u32;
    out.extend_from_slice(&[
        0x80 | ((len >> 21) & 0x7F) as u8,
        0x80 | ((len >> 14) & 0x7F) as u8,
        0x80 | ((len >> 7) & 0x7F) as u8,
        (len & 0x7F) as u8,
    ]);
    out.extend_from_slice(body);
}

fn esds(out: &mut Vec<u8>, config: &[u8]) {
    full_box(out, b"esds", 0, 0, |b| {
        let mut specific = Vec::new();
        descriptor(&mut specific, 0x05, config);
        let mut decoder = vec![0x40, 0x15, 0, 0, 0];
        u32s(&mut decoder, &[128_000, 128_000]);
        decoder.extend_from_slice(&specific);
        let mut es = vec![0, 1, 0];
        descriptor(&mut es, 0x04, &decoder);
        descriptor(&mut es, 0x06, &[0x02]);
        descriptor(b, 0x03, &es);
    });
}

fn sample_tables(out: &mut Vec<u8>, track: &Track) {
    full_box(out, b"stts", 0, 0, |b| {
        u32s(b, &[track.durations.len() as u32]);
        for (count, delta) in &track.durations {
            u32s(b, &[*count, *delta]);
        }
    });
    if !track.all_sync {
        full_box(out, b"stss", 0, 0, |b| {
            u32s(b, &[track.sync.len() as u32]);
            u32s(b, &track.sync);
        });
    }
    full_box(out, b"stsc", 0, 0, |b| {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (index, (_, count)) in track.chunks.iter().enumerate() {
            if runs.last().is_none_or(|(_, c)| c != count) {
                runs.push((index as u32 + 1, *count));
            }
        }
        u32s(b, &[runs.len() as u32]);
        for (first, count) in runs {
            u32s(b, &[first, count, 1]);
        }
    });
    full_box(out, b"stsz", 0, 0, |b| {
        u32s(b, &[0, track.sizes.len() as u32]);
        u32s(b, &track.sizes);
    });
    full_box(out, b"co64", 0, 0, |b| {
        u32s(b, &[track.chunks.len() as u32]);
        for (offset, _) in &track.chunks {
            b.extend_from_slice(&offset.to_be_bytes());
        }
    });
}
