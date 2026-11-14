use crate::archive::{Report, ReportError};
use citrine_gb::console::{Console, VideoSpec};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

pub fn frame_digest(frame: &[u8]) -> u64 {
    frame.iter().fold(FNV_OFFSET, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

pub struct Replayed {
    pub frames: u64,
    pub digest: u64,
    pub matches: bool,
    pub frame: Vec<u8>,
    pub video: VideoSpec,
}

pub fn replay(report: &Report, rom: &Rom, firmware: &Firmware) -> Result<Replayed, ReportError> {
    let (Some(info), Some(state)) = (&report.manifest.replay, &report.state) else {
        return Err(ReportError::NoReplay);
    };
    let mut machine = Machine::from_state(state, rom, info.state_unix, firmware)?;
    for input in &report.inputs {
        if input.reset {
            machine.soft_reset();
        }
        machine.set_keys(input.keys);
        machine.set_touch(input.touch);
        machine.run_frame();
    }
    let digest = frame_digest(machine.frame());
    Ok(Replayed {
        frames: report.inputs.len() as u64,
        digest,
        matches: format!("{digest:016x}") == info.end_digest,
        frame: machine.frame().to_vec(),
        video: machine.video(),
    })
}
