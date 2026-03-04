use crate::gb::apu::channels::channel_1::Channel1;
use crate::gb::timer::Timer;
use crate::{ReadMemory, WriteMemory};
use types::audio_master_control::AudioMasterControl;
use types::master_volume_vin::MasterVolumeVin;
use types::sound_panning::SoundPanning;

mod channels;
mod types;

#[derive(Debug, Default)]
pub struct Apu {
    /// Increments at a frequency of 512 Hz
    div_apu: u8,
    prev_div: u16,
    pub nr50: MasterVolumeVin,
    pub nr51: SoundPanning,
    pub nr52: AudioMasterControl,
    pub ch1: Channel1,
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cycle(&mut self, timer: &Timer, double_speed: bool) {
        self.update_div(timer, double_speed);
    }

    pub fn tick(&mut self) {
        let v1 = self.ch1.tick();
    }

    // ToDo: DIV-APU events => https://gbdev.io/pandocs/Audio_details.html#div-apu
    fn update_div(&mut self, timer: &Timer, double_speed: bool) {
        let bit = if double_speed { 5 } else { 4 };
        let set_prev = ((self.prev_div >> bit) & 1) == 1;
        let set_now = ((timer.div >> bit) & 1) == 1;

        if !set_prev && set_now {
            self.div_apu = self.div_apu.wrapping_add(1);
        }

        self.prev_div = timer.div;
    }
}

impl ReadMemory for Apu {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF14 => self.ch1.read_naive(addr),
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
            0xFF10..=0xFF14 => self.ch1.write_naive(addr, value),
            0xFF24 => self.nr50 = value.into(),
            0xFF25 => self.nr51 = value.into(),
            0xFF26 => self.nr52 = value.into(),
            _ => {}
        }
    }
}
