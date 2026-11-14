use super::{AirPort, Fault, Nds};
use crate::error::GbResult;
use crate::gba::cpu::{Arm9, Cpu as Arm7};
use crate::nds::bus::Buses;
use crate::nds::download::Download;
use crate::nds::sound::output::Output;
use crate::persistence::state::nds_state_body;
use std::sync::Arc;

#[derive(serde::Serialize)]
struct Saved<'a> {
    arm9: &'a Arm9,
    arm7: &'a Arm7,
    buses: &'a Buses,
    arm9_cycles: u64,
    arm7_cycles: u64,
    now: u64,
    frames: u64,
    #[serde(with = "crate::utils::bytes")]
    frame: &'a [u8],
    faults: [Option<Fault>; 2],
    audio_level: [i16; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    air_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<&'a Download>,
}

pub struct StateCopy {
    arm9: Arm9,
    arm7: Arm7,
    buses: Buses,
    arm9_cycles: u64,
    arm7_cycles: u64,
    now: u64,
    frames: u64,
    frame: Box<[u8]>,
    faults: [Option<Fault>; 2],
    audio_level: [i16; 2],
    air_epoch: Option<u64>,
    download: Option<Box<Download>>,
}

impl StateCopy {
    fn copy_from(&mut self, nds: &Nds) {
        let Self {
            arm9,
            arm7,
            buses,
            arm9_cycles,
            arm7_cycles,
            now,
            frames,
            frame,
            faults,
            audio_level,
            air_epoch,
            download,
        } = self;
        arm9.clone_from(&nds.arm9);
        arm7.clone_from(&nds.arm7);
        buses.copy_saved_from(&nds.buses);
        *arm9_cycles = nds.arm9_cycles;
        *arm7_cycles = nds.arm7_cycles;
        *now = nds.now;
        *frames = nds.frames;
        frame.clone_from(&nds.frame);
        *faults = nds.faults;
        *audio_level = nds.audio.level();
        *air_epoch = nds.air.as_ref().map(|air| air.epoch);
        download.clone_from(&nds.download);
    }

    pub fn encode_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        Ok(rmp_serde::encode::write(
            out,
            &Saved {
                arm9: &self.arm9,
                arm7: &self.arm7,
                buses: &self.buses,
                arm9_cycles: self.arm9_cycles,
                arm7_cycles: self.arm7_cycles,
                now: self.now,
                frames: self.frames,
                frame: &self.frame,
                faults: self.faults,
                audio_level: self.audio_level,
                air_epoch: self.air_epoch,
                download: self.download.as_deref(),
            },
        )?)
    }
}

#[derive(serde::Serialize)]
struct Hashed<'a> {
    arm9: &'a Arm9,
    arm7: &'a Arm7,
    buses: &'a Buses,
    arm9_cycles: u64,
    arm7_cycles: u64,
    now: u64,
    frames: u64,
    faults: [Option<Fault>; 2],
    audio_level: [i16; 2],
    air_epoch: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    download: Option<&'a Download>,
}

#[derive(serde::Deserialize)]
struct Loaded {
    arm9: Arm9,
    arm7: Arm7,
    buses: Buses,
    arm9_cycles: u64,
    arm7_cycles: u64,
    now: u64,
    frames: u64,
    #[serde(with = "crate::utils::bytes")]
    frame: Box<[u8]>,
    faults: [Option<Fault>; 2],
    audio_level: [i16; 2],
    #[serde(default)]
    air_epoch: Option<u64>,
    #[serde(default)]
    download: Option<Download>,
}

impl Nds {
    pub(super) fn write_hashed(&self, out: &mut impl std::io::Write) {
        let _ = rmp_serde::encode::write(
            out,
            &Hashed {
                arm9: &self.arm9,
                arm7: &self.arm7,
                buses: &self.buses,
                arm9_cycles: self.arm9_cycles,
                arm7_cycles: self.arm7_cycles,
                now: self.now,
                frames: self.frames,
                faults: self.faults,
                audio_level: self.audio.level(),
                air_epoch: self.air.as_ref().map(|air| air.epoch),
                download: self.download.as_deref(),
            },
        );
    }

    pub fn encode_state(&self) -> GbResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_state_into(&mut out)?;
        Ok(out)
    }

    pub fn encode_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        Ok(rmp_serde::encode::write(
            out,
            &Saved {
                arm9: &self.arm9,
                arm7: &self.arm7,
                buses: &self.buses,
                arm9_cycles: self.arm9_cycles,
                arm7_cycles: self.arm7_cycles,
                now: self.now,
                frames: self.frames,
                frame: &self.frame,
                faults: self.faults,
                audio_level: self.audio.level(),
                air_epoch: self.air.as_ref().map(|air| air.epoch),
                download: self.download.as_deref(),
            },
        )?)
    }

    pub fn copy_state_into(&self, slot: &mut Option<Box<StateCopy>>) {
        match slot {
            Some(copy) => copy.copy_from(self),
            None => {
                *slot = Some(Box::new(StateCopy {
                    arm9: self.arm9.clone(),
                    arm7: self.arm7.clone(),
                    buses: self.buses.clone(),
                    arm9_cycles: self.arm9_cycles,
                    arm7_cycles: self.arm7_cycles,
                    now: self.now,
                    frames: self.frames,
                    frame: self.frame.clone(),
                    faults: self.faults,
                    audio_level: self.audio.level(),
                    air_epoch: self.air.as_ref().map(|air| air.epoch),
                    download: self.download.clone(),
                }))
            }
        }
    }

    pub fn decode_state(&self, data: &[u8]) -> GbResult<Nds> {
        let loaded: Loaded = rmp_serde::from_slice(data)?;
        let mut buses = loaded.buses;
        buses.adopt_fixed(&self.buses);
        let torn = buses.lcd().mid_draw();
        Ok(Nds {
            idle_skips: self.idle_skips,
            arm9: loaded.arm9.landed_here(),
            arm7: loaded.arm7.landed_here(),
            buses,
            rom: Arc::clone(&self.rom),
            firmware: self.firmware.clone(),
            header: self.header.clone(),
            arm9_cycles: loaded.arm9_cycles,
            arm7_cycles: loaded.arm7_cycles,
            now: loaded.now,
            frames: loaded.frames,
            frame: loaded.frame,
            published: Vec::new(),
            faults: loaded.faults,
            audio: Output::resumed(self.audio.sample_rate(), loaded.audio_level),
            idle: self.idle.restarted(),
            air: loaded.air_epoch.map(|epoch| AirPort {
                epoch,
                ..AirPort::default()
            }),
            download: loaded.download.map(Box::new),
            slice_limit: u32::MAX,
            blind: false,
            torn,
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
        })
    }

    pub fn restore(&self, state: &[u8]) -> GbResult<Nds> {
        self.decode_state(&nds_state_body(state)?)
    }

    pub fn rewind_to(&mut self, data: &[u8]) -> GbResult<()> {
        #[cfg(not(feature = "debug"))]
        let back = self.decode_state(data)?;
        #[cfg(feature = "debug")]
        let mut back = self.decode_state(data)?;
        #[cfg(feature = "debug")]
        {
            std::mem::swap(&mut back.debugger, &mut self.debugger);
            std::mem::swap(&mut back.debug, &mut self.debug);
        }
        *self = back;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Console;
    use crate::nds::machine::NdsFirmware;
    use crate::nds::machine::test_rom::{BRANCH_TO_SELF, rom};
    const COUNT: [u32; 6] = [0xE3A0_0000, 0xE280_0001, 0xE58F_0004, 0xEAFF_FFFC, 0, 0];
    const WAKE_ON_VBLANK: [u32; 11] = [
        0xE3A0_0301,
        0xE3A0_1008,
        0xE1C0_10B4,
        0xE3A0_1001,
        0xE580_1210,
        0xE3A0_1080,
        0xE5C0_1301,
        0xE3A0_2042,
        0xE58F_2000,
        BRANCH_TO_SELF,
        0,
    ];
    const FRAMES: usize = 20;

    fn run(nds: &mut Nds) -> (Vec<Vec<u8>>, Vec<f32>) {
        let mut audio = Vec::new();
        let frames = (0..FRAMES)
            .map(|_| {
                nds.run_frame();
                nds.take_audio(&mut audio);
                nds.frame().to_vec()
            })
            .collect();
        (frames, audio)
    }

    #[test]
    fn a_restored_state_runs_the_same_frames_and_sound() {
        let mut nds = Nds::new(rom(&COUNT, &WAKE_ON_VBLANK), NdsFirmware::default()).unwrap();
        for _ in 0..7 {
            nds.run_frame();
        }
        nds.take_audio(&mut Vec::new());
        let state = nds.save_state(0).unwrap();
        let mut restored = nds.restore(&state).unwrap();
        assert_eq!(restored.frame(), nds.frame());
        let ahead = run(&mut nds);
        let again = run(&mut restored);
        assert!(ahead == again);
        assert_eq!(restored.cycles(), nds.cycles());
        assert!(restored.encode_state().unwrap() == nds.encode_state().unwrap());
    }

    #[cfg(jit)]
    #[test]
    fn a_state_load_rebuilds_the_fast_table() {
        use crate::jit::FastBus;
        let mut nds = Nds::new(rom(&COUNT, &WAKE_ON_VBLANK), NdsFirmware::default()).unwrap();
        nds.set_jit(true);
        nds.run_frame();
        let state = nds.encode_state().unwrap();
        let kept = nds.buses_mut().arm9().fast();
        unsafe { (*kept).windows[2].bytes = std::ptr::dangling_mut() };
        nds.rewind_to(&state).unwrap();
        let main = unsafe { (*nds.buses_mut().arm9().fast()).windows[2].bytes };
        assert_eq!(main.cast_const(), nds.buses().main_ram().as_ptr());
        nds.run_frame();
        assert!(nds.jit_stats().expect("the JIT is on").runs > 0);
    }

    #[test]
    fn a_state_keeps_the_save_chip() {
        let mut nds = Nds::new(rom(&COUNT, &[BRANCH_TO_SELF]), NdsFirmware::default()).unwrap();
        nds.put_battery(
            crate::persistence::battery_save::BatterySave::raw(vec![0x3C; 0x2000]),
            0,
        );
        let state = nds.save_state(0).unwrap();
        nds.put_battery(
            crate::persistence::battery_save::BatterySave::raw(vec![0; 0x2000]),
            0,
        );
        let restored = nds.restore(&state).unwrap();
        let backup = restored.buses().cart().backup().unwrap();
        assert_eq!(backup.bytes(), &[0x3C; 0x2000][..]);
    }

    #[test]
    fn a_state_from_another_console_is_refused() {
        let nds = Nds::new(
            rom(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]),
            NdsFirmware::default(),
        )
        .unwrap();
        assert!(nds.restore(b"CGBA\x01").is_err());
    }

    #[test]
    fn a_kept_copy_encodes_like_the_machine_it_copies() {
        let mut nds = Nds::new(rom(&COUNT, &WAKE_ON_VBLANK), NdsFirmware::default()).unwrap();
        let mut copy = None;
        for _ in 0..12 {
            nds.run_frame();
            nds.copy_state_into(&mut copy);
            let mut kept = Vec::new();
            copy.as_ref().unwrap().encode_into(&mut kept).unwrap();
            assert!(kept == nds.encode_state().unwrap());
        }
    }
}
