use super::{CYCLES_PER_FRAME, Fault, Gba};
use crate::console::{Keys, Rollback};
use crate::error::{GbError, GbResult};
use crate::gba::bios::{Boot, SKIP_ENTRY_CYCLES};
use crate::gba::bus::Bus;
use crate::gba::cpu::Cpu;
use crate::netplay::state::CheckHash;
use crate::persistence::state::check_game;
use crate::utils::digest::game_key;
use crate::utils::unsaved;

const CLOCK_LIMIT: u64 = 1 << 62;

fn loaded_clock(cycles: u64, frame_end: u64) -> (u64, u64) {
    let cycles = cycles.min(CLOCK_LIMIT);
    if frame_end == 0 && cycles <= SKIP_ENTRY_CYCLES {
        return (cycles, 0);
    }
    let ahead = frame_end > cycles + CYCLES_PER_FRAME;
    let behind = cycles >= frame_end.saturating_add(CYCLES_PER_FRAME);
    if ahead || behind {
        (cycles, cycles + CYCLES_PER_FRAME)
    } else {
        (cycles, frame_end)
    }
}

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
    #[serde(skip_serializing_if = "unkeyed")]
    game: u64,
}

fn unkeyed(game: &u64) -> bool {
    *game == 0
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
    #[serde(default)]
    game: u64,
}

impl Gba {
    pub fn restore(&self, state: &[u8]) -> GbResult<Gba> {
        self.decode_state(&crate::persistence::state::gba_state_body(state)?)
    }

    fn game(&self) -> u64 {
        game_key(self.bus.pristine_rom())
    }

    fn saved(&self, game: u64) -> Saved<'_> {
        Saved {
            cpu: &self.cpu,
            bus: &self.bus,
            boot: self.boot,
            keys: self.keys,
            cycles: self.cycles,
            frame_end: self.frame_end,
            fault: self.fault,
            rumble: self.rumble,
            game,
        }
    }

    pub(crate) fn check_hash(&self) -> u64 {
        unsaved::hashing(|| {
            let mut hash = CheckHash::new();
            let _ = rmp_serde::encode::write(&mut hash, &self.saved(0));
            hash.finish()
        })
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
        self.check_hash()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        Ok(rmp_serde::to_vec(&self.saved(self.game()))?)
    }

    fn encode_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        Ok(rmp_serde::encode::write(out, &self.saved(self.game()))?)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<Gba> {
        let loaded: Loaded = rmp_serde::from_slice(data)?;
        check_game(loaded.game, self.bus.pristine_rom())?;
        let mut bus = loaded.bus;
        if !bus.has_hardware_sizes() {
            return Err(GbError::DamagedState);
        }
        bus.clamp_loaded();
        bus.adopt_rom(&self.bus);
        bus.retime_prefetch();
        bus.sound_mut().keep_settings(self.bus.sound());
        let (cycles, frame_end) = loaded_clock(loaded.cycles, loaded.frame_end);
        Ok(Gba {
            cpu: loaded.cpu,
            bus,
            bios: self.bios.clone(),
            boot: loaded.boot,
            keys: loaded.keys,
            cycles,
            frame_end,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Console;
    use crate::error::GbError;

    #[test]
    fn a_real_state_is_unchanged_by_the_load_clamps() {
        let rom: Vec<u8> = std::iter::repeat_n(0xEAFF_FFFEu32.to_le_bytes(), 0x4000)
            .flatten()
            .collect();
        let mut gba = Gba::new(&rom);
        for _ in 0..3 {
            gba.run_frame();
        }
        let state = gba.encode_state().expect("state");
        let loaded = gba.decode_state(&state).expect("loaded");
        assert_eq!(loaded.encode_state().expect("state again"), state);
    }

    #[test]
    fn a_battery_flush_leaves_the_check_hash_alone() {
        let mut rom = vec![0; 0x400];
        rom[0x200..0x209].copy_from_slice(b"SRAM_V113");
        let mut gba = Gba::new(&rom);
        gba.bus.write8(0x0E00_0010, 0x42);
        assert!(gba.bus.backup().dirty());
        let unsaved = gba.check_hash();
        assert!(gba.battery(false, 0).is_some());
        assert_eq!(gba.check_hash(), unsaved);
    }

    #[test]
    fn a_loaded_clock_far_from_its_frame_end_starts_a_fresh_frame() {
        let cycles = 2_090_675_114_978_420_169;
        assert_eq!(
            loaded_clock(cycles, 76_684_609),
            (cycles, cycles + CYCLES_PER_FRAME)
        );
        assert_eq!(loaded_clock(5, 9_000_000), (5, 5 + CYCLES_PER_FRAME));
    }

    #[test]
    fn a_loaded_clock_past_its_limit_is_cut_to_it() {
        let (cycles, frame_end) = loaded_clock(u64::MAX, u64::MAX);
        assert_eq!(cycles, CLOCK_LIMIT);
        assert_eq!(frame_end, CLOCK_LIMIT + CYCLES_PER_FRAME);
    }

    #[test]
    fn a_real_clock_is_unchanged_by_the_load_clamp() {
        for (cycles, frame_end) in [
            (0, 0),
            (1_000, 0),
            (SKIP_ENTRY_CYCLES, 0),
            (76_684_609, 76_684_609 + 1_000),
            (76_684_609, 76_684_609 + CYCLES_PER_FRAME),
            (76_684_612, 76_684_609),
        ] {
            assert_eq!(loaded_clock(cycles, frame_end), (cycles, frame_end));
        }
    }

    #[test]
    fn a_state_with_a_damaged_clock_still_runs_its_frames() {
        let rom: Vec<u8> = std::iter::repeat_n(0xEAFF_FFFEu32.to_le_bytes(), 0x4000)
            .flatten()
            .collect();
        let mut gba = Gba::new(&rom);
        gba.run_frame();
        gba.cycles = 2_090_675_114_978_420_169;
        let state = gba.encode_state().expect("state");
        let mut loaded = gba.decode_state(&state).expect("loaded");
        let start = std::time::Instant::now();
        for _ in 0..3 {
            loaded.run_frame();
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert!(loaded.cycles >= 2_090_675_114_978_420_169 + 2 * CYCLES_PER_FRAME);
    }

    #[test]
    fn a_gba_state_loads_only_into_its_own_game() {
        let mut other = vec![0; 0x200];
        other[0xA0] = b'X';
        let (here, elsewhere) = (Gba::new(&[0; 0x200]), Gba::new(&other));
        let state = here.encode_state().expect("state");
        assert!(matches!(
            elsewhere.decode_state(&state),
            Err(GbError::OtherGame)
        ));
        assert!(here.decode_state(&state).is_ok());
        let old = rmp_serde::to_vec(&here.saved(0)).expect("old state");
        assert!(elsewhere.decode_state(&old).is_ok());
    }
}
