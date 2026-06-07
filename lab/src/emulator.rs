//! The emulator abstraction the comparison harness drives.
//!
//! Any emulator that can be stepped deterministically, fed button input at a known cycle, and
//! asked for its current framebuffer can be plugged into the lab by implementing [`FrameEmulator`].

use citrine_gb::gb::GbModel;

/// Re-export so the harness and adapters share one button type with the recording schema.
pub use citrine_gb::recording::Button;

/// Game Boy screen dimensions (DMG/CGB, no SGB border).
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
/// Number of RGBA bytes in a full frame.
pub const FRAME_BYTES: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;

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

    /// Same dimensions and pixel data.
    pub fn matches_dimensions(&self, other: &Frame) -> bool {
        self.width == other.width && self.height == other.height
    }

    /// Map every pixel to the nearest of four canonical greys (`0x00, 0x55, 0xAA, 0xFF`) by
    /// luminance, producing a palette-independent frame.
    ///
    /// DMG output is four evenly-spaced shades; once both emulators are forced to a clean
    /// greyscale this collapses any residual per-emulator RGB differences, so Exact-Frame and MSE
    /// measure structural divergence rather than cosmetic palette choices.
    pub fn to_canonical_greyscale(&self) -> Frame {
        let mut out = Frame::new(self.width, self.height, vec![0u8; self.rgba.len()]);
        self.normalize_into(&mut out);
        out
    }

    /// In-place [`to_canonical_greyscale`](Self::to_canonical_greyscale): normalize `self` into
    /// `out`, reusing `out`'s allocation. Used on the hot path so the streaming runner never
    /// allocates a fresh frame per step.
    pub fn normalize_into(&self, out: &mut Frame) {
        // The four shades are evenly spaced ~85 apart, so the nearest level is just the rounded
        // quotient — no per-pixel search over the palette.
        const LEVELS: [u8; 4] = [0x00, 0x55, 0xAA, 0xFF];
        out.width = self.width;
        out.height = self.height;
        out.rgba.resize(self.rgba.len(), 0);
        for (px, o) in self.rgba.chunks_exact(4).zip(out.rgba.chunks_exact_mut(4)) {
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

/// An emulator the harness can drive frame-by-frame with cycle-timed input.
///
/// The canonical cycle unit across the harness is the **T-cycle** (~4.19 MHz on DMG). Adapters are
/// responsible for converting their native cycle counter into T-cycles.
pub trait FrameEmulator {
    /// Human-readable name, used in reports (e.g. "citrine", "sameboy").
    fn name(&self) -> &str;

    /// Load a ROM and reset.
    ///
    /// When `boot_rom` is `Some`, both emulators run that boot ROM from power-on (the most rigorous
    /// comparison). When `None`, the emulator jumps straight to the canonical post-boot state for
    /// `model`. Note: SameBoy requires a boot ROM to actually execute, so `None` only produces a
    /// meaningful comparison for emulators that synthesise a post-boot state.
    fn load(&mut self, rom: &[u8], boot_rom: Option<&[u8]>, model: GbModel) -> anyhow::Result<()>;

    /// Press or release a button. Takes effect on subsequent steps.
    fn set_button(&mut self, button: Button, pressed: bool);

    /// Absolute number of T-cycles emulated since the last [`load`](FrameEmulator::load).
    fn total_cycles(&self) -> u64;

    /// Advance by the smallest atomic unit the emulator supports. Returns `true` if a frame
    /// completed during this step (i.e. the framebuffer is now a fresh, complete frame).
    fn step(&mut self) -> bool;

    /// Render the current framebuffer as RGBA into `out`, reusing its allocation.
    ///
    /// This is the hot-path entry point: the streaming runner hands back a recycled buffer each
    /// frame so no per-frame allocation occurs. Implementors must overwrite `out` completely
    /// (its previous contents are stale).
    fn render_into(&self, out: &mut Vec<u8>);

    /// Snapshot the current framebuffer into a fresh [`Frame`]. Convenience wrapper around
    /// [`render_into`](Self::render_into); prefer that on hot paths.
    fn frame(&self) -> Frame {
        let mut rgba = Vec::with_capacity(FRAME_BYTES);
        self.render_into(&mut rgba);
        Frame::new(SCREEN_WIDTH, SCREEN_HEIGHT, rgba)
    }
}
