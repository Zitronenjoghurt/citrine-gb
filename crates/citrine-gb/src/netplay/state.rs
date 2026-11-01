use super::pair::Cable;
use crate::error::GbResult;
use crate::gb::GameBoy;
use crate::gb::ppu::types::color::{ColorCorrection, DmgColors};
use crate::gb::ppu::types::theme::DmgTheme;
use std::io::Write;

impl GameBoy {
    pub(crate) fn clone_state(&self) -> GameBoy {
        GameBoy {
            boot_rom: self.boot_rom.clone(),
            cpu: self.cpu.clone(),
            cartridge: self.cartridge.clone(),
            cgb: self.cgb.clone(),
            #[cfg(feature = "debug")]
            debugger: Default::default(),
            dma: self.dma.clone(),
            hdma: self.hdma.clone(),
            ic: self.ic.clone(),
            memory: self.memory.clone(),
            timer: self.timer.clone(),
            ppu: self.ppu.clone(),
            apu: self.apu.clone_state(),
            joypad: self.joypad.clone(),
            serial: self.serial.clone(),
            model: self.model,
            cycle_counter: self.cycle_counter,
            ram_init: self.ram_init,
        }
    }

    pub(crate) fn copy_state_from(&mut self, source: &GameBoy) {
        let GameBoy {
            boot_rom,
            cpu,
            cartridge,
            cgb,
            #[cfg(feature = "debug")]
                debugger: _,
            dma,
            hdma,
            ic,
            memory,
            timer,
            ppu,
            apu,
            joypad,
            serial,
            model,
            cycle_counter,
            ram_init,
        } = self;
        boot_rom.clone_from(&source.boot_rom);
        cpu.clone_from(&source.cpu);
        cartridge.clone_from(&source.cartridge);
        cgb.clone_from(&source.cgb);
        dma.clone_from(&source.dma);
        hdma.clone_from(&source.hdma);
        ic.clone_from(&source.ic);
        memory.clone_from(&source.memory);
        timer.clone_from(&source.timer);
        let display = (
            ppu.dmg_theme,
            ppu.dmg_colors,
            ppu.color_correction,
            ppu.blind,
        );
        ppu.clone_from(&source.ppu);
        (
            ppu.dmg_theme,
            ppu.dmg_colors,
            ppu.color_correction,
            ppu.blind,
        ) = display;
        apu.copy_state_from(&source.apu);
        joypad.clone_from(&source.joypad);
        serial.clone_from(&source.serial);
        *model = source.model;
        *cycle_counter = source.cycle_counter;
        *ram_init = source.ram_init;
    }
}

pub(crate) struct Snapshot {
    pub(crate) frame: Option<u32>,
    pub(crate) consoles: [GameBoy; 2],
    pub(crate) cable: Cable,
}

impl Snapshot {
    pub(crate) fn new(consoles: &[GameBoy; 2]) -> Self {
        Self {
            frame: None,
            consoles: [consoles[0].clone_state(), consoles[1].clone_state()],
            cable: Cable::default(),
        }
    }

    pub(crate) fn save(&mut self, frame: u32, consoles: &[GameBoy; 2], cable: &Cable) {
        self.frame = Some(frame);
        for (slot, gb) in self.consoles.iter_mut().zip(consoles) {
            slot.copy_state_from(gb);
        }
        self.cable.copy_from(cable);
    }

    pub(crate) fn load(&self, consoles: &mut [GameBoy; 2], cable: &mut Cable) {
        for (gb, slot) in consoles.iter_mut().zip(&self.consoles) {
            gb.copy_state_from(slot);
        }
        cable.copy_from(&self.cable);
    }
}

pub(crate) struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;

    pub(crate) fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub(crate) fn finish(&self) -> u64 {
        self.0
    }
}

impl Write for Fnv {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &byte in buf {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(Self::PRIME);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn hash_consoles(
    scratch: &mut GameBoy,
    consoles: &[GameBoy; 2],
    cable: &Cable,
) -> [u64; 2] {
    [0, 1].map(|i| {
        scratch.copy_state_from(&consoles[i]);
        scratch.ppu.dmg_theme = DmgTheme::default();
        scratch.ppu.dmg_colors = DmgColors::default();
        scratch.ppu.color_correction = ColorCorrection::default();
        let mut fnv = Fnv::new();
        let _ = rmp_serde::encode::write(&mut fnv, &*scratch);
        for bank in scratch.cartridge.ram_banks() {
            let _ = fnv.write_all(bank);
        }
        let _ = fnv.write_all(&[cable.plugged as u8]);
        let _ = fnv.write_all(&cable.clocks[i].to_le_bytes());
        for (due, message) in &cable.in_flight[i] {
            let _ = fnv.write_all(&due.to_le_bytes());
            let _ = rmp_serde::encode::write(&mut fnv, message);
        }
        fnv.finish()
    })
}

pub(crate) fn encode_console(gb: &GameBoy) -> GbResult<Vec<u8>> {
    let ram = gb.cartridge.ram_banks().concat();
    Ok(rmp_serde::to_vec(&(gb, ram))?)
}

pub(crate) fn decode_console(data: &[u8], like: &GameBoy) -> GbResult<GameBoy> {
    let (mut gb, ram): (GameBoy, Vec<u8>) = rmp_serde::from_slice(data)?;
    gb.cartridge.restore_storage(&like.cartridge, &ram);
    gb.memory.ensure_wram_banks(gb.model);
    gb.cgb.ensure_model(gb.model);
    gb.apu.restore(gb.model);
    Ok(gb)
}
