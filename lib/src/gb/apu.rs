use crate::gb::apu::audio_master_control::AudioMasterControl;
use crate::gb::apu::master_volume_vin::MasterVolumeVin;
use crate::gb::apu::sound_panning::SoundPanning;
use crate::{ReadMemory, WriteMemory};

mod audio_master_control;
mod master_volume_vin;
mod sound_panning;

#[derive(Debug, Default)]
pub struct Apu {
    pub nr50: MasterVolumeVin,
    pub nr51: SoundPanning,
    pub nr52: AudioMasterControl,
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ReadMemory for Apu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF24 => self.nr50.into(),
            0xFF25 => self.nr51.into(),
            0xFF26 => self.nr52.into(),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Apu {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF24 => self.nr50 = value.into(),
            0xFF25 => self.nr51 = value.into(),
            0xFF26 => self.nr52 = value.into(),
            _ => {}
        }
    }
}
