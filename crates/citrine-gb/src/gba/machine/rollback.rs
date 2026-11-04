use super::{Fault, Gba};
use crate::console::{Keys, Rollback};
use crate::error::GbResult;
use crate::gba::bios::Boot;
use crate::gba::bus::Bus;
use crate::gba::cpu::Cpu;
use crate::netplay::state::Fnv;

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
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            #[cfg(feature = "debug")]
            code: Default::default(),
            #[cfg(feature = "debug")]
            stepping: Default::default(),
            #[cfg(feature = "rom-id")]
            rom_id: self.rom_id,
        }
    }

    fn copy_state_from(&mut self, source: &Gba) {
        self.cpu.clone_from(&source.cpu);
        self.bus.adopt_rom(&source.bus);
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
        #[cfg(feature = "rom-id")]
        {
            self.rom_id = source.rom_id;
        }
    }

    fn state_hash(&self, _scratch: &mut Gba) -> u64 {
        let mut fnv = Fnv::new();
        let _ = rmp_serde::encode::write(&mut fnv, &self.saved());
        fnv.finish()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        Ok(rmp_serde::to_vec(&self.saved())?)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<Gba> {
        let loaded: Loaded = rmp_serde::from_slice(data)?;
        let mut bus = loaded.bus;
        bus.adopt_rom(&self.bus);
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
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            #[cfg(feature = "debug")]
            code: Default::default(),
            #[cfg(feature = "debug")]
            stepping: Default::default(),
            #[cfg(feature = "rom-id")]
            rom_id: self.rom_id,
        })
    }

    fn set_blind(&mut self, blind: bool) {
        self.bus.set_blind(blind);
    }

    fn set_muted(&mut self, muted: bool) {
        self.bus.set_silent(muted);
    }
}
