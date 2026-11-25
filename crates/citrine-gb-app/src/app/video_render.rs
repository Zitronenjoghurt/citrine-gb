use super::Citrine;
use crate::app::tabs::media::Rendering;
use crate::media::recording;
use crate::media::render::{Render, Rendered};
use crate::media::{Frame as MediaFrame, MediaId, MediaItem, MediaKind, NewMedia};
use crate::play::shelf::Shelf;
use crate::progress::Progress;
use citrine_gb::console::Console as _;
use citrine_gb::machine::Firmware;
use citrine_gb::rom::Rom;
use citrine_gb_replay::Replay;
use std::collections::VecDeque;
use std::sync::mpsc;

#[cfg(not(target_arch = "wasm32"))]
type PackedFile = std::path::PathBuf;
#[cfg(target_arch = "wasm32")]
type PackedFile = Vec<u8>;

struct Packed {
    new: NewMedia,
    file: PackedFile,
    output: recording::Output,
}

struct Done {
    id: MediaId,
    result: Result<Option<Packed>, String>,
}

struct Job {
    id: MediaId,
    replay: Box<Replay>,
    rom: Option<Rom>,
}

struct Running {
    id: MediaId,
    frames: u64,
    #[cfg(not(target_arch = "wasm32"))]
    played: std::sync::Arc<std::sync::atomic::AtomicU64>,
    #[cfg(not(target_arch = "wasm32"))]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_arch = "wasm32")]
    stage: Stage,
}

#[cfg(target_arch = "wasm32")]
enum Stage {
    Playing(Render),
    Finishing(crate::media::render::Finishing),
    Packing,
}

pub(super) struct VideoRenders {
    queue: VecDeque<Job>,
    running: Option<Running>,
    tape_rom: Option<(String, Rom)>,
    #[cfg(target_arch = "wasm32")]
    abandoned: Vec<crate::media::render::Finishing>,
    read_tx: mpsc::Sender<(MediaId, Option<Vec<u8>>)>,
    read_rx: mpsc::Receiver<(MediaId, Option<Vec<u8>>)>,
    done_tx: mpsc::Sender<Done>,
    done_rx: mpsc::Receiver<Done>,
}

impl Default for VideoRenders {
    fn default() -> Self {
        let (read_tx, read_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        Self {
            queue: VecDeque::new(),
            running: None,
            tape_rom: None,
            #[cfg(target_arch = "wasm32")]
            abandoned: Vec::new(),
            read_tx,
            read_rx,
            done_tx,
            done_rx,
        }
    }
}

impl VideoRenders {
    pub(super) fn tape_rom_for(&self, sha1: &str) -> Option<Rom> {
        self.tape_rom
            .as_ref()
            .filter(|(kept, _)| kept == sha1)
            .map(|(_, rom)| rom.clone())
    }

    pub(super) fn forget_tape_rom(&mut self) {
        self.tape_rom = None;
    }

    pub(super) fn is_busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    fn shown(&self) -> Option<Rendering> {
        let running = self.running.as_ref()?;
        Some(Rendering {
            id: running.id.clone(),
            progress: Progress::items(running.played(), running.frames, "frames"),
        })
    }

    fn waits_for(&self, id: &MediaId) -> bool {
        self.running.as_ref().is_some_and(|r| &r.id == id)
            || self.queue.iter().any(|job| &job.id == id)
    }
}

impl Running {
    #[cfg(not(target_arch = "wasm32"))]
    fn played(&self) -> u64 {
        self.played.load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(target_arch = "wasm32")]
    fn played(&self) -> u64 {
        match &self.stage {
            Stage::Playing(render) => render.played(),
            Stage::Finishing(_) | Stage::Packing => self.frames,
        }
    }
}

const NO_GAME: &str = "Open the game once, then render the video again";

fn sha1_hex(sha1: [u8; 20]) -> String {
    sha1.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn sha1_bytes(hex: &str) -> Option<[u8; 20]> {
    let mut out = [0u8; 20];
    if hex.len() != 40 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn find_rom(job_rom: Option<Rom>, shelf: &dyn Shelf, sha1: &str) -> Result<Rom, String> {
    if let Some(rom) = job_rom {
        return Ok(rom);
    }
    sha1_bytes(sha1)
        .and_then(|hash| shelf.find(&hash))
        .map(Rom::from)
        .ok_or_else(|| NO_GAME.to_string())
}

fn info_of(item: &MediaItem) -> NewMedia {
    NewMedia {
        kind: MediaKind::Replay,
        game: item.info.game.clone(),
        rom_key: item.info.rom_key.clone(),
        model: item.info.model,
        created_at: item.info.created_at,
        width: item.info.width,
        height: item.info.height,
        length_ms: item.info.length_ms,
        frame_times: item.info.frame_times,
        video: true,
        replay: None,
        frame: MediaFrame {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        },
    }
}

fn described(rendered: Rendered) -> (NewMedia, Replay, recording::Output) {
    let Rendered { mut replay, done } = rendered;
    let mut new = done.info;
    new.width = done.width as u32;
    new.height = done.height as u32;
    new.length_ms = Some(done.length.as_millis() as u64);
    new.video = true;
    if done.thumbnail.len() == new.frame.rgba.len() {
        new.frame = MediaFrame {
            rgba: done.thumbnail,
            ..new.frame
        };
    }
    replay.thumb = crate::utils::screenshot::encode_png(
        new.frame.width as usize,
        new.frame.height as usize,
        &new.frame.rgba,
    );
    (new, *replay, done.output)
}

struct Start {
    spawner: citrine_gb_threads::Spawner,
    firmware: Firmware,
    info: NewMedia,
    scale: usize,
    shelf: Box<dyn Shelf>,
}

impl Start {
    fn render(&self, replay: Box<Replay>, rom: Option<Rom>) -> Result<Render, String> {
        let rom = find_rom(rom, self.shelf.as_ref(), &replay.header.sha1)?;
        Render::start(
            &self.spawner,
            replay,
            &rom,
            &self.firmware,
            self.info.clone(),
            self.scale,
        )
    }
}

impl Citrine {
    pub(super) fn render_video(&mut self, id: MediaId, replay: Box<Replay>) {
        if self.video_renders.waits_for(&id) {
            return;
        }
        if replay.video.is_some() {
            self.toasts.info("This recording already has its video");
            return;
        }
        if let Some(reason) = &replay.header.no_inputs {
            self.toasts
                .error(format!("This recording can't be rendered: {reason}"));
            return;
        }
        let kept = self
            .video_renders
            .tape_rom
            .take()
            .filter(|(sha1, _)| *sha1 == replay.header.sha1)
            .map(|(_, rom)| rom);
        let rom = match kept {
            Some(rom) => Some(rom),
            None => self.live_rom(&replay.header.sha1),
        };
        self.video_renders.queue.push_back(Job { id, replay, rom });
    }

    pub(super) fn ask_render_video(&mut self, id: MediaId) {
        if self.video_renders.waits_for(&id) {
            return;
        }
        let tx = self.video_renders.read_tx.clone();
        let key = id.clone();
        self.media.read_bytes(&id, move |bytes| {
            let _ = tx.send((key, bytes));
        });
    }

    pub(super) fn stop_video_render(&mut self, id: &MediaId) {
        self.video_renders.queue.retain(|job| &job.id != id);
        if self
            .video_renders
            .running
            .as_ref()
            .is_some_and(|r| &r.id == id)
        {
            let Some(running) = self.video_renders.running.take() else {
                return;
            };
            #[cfg(not(target_arch = "wasm32"))]
            running
                .stop
                .store(true, std::sync::atomic::Ordering::Relaxed);
            #[cfg(target_arch = "wasm32")]
            match running.stage {
                Stage::Playing(render) => self.video_renders.abandoned.push(render.finish()),
                Stage::Finishing(finishing) => self.video_renders.abandoned.push(finishing),
                Stage::Packing => {}
            }
        }
    }

    pub(super) fn keep_tape_rom(&mut self) {
        let machine = &self.emulator.core_mut().machine;
        self.video_renders.tape_rom = Some((sha1_hex(machine.rom_id().sha1), machine.rom()));
    }

    fn live_rom(&mut self, sha1: &str) -> Option<Rom> {
        if !self.emulator.has_game() {
            return None;
        }
        let machine = &self.emulator.core_mut().machine;
        (sha1_hex(machine.rom_id().sha1) == sha1).then(|| machine.rom())
    }

    pub(super) fn tick_video_renders(&mut self, ctx: &egui::Context) {
        if let Some(id) = self.ui.media.render.take() {
            self.ask_render_video(id);
        }
        while let Ok((id, bytes)) = self.video_renders.read_rx.try_recv() {
            match bytes.as_deref().map(Replay::read) {
                Some(Ok(replay)) => self.render_video(id, Box::new(replay)),
                Some(Err(err)) => {
                    self.toasts
                        .error(format!("Could not render the video: {err}"));
                }
                None => {
                    self.toasts
                        .error("Could not render the video: the recording can't be read");
                }
            }
        }
        if let Some(id) = self.ui.media.stop_render.take() {
            self.stop_video_render(&id);
        }
        if self.video_renders.running.is_none()
            && let Some(job) = self.video_renders.queue.pop_front()
        {
            self.start_video_render(job);
        }
        #[cfg(target_arch = "wasm32")]
        self.step_video_render();
        while let Ok(done) = self.video_renders.done_rx.try_recv() {
            self.video_rendered(done);
        }
        self.ui.media.rendering = self.video_renders.shown();
        if self.video_renders.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn render_start(&self, id: &MediaId) -> Option<Start> {
        let item = self.media.items().iter().find(|item| &item.id == id)?;
        Some(Start {
            spawner: self.files.spawner.clone(),
            firmware: self.emulator.firmware.clone(),
            info: info_of(item),
            scale: self.ui.settings.recording_scale,
            shelf: self.ui.playing.shelf(&self.ui.recent),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_video_render(&mut self, job: Job) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicU64};
        let Some(start) = self.render_start(&job.id) else {
            return;
        };
        let played = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let frames = job.replay.header.frames;
        self.video_renders.running = Some(Running {
            id: job.id.clone(),
            frames,
            played: played.clone(),
            stop: stop.clone(),
        });
        let tx = self.video_renders.done_tx.clone();
        self.files.spawner.run("render video", move || {
            let result = start
                .render(job.replay, job.rom)
                .and_then(|render| crate::media::render::run(render, &stop, &played))
                .and_then(|rendered| match rendered {
                    None => Ok(None),
                    Some(rendered) => {
                        let (new, replay, output) = described(rendered);
                        match output.pack(replay) {
                            Ok((file, output)) => Ok(Some(Packed { new, file, output })),
                            Err((err, output)) => {
                                recording::discard(output);
                                Err(err)
                            }
                        }
                    }
                });
            let _ = tx.send(Done { id: job.id, result });
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn start_video_render(&mut self, job: Job) {
        let Some(start) = self.render_start(&job.id) else {
            return;
        };
        let frames = job.replay.header.frames;
        match start.render(job.replay, job.rom) {
            Ok(render) => {
                self.video_renders.running = Some(Running {
                    id: job.id,
                    frames,
                    stage: Stage::Playing(render),
                });
            }
            Err(err) => {
                let _ = self.video_renders.done_tx.send(Done {
                    id: job.id,
                    result: Err(err),
                });
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn step_video_render(&mut self) {
        use crate::media::render::Finishing;
        let renders = &mut self.video_renders;
        renders.abandoned = std::mem::take(&mut renders.abandoned)
            .into_iter()
            .filter_map(Finishing::abandon)
            .collect();
        let Some(mut running) = renders.running.take() else {
            return;
        };
        let id = running.id.clone();
        let tx = renders.done_tx.clone();
        running.stage = match running.stage {
            Stage::Playing(mut render) => match render.step(std::time::Duration::from_millis(4)) {
                Ok(()) if render.is_done() => Stage::Finishing(render.finish()),
                Ok(()) => Stage::Playing(render),
                Err(err) => {
                    renders.abandoned.push(render.finish());
                    let _ = tx.send(Done {
                        id,
                        result: Err(err),
                    });
                    return;
                }
            },
            Stage::Finishing(mut finishing) => match finishing.poll() {
                None => Stage::Finishing(finishing),
                Some(outcome) => {
                    match finishing.rendered(outcome) {
                        Ok(rendered) => {
                            let (new, replay, output) = described(rendered);
                            citrine_gb_threads::spawn_local(async move {
                                let result = match output.pack(replay).await {
                                    Ok((file, output)) => Ok(Some(Packed { new, file, output })),
                                    Err((err, _)) => Err(err),
                                };
                                let _ = tx.send(Done { id, result });
                            });
                        }
                        Err(err) => {
                            let _ = tx.send(Done {
                                id,
                                result: Err(err),
                            });
                        }
                    }
                    Stage::Packing
                }
            },
            Stage::Packing => Stage::Packing,
        };
        renders.running = Some(running);
    }

    fn video_rendered(&mut self, done: Done) {
        if self
            .video_renders
            .running
            .as_ref()
            .is_some_and(|r| r.id == done.id)
        {
            self.video_renders.running = None;
        }
        let exists = self.media.items().iter().any(|item| item.id == done.id);
        match done.result {
            Ok(None) => {}
            Ok(Some(packed)) if !exists => {
                discard_packed(packed.file);
                recording::discard(packed.output);
            }
            Ok(Some(packed)) => {
                let replaced = self.replace_media(&done.id, &packed.file, packed.new);
                recording::discard(packed.output);
                if let Err(err) = replaced {
                    discard_packed(packed.file);
                    self.toasts
                        .error(format!("Could not render the video: {err}"));
                }
            }
            Err(_) if !exists => {}
            Err(err) => {
                self.toasts
                    .error(format!("Could not render the video: {err}"));
            }
        }
    }

    fn replace_media(
        &mut self,
        id: &MediaId,
        file: &PackedFile,
        new: NewMedia,
    ) -> std::io::Result<MediaItem> {
        self.media.replace(id, file, new)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn discard_packed(file: PackedFile) {
    let _ = std::fs::remove_file(file);
}

#[cfg(target_arch = "wasm32")]
fn discard_packed(_file: PackedFile) {}
