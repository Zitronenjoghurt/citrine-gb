use std::cell::Cell;

use super::{ERASED, fitted};

const BLOCK_BYTES: usize = 8;
const BLOCK_BITS: u8 = 64;
const COMMAND_BITS: u8 = 2;
const READ_REQUEST: u8 = 0b11;
const WRITE_REQUEST: u8 = 0b10;
const JUNK_BITS: u8 = 4;
const READ_BITS: u8 = JUNK_BITS + BLOCK_BITS;
const READY: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EepromSize {
    Small,
    Large,
}

impl EepromSize {
    fn bytes(self) -> usize {
        match self {
            Self::Small => 0x200,
            Self::Large => 0x2000,
        }
    }

    fn address_bits(self) -> u8 {
        match self {
            Self::Small => 6,
            Self::Large => 14,
        }
    }

    fn from_dma_units(units: u32) -> Option<Self> {
        let request = |size: Self| {
            let base = u32::from(COMMAND_BITS + size.address_bits() + 1);
            [base, base + u32::from(BLOCK_BITS)]
        };
        [Self::Small, Self::Large]
            .into_iter()
            .find(|&size| request(size).contains(&units))
    }

    fn from_file_len(len: usize) -> Self {
        if len > Self::Small.bytes() {
            Self::Large
        } else {
            Self::Small
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Request {
    received: u8,
    command: u8,
    address: u16,
    value: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Reading {
    block: usize,
    bit: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Eeprom {
    size: Option<EepromSize>,
    data: Vec<u8>,
    request: Request,
    reading: Cell<Option<Reading>>,
}

impl Eeprom {
    pub fn new(size: Option<EepromSize>) -> Self {
        Self {
            size,
            data: size.map_or_else(Vec::new, |size| vec![ERASED; size.bytes()]),
            request: Request::default(),
            reading: Cell::new(None),
        }
    }

    pub fn size(&self) -> Option<EepromSize> {
        self.size
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn load(&mut self, bytes: &[u8]) {
        let size = self
            .size
            .unwrap_or_else(|| EepromSize::from_file_len(bytes.len()));
        self.size = Some(size);
        self.data = fitted(bytes, size.bytes());
    }

    pub fn start_transfer(&mut self, units: u32) {
        self.request = Request::default();
        if let Some(size) = EepromSize::from_dma_units(units) {
            self.resize(size);
        }
    }

    pub fn read(&self) -> u16 {
        let Some(Reading { block, bit }) = self.reading.get() else {
            return READY;
        };
        let next = bit + 1;
        self.reading
            .set((next < READ_BITS).then_some(Reading { block, bit: next }));
        if bit < JUNK_BITS {
            return 0;
        }
        let index = usize::from(bit - JUNK_BITS);
        let byte = self.data[block * BLOCK_BYTES + index / 8];
        u16::from(byte >> (7 - index % 8) & 1)
    }

    pub fn write(&mut self, bit: bool) -> bool {
        let size = self.size.unwrap_or(EepromSize::Small);
        self.resize(size);
        let address_end = COMMAND_BITS + size.address_bits();
        let request = &mut self.request;
        if request.received == 0 && !bit {
            return false;
        }
        let position = request.received;
        request.received += 1;
        if position < COMMAND_BITS {
            request.command = request.command << 1 | u8::from(bit);
            return false;
        }
        if position < address_end {
            request.address = request.address << 1 | u16::from(bit);
            return false;
        }
        let done = *request;
        match done.command {
            WRITE_REQUEST if position < address_end + BLOCK_BITS => {
                request.value = done.value << 1 | u64::from(bit);
                false
            }
            WRITE_REQUEST => {
                self.request = Request::default();
                self.reading.set(None);
                let start = self.block(done.address) * BLOCK_BYTES;
                self.data[start..start + BLOCK_BYTES].copy_from_slice(&done.value.to_be_bytes());
                true
            }
            READ_REQUEST => {
                self.request = Request::default();
                let block = self.block(done.address);
                self.reading.set(Some(Reading { block, bit: 0 }));
                false
            }
            _ => {
                self.request = Request::default();
                false
            }
        }
    }

    fn block(&self, address: u16) -> usize {
        usize::from(address) % (self.data.len() / BLOCK_BYTES)
    }

    fn resize(&mut self, size: EepromSize) {
        if self.size != Some(size) {
            self.size = Some(size);
            let data = std::mem::take(&mut self.data);
            self.data = fitted(&data, size.bytes());
        }
    }
}
