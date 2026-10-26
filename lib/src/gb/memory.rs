use crate::gb::GbModel;
use crate::gb::ram_init::RamInit;
use crate::{ReadMemory, WriteMemory};

const WRAM_BANK_SIZE: usize = 0x1000; // 4KiB
const HRAM_SIZE: usize = 127; // Bytes
const IO_SIZE: usize = 128; // Bytes

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Memory {
    #[cfg_attr(feature = "serde", serde(with = "serde_wram"))]
    wram: Vec<[u8; WRAM_BANK_SIZE]>,
    #[cfg_attr(feature = "serde", serde(with = "serde_hram"))]
    hram: [u8; HRAM_SIZE],
    #[cfg_attr(feature = "serde", serde(with = "serde_io"))]
    // ToDo: Put in IO components (e.g. Timer, Serial, Joypad)
    io: [u8; IO_SIZE],
    #[cfg_attr(feature = "serde", serde(default))]
    svbk: u8,
}

impl Memory {
    pub fn new(model: GbModel, ram_init: RamInit) -> Self {
        let mut memory = Self {
            wram: vec![[0; WRAM_BANK_SIZE]; wram_banks(model)],
            hram: [0; HRAM_SIZE],
            io: [0; IO_SIZE],
            svbk: 0,
        };
        memory.fill_power_on(model, ram_init);
        memory
    }

    pub fn soft_reset(&mut self, model: GbModel, ram_init: RamInit) {
        self.wram = vec![[0; WRAM_BANK_SIZE]; wram_banks(model)];
        self.hram = [0; HRAM_SIZE];
        self.io = [0; IO_SIZE];
        self.svbk = 0;
        self.fill_power_on(model, ram_init);
    }

    pub fn ensure_wram_banks(&mut self, model: GbModel) {
        let banks = wram_banks(model);
        if self.wram.len() < banks {
            self.wram.resize(banks, [0; WRAM_BANK_SIZE]);
        }
    }

    pub fn svbk(&self) -> u8 {
        self.svbk | 0xF8
    }

    pub fn set_svbk(&mut self, value: u8) {
        self.svbk = value & 0x07;
    }

    fn switchable_bank(&self) -> usize {
        let bank = (self.svbk as usize).max(1);
        if bank < self.wram.len() { bank } else { 1 }
    }

    fn fill_power_on(&mut self, model: GbModel, ram_init: RamInit) {
        let Some(mut rng) = ram_init.rng() else {
            return;
        };
        for (bank_index, bank) in self.wram.iter_mut().enumerate() {
            for (offset, byte) in bank.iter_mut().enumerate() {
                *byte = rng.wram_byte(bank_index * WRAM_BANK_SIZE + offset, model);
            }
        }
        for (index, byte) in self.hram.iter_mut().enumerate() {
            *byte = rng.hram_byte(index, model);
        }
    }
}

fn wram_banks(model: GbModel) -> usize {
    match model {
        GbModel::Dmg => 2,
        GbModel::Cgb => 8,
    }
}

impl ReadMemory for Memory {
    fn read_naive(&self, addr: u16) -> u8 {
        match addr {
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF => self.wram[self.switchable_bank()][(addr - 0xD000) as usize],
            0xE000..=0xFDFF => self.read_naive(addr - 0x2000), // echo RAM
            0xFF00..=0xFF7F => self.io[(addr - 0xFF00) as usize],
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.io[0x7F],
            _ => 0xFF,
        }
    }
}

impl WriteMemory for Memory {
    fn write_naive(&mut self, addr: u16, value: u8) {
        match addr {
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF => {
                let bank = self.switchable_bank();
                self.wram[bank][(addr - 0xD000) as usize] = value
            }
            0xE000..=0xFDFF => self.write_naive(addr - 0x2000, value), // echo RAM
            0xFF00..=0xFF7F => self.io[(addr - 0xFF00) as usize] = value,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = value,
            0xFFFF => self.io[0x7F] = value,
            _ => {}
        }
    }
}

#[cfg(feature = "serde")]
mod serde_wram {
    use super::WRAM_BANK_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(
        wram: &Vec<[u8; WRAM_BANK_SIZE]>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let slices: Vec<&[u8]> = wram.iter().map(|bank| bank.as_slice()).collect();
        slices.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Vec<[u8; WRAM_BANK_SIZE]>, D::Error> {
        let vec_of_vecs: Vec<Vec<u8>> = Vec::deserialize(d)?;
        let mut wram = Vec::with_capacity(vec_of_vecs.len());

        for vec in vec_of_vecs {
            let bank: [u8; WRAM_BANK_SIZE] = vec
                .try_into()
                .map_err(|_| D::Error::custom("WRAM bank size mismatch"))?;
            wram.push(bank);
        }
        Ok(wram)
    }
}

#[cfg(feature = "serde")]
mod serde_hram {
    use super::HRAM_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(hram: &[u8; HRAM_SIZE], s: S) -> Result<S::Ok, S::Error> {
        hram.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; HRAM_SIZE], D::Error> {
        let vec: Vec<u8> = Vec::deserialize(d)?;
        vec.try_into()
            .map_err(|_| D::Error::custom("HRAM size mismatch"))
    }
}

#[cfg(feature = "serde")]
mod serde_io {
    use super::IO_SIZE;
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    pub fn serialize<S: Serializer>(io: &[u8; IO_SIZE], s: S) -> Result<S::Ok, S::Error> {
        io.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; IO_SIZE], D::Error> {
        let vec: Vec<u8> = Vec::deserialize(d)?;
        vec.try_into()
            .map_err(|_| D::Error::custom("IO size mismatch"))
    }
}
