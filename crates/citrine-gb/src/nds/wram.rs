use super::Cpu;
use super::ram::Ram;
use crate::gba::wait::Width;
#[cfg(jit)]
use crate::jit;

pub const SHARED_WRAM_SIZE: usize = 0x8000;
const HALF: usize = SHARED_WRAM_SIZE / 2;
const MODE_MASK: u8 = 0x03;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    base: usize,
    len: usize,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SharedWram {
    ram: Ram,
    control: u8,
}

impl Clone for SharedWram {
    fn clone(&self) -> Self {
        Self {
            ram: self.ram.clone(),
            control: self.control,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.ram.clone_from(&source.ram);
        self.control = source.control;
    }
}

impl Default for SharedWram {
    fn default() -> Self {
        Self {
            ram: Ram::new(SHARED_WRAM_SIZE),
            control: 0,
        }
    }
}

impl SharedWram {
    pub fn control(&self) -> u8 {
        self.control
    }

    pub fn write_control(&mut self, value: u8) {
        self.control = value & MODE_MASK;
    }

    pub fn maps(&self, cpu: Cpu) -> bool {
        self.window(cpu).is_some()
    }

    pub fn bytes(&self, cpu: Cpu) -> &[u8] {
        self.window(cpu).map_or(&[], |window| {
            &self.ram.bytes()[window.base..window.base + window.len]
        })
    }

    pub fn read(&self, cpu: Cpu, offset: usize, width: Width) -> Option<u32> {
        let window = self.window(cpu)?;
        Some(
            self.ram
                .read(window.base + (offset & (window.len - 1)), width),
        )
    }

    pub fn write(&mut self, cpu: Cpu, offset: usize, width: Width, value: u32) -> bool {
        let Some(window) = self.window(cpu) else {
            return false;
        };
        self.ram
            .write(window.base + (offset & (window.len - 1)), width, value);
        true
    }

    pub fn code_offset(&self, cpu: Cpu, offset: usize) -> Option<usize> {
        let window = self.window(cpu)?;
        Some(window.base + (offset & (window.len - 1)))
    }

    pub fn ram(&self) -> &Ram {
        &self.ram
    }

    #[cfg(jit)]
    pub fn fast_window(&mut self, cpu: Cpu, base: u32, size: u64) -> Option<(jit::Window, u32)> {
        let window = self.window(cpu)?;
        let fast = self.ram.window(base, size).skip(window.base as u32);
        Some((fast, window.len as u32))
    }

    pub fn ram_mut(&mut self) -> &mut Ram {
        &mut self.ram
    }

    fn window(&self, cpu: Cpu) -> Option<Window> {
        let full = Window {
            base: 0,
            len: SHARED_WRAM_SIZE,
        };
        let first = Window { base: 0, len: HALF };
        let second = Window {
            base: HALF,
            len: HALF,
        };
        match (self.control, cpu) {
            (0, Cpu::Arm9) | (3, Cpu::Arm7) => Some(full),
            (1, Cpu::Arm9) | (2, Cpu::Arm7) => Some(second),
            (2, Cpu::Arm9) | (1, Cpu::Arm7) => Some(first),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_mode(mode: u8) -> SharedWram {
        let mut wram = SharedWram::default();
        wram.write_control(mode);
        wram
    }

    fn fill_halves(wram: &mut SharedWram) {
        let mode = wram.control();
        wram.write_control(0);
        wram.write(Cpu::Arm9, 0x0000, Width::Word, 0x1111_1111);
        wram.write(Cpu::Arm9, HALF, Width::Word, 0x2222_2222);
        wram.write_control(mode);
    }

    #[test]
    fn mode_0_gives_the_arm9_all_32k_and_the_arm7_none() {
        let mut wram = with_mode(0);
        fill_halves(&mut wram);
        assert_eq!(wram.read(Cpu::Arm9, 0x0000, Width::Word), Some(0x1111_1111));
        assert_eq!(wram.read(Cpu::Arm9, 0x4000, Width::Word), Some(0x2222_2222));
        assert_eq!(wram.read(Cpu::Arm9, 0x8000, Width::Word), Some(0x1111_1111));
        assert_eq!(wram.read(Cpu::Arm7, 0x0000, Width::Word), None);
    }

    #[test]
    fn mode_1_gives_the_arm9_the_second_half_and_the_arm7_the_first() {
        let mut wram = with_mode(1);
        fill_halves(&mut wram);
        assert_eq!(wram.read(Cpu::Arm9, 0x0000, Width::Word), Some(0x2222_2222));
        assert_eq!(wram.read(Cpu::Arm9, 0x4000, Width::Word), Some(0x2222_2222));
        assert_eq!(wram.read(Cpu::Arm7, 0x0000, Width::Word), Some(0x1111_1111));
        assert_eq!(wram.read(Cpu::Arm7, 0x4000, Width::Word), Some(0x1111_1111));
    }

    #[test]
    fn mode_2_gives_the_arm9_the_first_half_and_the_arm7_the_second() {
        let mut wram = with_mode(2);
        fill_halves(&mut wram);
        assert_eq!(wram.read(Cpu::Arm9, 0x4000, Width::Word), Some(0x1111_1111));
        assert_eq!(wram.read(Cpu::Arm7, 0x0000, Width::Word), Some(0x2222_2222));
    }

    #[test]
    fn mode_3_gives_the_arm7_all_32k_and_the_arm9_none() {
        let mut wram = with_mode(3);
        fill_halves(&mut wram);
        assert_eq!(wram.read(Cpu::Arm7, 0x4000, Width::Word), Some(0x2222_2222));
        assert_eq!(wram.read(Cpu::Arm9, 0x0000, Width::Word), None);
        assert!(!wram.write(Cpu::Arm9, 0, Width::Word, 0));
    }

    #[test]
    fn a_write_through_one_cpu_shows_through_the_other_after_a_switch() {
        let mut wram = with_mode(1);
        wram.write(Cpu::Arm7, 0x10, Width::Half, 0xCAFE);
        wram.write_control(2);
        assert_eq!(wram.read(Cpu::Arm9, 0x10, Width::Half), Some(0xCAFE));
    }

    #[test]
    fn control_keeps_only_the_mode_bits() {
        assert_eq!(with_mode(0xFF).control(), 3);
    }
}
