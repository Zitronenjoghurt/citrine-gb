use citrine_gb::console::{ConsoleKind, VideoSpec};

pub use citrine_gb::recording::Button;

/// A completed frame as RGBA8888, row-major, top-left origin.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), width * height * 4);
        Self {
            width,
            height,
            rgba,
        }
    }

    /// Snaps to four canonical greys, so metrics ignore each emulator's palette choice.
    pub fn to_canonical_greyscale(&self) -> Frame {
        let mut out = Frame::new(self.width, self.height, vec![0u8; self.rgba.len()]);
        self.normalize_into(&mut out);
        out
    }

    pub fn canonical_for(console: ConsoleKind) -> fn(&Frame, &mut Frame) {
        match console {
            ConsoleKind::Gba => Frame::rgb555_into,
            ConsoleKind::Gb | ConsoleKind::Gbc => Frame::normalize_into,
        }
    }

    pub fn rgb555_into(&self, out: &mut Frame) {
        out.width = self.width;
        out.height = self.height;
        out.rgba.resize(self.rgba.len(), 0);
        for (px, o) in self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.rgba.as_chunks_mut::<4>().0)
        {
            *o = [px[0] & 0xF8, px[1] & 0xF8, px[2] & 0xF8, 0xFF];
        }
    }

    pub fn normalize_into(&self, out: &mut Frame) {
        // Evenly spaced ~85 apart, so the nearest level is the rounded quotient and needs no palette search.
        const LEVELS: [u8; 4] = [0x00, 0x55, 0xAA, 0xFF];
        out.width = self.width;
        out.height = self.height;
        out.rgba.resize(self.rgba.len(), 0);
        for (px, o) in self
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(out.rgba.as_chunks_mut::<4>().0)
        {
            let luma = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
            let idx = ((luma / 85.0).round() as usize).min(3);
            let level = LEVELS[idx];
            o[0] = level;
            o[1] = level;
            o[2] = level;
            o[3] = 0xFF;
        }
    }
}

/// The harness-wide cycle unit is the T-cycle; adapters convert their native counter to it.
pub trait FrameEmulator {
    fn name(&self) -> &str;

    /// `None` jumps straight to the post-boot state. SameBoy cannot run without a boot ROM.
    fn load(
        &mut self,
        rom: &[u8],
        boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec>;

    fn set_button(&mut self, button: Button, pressed: bool);

    fn total_cycles(&self) -> u64;

    /// Advance by the smallest unit available; `true` if that completed a frame.
    fn step(&mut self) -> bool;

    /// `out` is a recycled buffer with stale contents; implementors must overwrite it completely.
    fn render_into(&self, out: &mut Vec<u8>);

    fn frame(&self, video: VideoSpec) -> Frame {
        let mut rgba = Vec::with_capacity(video.width * video.height * 4);
        self.render_into(&mut rgba);
        Frame::new(video.width, video.height, rgba)
    }
}

impl<E: FrameEmulator + ?Sized> FrameEmulator for Box<E> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn load(
        &mut self,
        rom: &[u8],
        boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec> {
        (**self).load(rom, boot_rom, console)
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        (**self).set_button(button, pressed)
    }

    fn total_cycles(&self) -> u64 {
        (**self).total_cycles()
    }

    fn step(&mut self) -> bool {
        (**self).step()
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        (**self).render_into(out)
    }
}
