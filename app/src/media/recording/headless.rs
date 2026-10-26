use super::h264::{split_annex_b, upscale_to_i420};
use super::{FinishedRecording, Outcome, ScreenRecorder, StopReason, fps};
use crate::media::{Frame, MediaKind, NewMedia};
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const RATE: u32 = 48_000;

fn temp_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("citrine-recording-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn new_media(gb: &GameBoy) -> NewMedia {
    NewMedia {
        kind: MediaKind::Recording,
        game: gb.cartridge.header.title.trim().to_string(),
        rom_key: None,
        model: gb.model,
        created_at: 0,
        width: 160,
        height: 144,
        length_ms: None,
        frame: Frame {
            width: 160,
            height: 144,
            rgba: gb.frame().as_slice().to_vec(),
        },
    }
}

fn game(rom: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.apu.set_sample_rate(RATE);
    gb.load_rom(&Rom::new(rom)).unwrap();
    gb
}

fn play(gb: &mut GameBoy, frame: usize) {
    if (frame / 30) % 4 == 1 {
        gb.joypad.press(JoypadState::START);
    } else {
        gb.joypad.release(JoypadState::START);
    }
    gb.run_frame();
}

struct Recorded {
    done: FinishedRecording,
    sound_frames: u64,
    frames: Vec<Vec<u8>>,
}

fn record(rom: &[u8], name: &str, frames: usize) -> Recorded {
    record_with(rom, frames, |recorder, gb| {
        recorder.start_into(temp_file(name), new_media(gb), RATE, super::DEFAULT_SCALE)
    })
}

fn record_with(
    rom: &[u8],
    frames: usize,
    start: impl FnOnce(&mut ScreenRecorder, &GameBoy) -> anyhow::Result<()>,
) -> Recorded {
    let mut gb = game(rom);
    let mut recorder = ScreenRecorder::default();
    start(&mut recorder, &gb).unwrap();
    let mut sound_frames = 0;
    let mut shown = Vec::new();
    for frame in 0..frames {
        play(&mut gb, frame);
        sound_frames += gb.apu.audio_buffer.len() as u64 / 2;
        recorder.capture(gb.frame().as_slice(), &gb.apu.audio_buffer, RATE);
        gb.apu.audio_buffer.clear();
        shown.push(gb.frame().as_slice().to_vec());
    }
    recorder.stop();
    let mut outcomes = recorder.wait();
    assert_eq!(outcomes.len(), 1);
    match outcomes.remove(0) {
        Outcome::Saved(done) => Recorded {
            done: *done,
            sound_frames,
            frames: shown,
        },
        Outcome::Empty => panic!("nothing was recorded"),
        Outcome::Failed(err) => panic!("recording failed: {err}"),
    }
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let se: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| (*x as f64 - *y as f64).powi(2))
        .sum();
    10.0 * (255.0f64.powi(2) / (se / a.len() as f64).max(1e-9)).log10()
}

#[test]
fn five_seconds_of_homebrew_make_an_mp4_that_plays() {
    let frames = 300;
    let rec = record(
        include_bytes!("../../../homebrew/tobu.gb"),
        "tobu.mp4",
        frames,
    );
    let path = &rec.done.output.path;
    let size = std::fs::metadata(path).unwrap().len();
    assert_eq!(size, rec.done.output.size);

    let file = std::fs::File::open(path).unwrap();
    let mut mp4 = mp4::Mp4Reader::read_header(BufReader::new(file), size).unwrap();
    let mut tracks: Vec<(u32, mp4::TrackType)> = mp4
        .tracks()
        .values()
        .map(|t| (t.track_id(), t.track_type().unwrap()))
        .collect();
    tracks.sort_by_key(|(id, _)| *id);
    assert_eq!(
        tracks,
        vec![(1, mp4::TrackType::Video), (2, mp4::TrackType::Audio)]
    );

    let video = &mp4.tracks()[&1];
    assert_eq!(video.media_type().unwrap(), mp4::MediaType::H264);
    assert_eq!((video.width(), video.height()), (640, 576));
    assert_eq!(video.sample_count() as u64, rec.done.frames);
    assert!(
        rec.done.frames.abs_diff(frames as u64) <= 1,
        "{} frames for {frames} emulated",
        rec.done.frames
    );
    let video_secs = video.duration().as_secs_f64();
    let expected = rec.done.frames as f64 / fps();
    assert!((video_secs - expected).abs() < 0.001, "{video_secs} s");

    let audio = &mp4.tracks()[&2];
    assert_eq!(audio.media_type().unwrap(), mp4::MediaType::AAC);
    assert_eq!(audio.timescale(), RATE);
    assert_eq!(audio.channel_config().unwrap(), mp4::ChannelConfig::Stereo);
    let edit = &audio
        .trak
        .edts
        .as_ref()
        .unwrap()
        .elst
        .as_ref()
        .unwrap()
        .entries[0];
    assert_eq!(edit.media_time, 1024);
    let audio_secs = edit.segment_duration as f64 / mp4.timescale() as f64;
    let sound_secs = rec.sound_frames as f64 / RATE as f64;
    assert!((audio_secs - sound_secs).abs() < 0.002, "{audio_secs} s");
    assert!(
        (audio_secs - video_secs).abs() < 1.0 / fps(),
        "sound {audio_secs} s against video {video_secs} s"
    );
    let audio_media = audio.duration().as_secs_f64();
    assert!(audio_media >= sound_secs + 1024.0 / RATE as f64);

    let mut decoder = openh264::decoder::Decoder::new().unwrap();
    let mut i420 = vec![0; 640 * 576 * 3 / 2];
    let mut decoded = 0;
    let mut worst = f64::MAX;
    let sample_count = video.sample_count();
    let parameter_sets = [
        video.sequence_parameter_set().unwrap().to_vec(),
        video.picture_parameter_set().unwrap().to_vec(),
    ];
    for id in 1..=sample_count {
        let sample = mp4.read_sample(1, id).unwrap().unwrap();
        if id == 1 {
            assert!(sample.is_sync, "the first frame is a key frame");
        }
        let mut annex_b = Vec::new();
        if id == 1 {
            for unit in &parameter_sets {
                annex_b.extend_from_slice(&[0, 0, 0, 1]);
                annex_b.extend_from_slice(unit);
            }
        }
        let mut rest = &sample.bytes[..];
        while rest.len() >= 4 {
            let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            annex_b.extend_from_slice(&[0, 0, 0, 1]);
            annex_b.extend_from_slice(&rest[4..4 + len]);
            rest = &rest[4 + len..];
        }
        assert!(rest.is_empty(), "sample {id} ends cleanly");
        assert!(!split_annex_b(&annex_b).is_empty());
        if let Some(image) = decoder.decode(&annex_b).unwrap() {
            use openh264::formats::YUVSource;
            assert_eq!(image.dimensions(), (640, 576));
            let (stride, _, _) = image.strides();
            let luma: Vec<u8> = image
                .y()
                .chunks(stride)
                .take(576)
                .flat_map(|row| &row[..640])
                .copied()
                .collect();
            let index = id as usize - 1;
            let nearest = index.saturating_sub(1)..=(index + 1).min(rec.frames.len() - 1);
            let best = nearest
                .map(|i| {
                    upscale_to_i420(&rec.frames[i], 160, 144, 4, &mut i420);
                    psnr(&luma, &i420[..640 * 576])
                })
                .fold(f64::MIN, f64::max);
            worst = worst.min(best);
            decoded += 1;
        }
    }
    assert_eq!(decoded, sample_count);
    assert!(worst > 45.0, "worst frame luma PSNR {worst:.1} dB");

    let thumbnail = &rec.done.thumbnail;
    assert_eq!(thumbnail.len(), 160 * 144 * 4);
    let middle = rec.frames.len() / 2;
    assert!(
        (middle - 60..=middle + 60).any(|i| rec.frames[i] == *thumbnail),
        "the thumbnail comes from the middle"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_paused_stretch_is_left_out() {
    let mut gb = game(include_bytes!("../../../homebrew/porklike.gb"));
    let mut recorder = ScreenRecorder::default();
    recorder
        .start_into(
            temp_file("paused.mp4"),
            new_media(&gb),
            RATE,
            super::DEFAULT_SCALE,
        )
        .unwrap();
    for frame in 0..60 {
        play(&mut gb, frame);
        recorder.capture(gb.frame().as_slice(), &gb.apu.audio_buffer, RATE);
        gb.apu.audio_buffer.clear();
    }
    let before = recorder.elapsed();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        recorder.elapsed(),
        before,
        "the clock stands still while paused"
    );
    for frame in 60..120 {
        play(&mut gb, frame);
        recorder.capture(gb.frame().as_slice(), &gb.apu.audio_buffer, RATE);
        gb.apu.audio_buffer.clear();
    }
    recorder.stop();
    let Outcome::Saved(done) = recorder.wait().remove(0) else {
        panic!("the recording was not saved");
    };
    assert!(done.frames.abs_diff(120) <= 1, "{} frames", done.frames);
    std::fs::remove_file(done.output.path).unwrap();
}

#[test]
fn a_recording_stopped_while_paused_is_not_kept() {
    let gb = game(include_bytes!("../../../homebrew/porklike.gb"));
    let mut recorder = ScreenRecorder::default();
    let path = temp_file("empty.mp4");
    recorder
        .start_into(path.clone(), new_media(&gb), RATE, super::DEFAULT_SCALE)
        .unwrap();
    recorder.stop();
    assert!(matches!(recorder.wait().remove(0), Outcome::Empty));
    assert!(!path.exists());
}

#[test]
fn a_new_sound_rate_ends_the_recording() {
    let mut gb = game(include_bytes!("../../../homebrew/porklike.gb"));
    let mut recorder = ScreenRecorder::default();
    recorder
        .start_into(
            temp_file("rate.mp4"),
            new_media(&gb),
            RATE,
            super::DEFAULT_SCALE,
        )
        .unwrap();
    for frame in 0..30 {
        play(&mut gb, frame);
        recorder.capture(gb.frame().as_slice(), &gb.apu.audio_buffer, RATE);
        gb.apu.audio_buffer.clear();
    }
    recorder.capture(gb.frame().as_slice(), &[], 44_100);
    assert!(!recorder.is_recording());
    assert_eq!(recorder.take_stopped(), Some(StopReason::SoundChanged));
    let Outcome::Saved(done) = recorder.wait().remove(0) else {
        panic!("the part before the change was not saved");
    };
    std::fs::remove_file(done.output.path).unwrap();
}

#[test]
#[ignore = "downloads Cisco's codec, run with --ignored"]
fn cisco_s_downloaded_codec_records_an_mp4() {
    use super::codec::{self, Codec, Status};
    let dir = std::env::temp_dir().join(format!("citrine-cisco-{}", std::process::id()));
    let binary = codec::binary_for(std::env::consts::OS, std::env::consts::ARCH)
        .expect("Cisco has no build for this system");
    let codec = Codec::new(dir.clone(), Some(binary));
    codec.download(codec::from_cisco());
    codec.wait();
    assert_eq!(codec.take_failure(), None);
    assert_eq!(codec.status(), Status::Ready);
    let api = codec.load().unwrap();
    assert!(matches!(api, openh264::OpenH264API::Libloading(_)));
    let rec = record_with(include_bytes!("../../../homebrew/tobu.gb"), 300, |r, gb| {
        r.start_with(api, temp_file("cisco.mp4"), new_media(gb), RATE)
    });
    assert!(rec.done.frames.abs_diff(300) <= 1);
    if let Some(keep) = std::env::var_os("CITRINE_KEEP_RECORDING") {
        std::fs::copy(&rec.done.output.path, keep).unwrap();
    }
    std::fs::remove_file(&rec.done.output.path).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "timing, run in release with --ignored --nocapture"]
fn recording_cost_per_frame() {
    let rom = include_bytes!("../../../homebrew/tobu.gb");
    let frames = 3600;
    let mut gb = game(rom);
    let mut plain = Duration::ZERO;
    for frame in 0..frames {
        let t = Instant::now();
        play(&mut gb, frame);
        gb.apu.audio_buffer.clear();
        plain += t.elapsed();
    }

    let mut gb = game(rom);
    let mut recorder = ScreenRecorder::default();
    let path = temp_file("cost.mp4");
    recorder
        .start_into(path.clone(), new_media(&gb), RATE, super::DEFAULT_SCALE)
        .unwrap();
    let mut recording = Duration::ZERO;
    let mut worst = Duration::ZERO;
    let started = Instant::now();
    for frame in 0..frames {
        let t = Instant::now();
        play(&mut gb, frame);
        recorder.capture(gb.frame().as_slice(), &gb.apu.audio_buffer, RATE);
        gb.apu.audio_buffer.clear();
        let spent = t.elapsed();
        recording += spent;
        worst = worst.max(spent);
        let due = Duration::from_secs_f64((frame + 1) as f64 / fps());
        if let Some(wait) = due.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    let t = Instant::now();
    recorder.stop();
    let Outcome::Saved(done) = recorder.wait().remove(0) else {
        panic!("not saved");
    };
    let drain = t.elapsed();
    println!(
        "per frame: {:.3} ms without recording, {:.3} ms recording (worst {:.3} ms); {} frames, {:.1} MB for {:.0} s; finishing took {:.0} ms",
        plain.as_secs_f64() * 1e3 / frames as f64,
        recording.as_secs_f64() * 1e3 / frames as f64,
        worst.as_secs_f64() * 1e3,
        done.frames,
        done.output.size as f64 / 1e6,
        done.length.as_secs_f64(),
        drain.as_secs_f64() * 1e3
    );
    std::fs::remove_file(path).unwrap();
}
