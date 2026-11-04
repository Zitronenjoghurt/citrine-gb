use super::GameBoy;
use super::ppu::types::color::{ColorCorrection, DmgColors};
use super::ppu::types::theme::DmgTheme;
use crate::console::Rollback;
use crate::error::GbResult;
use crate::netplay::state::Fnv;
use std::io::Write;

impl Rollback for GameBoy {
    fn clone_state(&self) -> GameBoy {
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

    fn copy_state_from(&mut self, source: &GameBoy) {
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

    fn state_hash(&self, scratch: &mut GameBoy) -> u64 {
        scratch.copy_state_from(self);
        scratch.ppu.dmg_theme = DmgTheme::default();
        scratch.ppu.dmg_colors = DmgColors::default();
        scratch.ppu.color_correction = ColorCorrection::default();
        let mut fnv = Fnv::new();
        let _ = rmp_serde::encode::write(&mut fnv, &*scratch);
        for bank in scratch.cartridge.ram_banks() {
            let _ = fnv.write_all(bank);
        }
        fnv.finish()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        let ram = self.cartridge.ram_banks().concat();
        Ok(rmp_serde::to_vec(&(self, ram))?)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<GameBoy> {
        let (mut gb, ram): (GameBoy, Vec<u8>) = rmp_serde::from_slice(data)?;
        gb.cartridge.restore_storage(&self.cartridge, &ram);
        gb.memory.ensure_wram_banks(gb.model);
        gb.cgb.ensure_model(gb.model);
        gb.apu.restore(gb.model);
        Ok(gb)
    }

    fn set_blind(&mut self, blind: bool) {
        self.ppu.blind = blind;
    }

    fn set_muted(&mut self, muted: bool) {
        self.apu.muted = muted;
    }
}
