use super::melonds::DS_VIDEO;
use crate::emulator::{Button, FrameEmulator};
use citrine_gb::console::{ConsoleKind, Keys, VideoSpec};
use citrine_gb::nds::machine::{Nds, NdsFirmware};

#[derive(Default)]
pub struct CitrineNdsEmulator {
    nds: Option<Nds>,
    keys: Keys,
}

impl CitrineNdsEmulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_nds(&mut self, rom: &[u8]) -> anyhow::Result<VideoSpec> {
        self.nds = Some(Nds::new(rom.into(), NdsFirmware::default())?);
        self.keys = Keys::empty();
        Ok(DS_VIDEO)
    }

    fn nds(&self) -> &Nds {
        self.nds.as_ref().expect("a DS game is loaded")
    }

    fn nds_mut(&mut self) -> &mut Nds {
        self.nds.as_mut().expect("a DS game is loaded")
    }
}

impl FrameEmulator for CitrineNdsEmulator {
    fn name(&self) -> &str {
        "citrine"
    }

    fn load(
        &mut self,
        _rom: &[u8],
        _boot_rom: Option<&[u8]>,
        console: ConsoleKind,
    ) -> anyhow::Result<VideoSpec> {
        anyhow::bail!("citrine's DS core runs only DS games, not {console:?}")
    }

    fn set_button(&mut self, button: Button, pressed: bool) {
        self.keys.set(button.to_keys(), pressed);
        let keys = self.keys;
        self.nds_mut().set_keys(keys);
    }

    fn total_cycles(&self) -> u64 {
        self.nds().cycles()
    }

    fn step(&mut self) -> bool {
        self.nds_mut().run_frame();
        true
    }

    fn render_into(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(self.nds().frame());
    }
}
