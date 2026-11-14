use super::{Fault, Gba};
use crate::console::{Keys, Rollback};
use crate::error::GbResult;
use crate::gba::bios::Boot;
use crate::gba::bus::Bus;
use crate::gba::cpu::Cpu;
use crate::netplay::state::CheckHash;

#[derive(serde::Serialize)]
struct Saved<'a> {
    cpu: &'a Cpu,
    bus: &'a Bus,
    boot: Boot,
    keys: Keys,
    cycles: u64,
    frame_end: u64,
    fault: Option<Fault>,
    rumble: bool,
}

#[derive(serde::Deserialize)]
struct Loaded {
    cpu: Cpu,
    bus: Bus,
    boot: Boot,
    keys: Keys,
    cycles: u64,
    frame_end: u64,
    fault: Option<Fault>,
    rumble: bool,
}

impl Gba {
    pub fn restore(&self, state: &[u8]) -> GbResult<Gba> {
        self.decode_state(&crate::persistence::state::gba_state_body(state)?)
    }

    fn saved(&self) -> Saved<'_> {
        Saved {
            cpu: &self.cpu,
            bus: &self.bus,
            boot: self.boot,
            keys: self.keys,
            cycles: self.cycles,
            frame_end: self.frame_end,
            fault: self.fault,
            rumble: self.rumble,
        }
    }
}

impl Rollback for Gba {
    fn clone_state(&self) -> Gba {
        Gba {
            cpu: self.cpu.clone(),
            bus: self.bus.clone(),
            bios: self.bios.clone(),
            boot: self.boot,
            keys: self.keys,
            cycles: self.cycles,
            frame_end: self.frame_end,
            fault: self.fault,
            rumble: self.rumble,
            idle: self.idle.restarted(),
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            #[cfg(feature = "debug")]
            code: Default::default(),
            #[cfg(feature = "debug")]
            stepping: Default::default(),
            #[cfg(feature = "rom-id")]
            rom_id: self.rom_id,
            #[cfg(jit)]
            jit: self.jit.as_deref().map(|jit| Box::new(jit.fork())),
            #[cfg(jit)]
            jit_cache: self.jit_cache.clone(),
            #[cfg(jit)]
            straight: false,
            #[cfg(feature = "jit-lockstep")]
            lockstep: Default::default(),
        }
    }

    fn copy_state_from(&mut self, source: &Gba) {
        self.cpu.clone_from(&source.cpu);
        let rom_changed = self.bus.adopt_rom(&source.bus);
        self.bus.copy_state_from(&source.bus);
        if self.bios != source.bios {
            self.bios.clone_from(&source.bios);
        }
        self.boot = source.boot;
        self.keys = source.keys;
        self.cycles = source.cycles;
        self.frame_end = source.frame_end;
        self.fault = source.fault;
        self.rumble = source.rumble;
        if rom_changed {
            self.idle.forget();
        } else {
            self.idle.restart();
        }
        #[cfg(feature = "rom-id")]
        {
            self.rom_id = source.rom_id;
        }
        #[cfg(jit)]
        {
            self.straight = false;
            if let Some(jit) = self.jit.as_deref_mut() {
                jit.recheck();
            }
        }
    }

    fn snapshot_into(&mut self, slot: &mut Gba) {
        self.bus.share_pages_into(&mut slot.bus);
        slot.copy_state_from(self);
    }

    fn state_hash(&self, _scratch: &mut Gba) -> u64 {
        let mut hash = CheckHash::new();
        let _ = rmp_serde::encode::write(&mut hash, &self.saved());
        hash.finish()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        Ok(rmp_serde::to_vec(&self.saved())?)
    }

    fn encode_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        Ok(rmp_serde::encode::write(out, &self.saved())?)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<Gba> {
        let loaded: Loaded = rmp_serde::from_slice(data)?;
        let mut bus = loaded.bus;
        bus.adopt_rom(&self.bus);
        bus.retime_prefetch();
        bus.sound_mut().keep_settings(self.bus.sound());
        Ok(Gba {
            cpu: loaded.cpu,
            bus,
            bios: self.bios.clone(),
            boot: loaded.boot,
            keys: loaded.keys,
            cycles: loaded.cycles,
            frame_end: loaded.frame_end,
            fault: loaded.fault,
            rumble: loaded.rumble,
            idle: self.idle.restarted(),
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            #[cfg(feature = "debug")]
            code: Default::default(),
            #[cfg(feature = "debug")]
            stepping: Default::default(),
            #[cfg(feature = "rom-id")]
            rom_id: self.rom_id,
            #[cfg(jit)]
            jit: self.jit.as_deref().map(|jit| Box::new(jit.fork())),
            #[cfg(jit)]
            jit_cache: self.jit_cache.clone(),
            #[cfg(jit)]
            straight: false,
            #[cfg(feature = "jit-lockstep")]
            lockstep: Default::default(),
        })
    }

    fn set_blind(&mut self, blind: bool) {
        self.bus.set_blind(blind);
    }

    fn set_muted(&mut self, muted: bool) {
        self.bus.set_silent(muted);
    }
}
