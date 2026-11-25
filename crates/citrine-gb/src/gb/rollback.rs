use super::GameBoy;
use super::ppu::types::color::{ColorCorrection, DmgColors};
use super::ppu::types::theme::DmgTheme;
use crate::console::Rollback;
use crate::error::GbResult;
use crate::netplay::state::CheckHash;
use crate::persistence::state::check_game;
use crate::utils::digest::game_key;
use std::io::Write;

impl GameBoy {
    pub(crate) fn check_hash(&self) -> u64 {
        let mut scratch = self.scratch();
        scratch.serial.forget_link();
        scratch.hashed()
    }

    fn hashed(&mut self) -> u64 {
        self.ppu.dmg_theme = DmgTheme::default();
        self.ppu.dmg_colors = DmgColors::default();
        self.ppu.color_correction = ColorCorrection::default();
        self.cartridge.forget_unsaved();
        let mut hash = CheckHash::new();
        let _ = rmp_serde::encode::write(&mut hash, &*self);
        for bank in self.cartridge.ram_banks() {
            let _ = hash.write_all(bank);
        }
        hash.finish()
    }
}

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
        ppu.settle();
        (
            ppu.dmg_theme,
            ppu.dmg_colors,
            ppu.color_correction,
            ppu.blind,
        ) = display;
        apu.copy_state_from(&source.apu);
        joypad.clone_from(&source.joypad);
        serial.copy_keeping_traffic(&source.serial);
        *model = source.model;
        *cycle_counter = source.cycle_counter;
        *ram_init = source.ram_init;
    }

    fn state_hash(&self, scratch: &mut GameBoy) -> u64 {
        scratch.copy_state_from(self);
        scratch.hashed()
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        let mut out = Vec::new();
        self.encode_state_into(&mut out)?;
        Ok(out)
    }

    fn encode_state_into(&self, out: &mut Vec<u8>) -> GbResult<()> {
        let ram = self.cartridge.ram_banks().concat();
        let game = game_key(self.cartridge.pristine_rom());
        Ok(rmp_serde::encode::write(out, &(self, ram, game))?)
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<GameBoy> {
        let (mut gb, ram, game): (GameBoy, Vec<u8>, u64) = rmp_serde::from_slice(data)?;
        check_game(game, self.cartridge.pristine_rom())?;
        gb.cartridge.restore_storage(&self.cartridge, &ram)?;
        gb.memory.ensure_wram_banks(gb.model);
        gb.cgb.ensure_model(gb.model);
        gb.apu.restore(gb.model);
        gb.ppu.clamp_loaded();
        Ok(gb)
    }

    fn set_blind(&mut self, blind: bool) {
        if self.ppu.blind != blind {
            self.ppu.settle();
        }
        self.ppu.blind = blind;
    }

    fn set_muted(&mut self, muted: bool) {
        self.apu.muted = muted;
    }
}
