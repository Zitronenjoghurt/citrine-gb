#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub offset: u64,
    pub size: u32,
    pub time: u64,
    pub sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    Avc { config: Vec<u8> },
    Aac { config: Vec<u8> },
    Opus,
    Other,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub codec: Codec,
    pub timescale: u32,
    pub duration: u64,
    pub skip: u64,
    pub sample_rate: u32,
    pub samples: Vec<Sample>,
}

impl Track {
    pub fn seconds(&self, time: u64) -> f64 {
        time as f64 / self.timescale.max(1) as f64
    }

    pub fn ticks(&self, seconds: f64) -> u64 {
        (seconds.max(0.0) * self.timescale as f64) as u64
    }

    pub fn length(&self) -> f64 {
        self.seconds(self.duration.saturating_sub(self.skip))
    }

    pub fn index_at(&self, seconds: f64) -> usize {
        let ticks = self.ticks(seconds) + self.skip;
        self.samples
            .partition_point(|s| s.time <= ticks)
            .saturating_sub(1)
    }

    pub fn sync_before(&self, index: usize) -> usize {
        self.samples[..=index.min(self.samples.len().saturating_sub(1))]
            .iter()
            .rposition(|s| s.sync)
            .unwrap_or(0)
    }

    pub fn data<'a>(&self, bytes: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let sample = self.samples.get(index)?;
        let start = usize::try_from(sample.offset).ok()?;
        bytes.get(start..start.checked_add(sample.size as usize)?)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Movie {
    pub video: Option<Track>,
    pub audio: Option<Track>,
}

impl Movie {
    pub fn length(&self) -> f64 {
        let video = self.video.as_ref().map_or(0.0, Track::length);
        let audio = self.audio.as_ref().map_or(0.0, Track::length);
        video.max(audio)
    }
}

pub fn read(bytes: &[u8]) -> Option<Movie> {
    let moov = children(bytes).find(|(kind, _)| kind == b"moov")?.1;
    let mut movie = Movie::default();
    for (_, trak) in children(moov).filter(|(kind, _)| kind == b"trak") {
        let Some(track) = track(trak) else {
            continue;
        };
        match track.codec {
            Codec::Avc { .. } if movie.video.is_none() => movie.video = Some(track),
            Codec::Aac { .. } | Codec::Opus if movie.audio.is_none() => movie.audio = Some(track),
            _ => {}
        }
    }
    (movie.video.is_some() || movie.audio.is_some()).then_some(movie)
}

fn track(trak: &[u8]) -> Option<Track> {
    let mdia = child(trak, b"mdia")?;
    let mdhd = child(mdia, b"mdhd")?;
    let (timescale, duration) = match mdhd.first()? {
        1 => (be32(mdhd, 20)?, be64(mdhd, 24)?),
        _ => (be32(mdhd, 12)?, be32(mdhd, 16)? as u64),
    };
    let stbl = child(child(mdia, b"minf")?, b"stbl")?;
    let stsd = child(stbl, b"stsd")?;
    let (kind, entry) = children(stsd.get(8..)?).next()?;
    let mut sample_rate = timescale;
    let codec = match &kind {
        b"avc1" => Codec::Avc {
            config: child(entry.get(78..)?, b"avcC")?.to_vec(),
        },
        b"mp4a" | b"Opus" => {
            let rate = be32(entry, 24)? >> 16;
            if rate > 0 {
                sample_rate = rate;
            }
            let boxes = entry.get(28..)?;
            if &kind == b"Opus" {
                Codec::Opus
            } else {
                Codec::Aac {
                    config: specific_info(child(boxes, b"esds")?.get(4..)?)?,
                }
            }
        }
        _ => Codec::Other,
    };
    let skip = child(trak, b"edts")
        .and_then(|edts| child(edts, b"elst"))
        .and_then(|elst| match elst.first()? {
            1 => be64(elst, 16),
            _ => be32(elst, 12).map(u64::from),
        })
        .unwrap_or(0);
    let samples = samples(stbl)?;
    Some(Track {
        codec,
        timescale,
        duration,
        skip,
        sample_rate,
        samples,
    })
}

fn samples(stbl: &[u8]) -> Option<Vec<Sample>> {
    let stsz = child(stbl, b"stsz")?;
    let fixed = be32(stsz, 4)?;
    let count = be32(stsz, 8)? as usize;
    let sizes: Vec<u32> = if fixed > 0 {
        vec![fixed; count]
    } else {
        (0..count)
            .map(|i| be32(stsz, 12 + i * 4))
            .collect::<Option<_>>()?
    };

    let offsets: Vec<u64> = if let Some(co64) = child(stbl, b"co64") {
        (0..be32(co64, 4)? as usize)
            .map(|i| be64(co64, 8 + i * 8))
            .collect::<Option<_>>()?
    } else {
        let stco = child(stbl, b"stco")?;
        (0..be32(stco, 4)? as usize)
            .map(|i| be32(stco, 8 + i * 4).map(u64::from))
            .collect::<Option<_>>()?
    };

    let stsc = child(stbl, b"stsc")?;
    let runs: Vec<(u32, u32)> = (0..be32(stsc, 4)? as usize)
        .map(|i| Some((be32(stsc, 8 + i * 12)?, be32(stsc, 12 + i * 12)?)))
        .collect::<Option<_>>()?;

    let stts = child(stbl, b"stts")?;
    let mut times = Vec::with_capacity(count);
    let mut time = 0u64;
    for i in 0..be32(stts, 4)? as usize {
        let (n, delta) = (be32(stts, 8 + i * 8)?, be32(stts, 12 + i * 8)?);
        for _ in 0..n {
            times.push(time);
            time += delta as u64;
        }
    }

    let sync: Option<Vec<u32>> = child(stbl, b"stss").and_then(|stss| {
        (0..be32(stss, 4)? as usize)
            .map(|i| be32(stss, 8 + i * 4))
            .collect()
    });

    let mut out = Vec::with_capacity(count);
    let mut index = 0usize;
    for (chunk, offset) in offsets.iter().enumerate() {
        let number = chunk as u32 + 1;
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= number)
            .map_or(0, |(_, n)| *n);
        let mut at = *offset;
        for _ in 0..per {
            let (Some(size), Some(time)) = (sizes.get(index), times.get(index)) else {
                break;
            };
            let number = index as u32 + 1;
            out.push(Sample {
                offset: at,
                size: *size,
                time: *time,
                sync: sync
                    .as_ref()
                    .is_none_or(|s| s.binary_search(&number).is_ok()),
            });
            at += *size as u64;
            index += 1;
        }
    }
    Some(out)
}

fn specific_info(mut es: &[u8]) -> Option<Vec<u8>> {
    loop {
        let tag = *es.first()?;
        let (len, header) = descriptor_len(es.get(1..)?)?;
        let body = es.get(1 + header..1 + header + len)?;
        match tag {
            0x03 => {
                let flags = *body.get(2)?;
                let mut skip = 3;
                if flags & 0x80 != 0 {
                    skip += 2;
                }
                if flags & 0x40 != 0 {
                    skip += 1 + *body.get(skip)? as usize;
                }
                if flags & 0x20 != 0 {
                    skip += 2;
                }
                es = body.get(skip..)?;
            }
            0x04 => es = body.get(13..)?,
            0x05 => return Some(body.to_vec()),
            _ => return None,
        }
    }
}

fn descriptor_len(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut len = 0usize;
    for (i, byte) in bytes.iter().take(4).enumerate() {
        len = (len << 7) | (byte & 0x7F) as usize;
        if byte & 0x80 == 0 {
            return Some((len, i + 1));
        }
    }
    None
}

fn child<'a>(bytes: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    children(bytes)
        .find(|(k, _)| k == kind)
        .map(|(_, body)| body)
}

fn children(bytes: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut rest = bytes;
    std::iter::from_fn(move || {
        let size = be32(rest, 0)? as u64;
        let kind: [u8; 4] = rest.get(4..8)?.try_into().ok()?;
        let (start, size) = match size {
            1 => (16, be64(rest, 8)?),
            0 => (8, rest.len() as u64),
            n => (8, n),
        };
        let size = usize::try_from(size).ok()?;
        let body = rest.get(start..size)?;
        rest = rest.get(size..)?;
        Some((kind, body))
    })
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn be64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

pub fn annex_b(sample: &[u8], out: &mut Vec<u8>) {
    let mut rest = sample;
    while let Some(len) = be32(rest, 0) {
        let Some(unit) = rest.get(4..4 + len as usize) else {
            break;
        };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(unit);
        rest = &rest[4 + len as usize..];
    }
}

pub fn parameter_sets(config: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 5;
    for mask in [0x1F, 0xFF] {
        let Some(count) = config.get(at) else {
            break;
        };
        at += 1;
        for _ in 0..(count & mask) {
            let Some(len) = be16(config, at) else {
                return out;
            };
            let Some(unit) = config.get(at + 2..at + 2 + len as usize) else {
                return out;
            };
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(unit);
            at += 2 + len as usize;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::recording::mp4::{
        AudioCodec, AudioFormat, Mp4Writer, VideoFormat, avc_config,
    };
    use std::io::Cursor;

    fn written(audio: AudioCodec) -> Vec<u8> {
        let mut writer = Mp4Writer::new(Cursor::new(Vec::new())).unwrap();
        writer.layout().set_video(VideoFormat {
            width: 320,
            height: 288,
            timescale: 1000,
            avc_config: avc_config(&[0x67, 1, 2, 3], &[0x68, 4]),
        });
        writer.layout().set_audio(AudioFormat {
            codec: audio,
            sample_rate: 48_000,
            channels: 2,
            priming: 1024,
            frames: 0,
        });
        for i in 0..6u8 {
            writer
                .write_video(&[0, 0, 0, 2, i, i], 20, i % 3 == 0)
                .unwrap();
            writer.write_audio(&[9, i, i, i], 1024).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn a_written_recording_reads_back_sample_by_sample() {
        let bytes = written(AudioCodec::Aac {
            config: vec![0x11, 0x90],
        });
        let movie = read(&bytes).unwrap();
        let video = movie.video.unwrap();
        assert_eq!(video.timescale, 1000);
        assert_eq!(video.samples.len(), 6);
        assert_eq!(video.data(&bytes, 4), Some(&[0, 0, 0, 2, 4, 4][..]));
        assert_eq!(video.samples[5].time, 100);
        let syncs: Vec<bool> = video.samples.iter().map(|s| s.sync).collect();
        assert_eq!(syncs, [true, false, false, true, false, false]);
        assert_eq!(video.sync_before(5), 3);
        assert_eq!(video.index_at(0.065), 3);
        let Codec::Avc { config } = &video.codec else {
            panic!("not avc");
        };
        assert_eq!(
            parameter_sets(config),
            [0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 4]
        );

        let audio = movie.audio.unwrap();
        assert_eq!(
            audio.codec,
            Codec::Aac {
                config: vec![0x11, 0x90]
            }
        );
        assert_eq!((audio.sample_rate, audio.skip), (48_000, 1024));
        assert_eq!(audio.data(&bytes, 2), Some(&[9, 2, 2, 2][..]));
    }

    #[test]
    fn opus_sound_is_named_and_other_bytes_are_refused() {
        let movie = read(&written(AudioCodec::Opus)).unwrap();
        assert_eq!(movie.audio.unwrap().codec, Codec::Opus);
        assert!(read(b"not a movie").is_none());
        assert!(read(&[0, 0, 0, 8, b'm', b'o', b'o', b'v']).is_none());
    }

    #[test]
    fn length_prefixed_units_become_start_codes() {
        let mut out = Vec::new();
        annex_b(&[0, 0, 0, 2, 7, 8, 0, 0, 0, 1, 9], &mut out);
        assert_eq!(out, [0, 0, 0, 1, 7, 8, 0, 0, 0, 1, 9]);
    }
}
