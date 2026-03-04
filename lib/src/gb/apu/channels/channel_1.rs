use crate::gb::apu::channels::channel_1::control::Channel1Control;
use crate::gb::apu::channels::channel_1::sweep::Channel1Sweep;
use crate::gb::apu::channels::channel_1::timer::Channel1Timer;
use crate::gb::apu::channels::channel_1::volume::Channel1Volume;
use crate::{ReadMemory, WriteMemory};

mod control;
mod sweep;
mod timer;
mod volume;

#[derive(Debug, Default)]
pub struct Channel1 {
    pub enabled: bool,
    /// NR10 (0xFF10)
    pub sweep: Channel1Sweep,
    /// NR11 (0xFF11)
    pub timer: Channel1Timer,
    /// NR12 (0xFF12)
    pub volume: Channel1Volume,
    /// NR13 (0xFF13) => Write-only
    pub period_low: u8,
    /// NR14 (0xFF14)
    pub control: Channel1Control,
}

impl Channel1 {
    /// Returns the current volume of the channel (0-15)
    pub fn tick(&mut self) -> u8 {
        todo!()
    }
}

impl ReadMemory for Channel1 {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => self.sweep.into(),
            0xFF11 => self.timer.into(),
            0xFF12 => self.volume.into(),
            0xFF13 => 0xFF,
            0xFF14 => self.control.into(),
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Channel1 {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF10 => self.sweep = value.into(),
            0xFF11 => self.timer = value.into(),
            0xFF12 => self.volume = value.into(),
            0xFF13 => self.period_low = value,
            0xFF14 => self.control = value.into(),
            _ => {}
        }
    }
}
