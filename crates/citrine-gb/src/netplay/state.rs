use super::pair::{Cable, LinkPort};
use crate::console::Rollback;
use std::io::Write;

pub(crate) struct Snapshot<C: LinkPort, const N: usize = 2> {
    pub(crate) frame: Option<u32>,
    pub(crate) consoles: [C; N],
    pub(crate) cable: Cable<C::Message, N>,
}

impl<C: Rollback + LinkPort, const N: usize> Snapshot<C, N> {
    pub(crate) fn new(consoles: &[C; N]) -> Self {
        Self {
            frame: None,
            consoles: consoles.each_ref().map(Rollback::clone_state),
            cable: Cable::default(),
        }
    }

    pub(crate) fn save(&mut self, frame: u32, consoles: &[C; N], cable: &Cable<C::Message, N>) {
        self.frame = Some(frame);
        for (slot, gb) in self.consoles.iter_mut().zip(consoles) {
            slot.copy_state_from(gb);
        }
        self.cable.copy_from(cable);
    }

    pub(crate) fn load(&self, consoles: &mut [C; N], cable: &mut Cable<C::Message, N>) {
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

    pub(crate) fn resume(hash: u64) -> Self {
        Self(hash)
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

pub(crate) fn hash_consoles<C: Rollback + LinkPort, const N: usize>(
    scratch: &mut C,
    consoles: &[C; N],
    cable: &Cable<C::Message, N>,
) -> [u64; N] {
    std::array::from_fn(|i| {
        let mut fnv = Fnv::resume(consoles[i].state_hash(scratch));
        let _ = fnv.write_all(&[cable.plugged as u8]);
        let _ = fnv.write_all(&cable.clocks[i].to_le_bytes());
        for (due, message) in &cable.in_flight[i] {
            let _ = fnv.write_all(&due.to_le_bytes());
            let _ = rmp_serde::encode::write(&mut fnv, message);
        }
        fnv.finish()
    })
}
