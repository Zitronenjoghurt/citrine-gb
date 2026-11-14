use super::pair::{Cable, LinkPort};
use crate::console::Rollback;
use citrine_gb_threads::Split;
use std::io::Write;
use xxhash_rust::xxh3::Xxh3;

pub(crate) struct Snapshot<C: LinkPort, const N: usize = 2> {
    pub(crate) frame: Option<u32>,
    pub(crate) consoles: [C; N],
    pub(crate) cable: Cable<C::Message, N>,
}

impl<C: Rollback + LinkPort, const N: usize> Snapshot<C, N> {
    pub(crate) fn new(consoles: &mut [C; N]) -> Self {
        Self {
            frame: None,
            consoles: consoles.each_mut().map(Rollback::snapshot),
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

    pub(crate) fn take(
        &mut self,
        frame: u32,
        consoles: &mut [C; N],
        cable: &Cable<C::Message, N>,
        split: &mut Split<'_>,
    ) {
        self.frame = Some(frame);
        split.scope("snapshot", |scope| {
            for (slot, gb) in self.consoles.iter_mut().zip(consoles) {
                scope.spawn(move || gb.snapshot_into(slot));
            }
        });
        self.cable.copy_from(cable);
    }

    pub(crate) fn load(&self, consoles: &mut [C; N], cable: &mut Cable<C::Message, N>) {
        for (gb, slot) in consoles.iter_mut().zip(&self.consoles) {
            gb.copy_state_from(slot);
        }
        cable.copy_from(&self.cable);
    }
}

pub(crate) struct CheckHash(Xxh3);

impl CheckHash {
    pub(crate) fn new() -> Self {
        Self(Xxh3::new())
    }

    pub(crate) fn finish(&self) -> u64 {
        self.0.digest()
    }
}

impl Write for CheckHash {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn hash_consoles<C: Rollback + LinkPort, const N: usize>(
    scratch: &mut C,
    consoles: &[C; N],
    cable: &Cable<C::Message, N>,
) -> [u64; N] {
    let states = consoles.each_ref().map(|gb| gb.state_hash(scratch));
    with_cable(states, cable)
}

pub(crate) fn hash_consoles_on<C: Rollback + LinkPort, const N: usize>(
    scratches: &mut [C; N],
    consoles: &mut [C; N],
    cable: &Cable<C::Message, N>,
    split: &mut Split<'_>,
) -> [u64; N] {
    let mut states = [0; N];
    split.scope("state check", |scope| {
        for ((state, scratch), gb) in states.iter_mut().zip(scratches).zip(consoles) {
            scope.spawn(move || *state = gb.state_hash(scratch));
        }
    });
    with_cable(states, cable)
}

fn with_cable<M: serde::Serialize, const N: usize>(
    states: [u64; N],
    cable: &Cable<M, N>,
) -> [u64; N] {
    std::array::from_fn(|i| {
        let mut hash = CheckHash::new();
        let _ = hash.write_all(&states[i].to_le_bytes());
        let _ = hash.write_all(&[cable.plugged as u8]);
        let _ = hash.write_all(&cable.clocks[i].to_le_bytes());
        if cable.exact.contains(&true) {
            let _ = hash.write_all(&cable.exact.map(u8::from));
        }
        for (due, message) in &cable.in_flight[i] {
            let _ = hash.write_all(&due.to_le_bytes());
            let _ = rmp_serde::encode::write(&mut hash, message);
        }
        hash.finish()
    })
}
