use super::Nds;
use crate::console::Rollback;
use crate::error::GbResult;
use crate::nds::sound::output::Output;
use crate::netplay::state::CheckHash;

impl Rollback for Nds {
    fn clone_state(&self) -> Nds {
        Nds {
            arm9: self.arm9.clone(),
            arm7: self.arm7.clone(),
            buses: self.buses.clone(),
            rom: self.rom.clone(),
            firmware: self.firmware.clone(),
            header: self.header.clone(),
            arm9_cycles: self.arm9_cycles,
            arm7_cycles: self.arm7_cycles,
            now: self.now,
            frames: self.frames,
            frame: self.frame.clone(),
            published: Vec::new(),
            faults: self.faults,
            idle_skips: self.idle_skips,
            audio: Output::resumed(self.audio.sample_rate(), self.audio.level()),
            idle: self.idle.clone(),
            air: self.air.clone(),
            download: self.download.clone(),
            slice_limit: u32::MAX,
            blind: self.blind,
            torn: self.torn,
            #[cfg(feature = "rom-id")]
            game_sha1: self.game_sha1,
            #[cfg(feature = "rom-id")]
            rom_id: self.rom_id,
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            #[cfg(feature = "debug")]
            debug: Default::default(),
            #[cfg(jit)]
            jit: self.jit.as_deref().map(|jit| Box::new(jit.fork())),
            #[cfg(jit)]
            jit_cache: self.jit_cache.clone(),
            #[cfg(feature = "jit-lockstep")]
            lockstep: Default::default(),
        }
    }

    fn copy_state_from(&mut self, source: &Nds) {
        self.arm9.clone_from(&source.arm9);
        self.arm7.clone_from(&source.arm7);
        self.buses.copy_state_from(&source.buses);
        if !std::sync::Arc::ptr_eq(&self.rom, &source.rom) {
            self.rom = source.rom.clone();
        }
        if self.firmware != source.firmware {
            self.firmware.clone_from(&source.firmware);
        }
        self.header.clone_from(&source.header);
        self.arm9_cycles = source.arm9_cycles;
        self.arm7_cycles = source.arm7_cycles;
        self.now = source.now;
        self.frames = source.frames;
        self.frame.copy_from_slice(&source.frame);
        self.published.clear();
        self.faults = source.faults;
        self.idle_skips = source.idle_skips;
        self.idle.restart();
        self.audio.hold(source.audio.level());
        self.air.clone_from(&source.air);
        self.download.clone_from(&source.download);
        self.torn = source.torn;
        #[cfg(feature = "rom-id")]
        {
            self.game_sha1 = source.game_sha1;
            self.rom_id = source.rom_id;
        }
        #[cfg(jit)]
        if let Some(jit) = self.jit.as_deref_mut() {
            jit.restart();
        }
    }

    fn snapshot_into(&mut self, slot: &mut Nds) {
        #[cfg(jit)]
        {
            slot.jit = None;
        }
        self.buses.share_pages_into(&mut slot.buses);
        slot.copy_state_from(self);
    }

    fn scratch(&self) -> Nds {
        let mut scratch = self.clone_state();
        scratch.buses.empty_rams();
        #[cfg(jit)]
        {
            scratch.jit = None;
        }
        scratch
    }

    fn state_hash(&self, _scratch: &mut Nds) -> u64 {
        let mut hash = CheckHash::new();
        self.write_hashed(&mut hash);
        hash.finish()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        Nds::encode_state(self)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<Nds> {
        let mut nds = Nds::decode_state(self, data)?;
        nds.blind = self.blind;
        nds.buses.set_skip_draw(self.blind);
        nds.buses.set_sound_muted(self.buses.sound_muted());
        Ok(nds)
    }

    fn set_blind(&mut self, blind: bool) {
        self.torn |= self.blind && !blind && self.buses.lcd().mid_draw();
        self.blind = blind;
        self.buses.set_skip_draw(blind);
    }

    fn set_muted(&mut self, muted: bool) {
        self.buses.set_sound_muted(muted);
    }
}
