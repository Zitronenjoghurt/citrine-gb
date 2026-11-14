use crate::jit::lockstep::{BlockEnd, Track};

#[derive(Debug, Default)]
pub struct Ends {
    arm9: Vec<BlockEnd<true>>,
    arm7: Vec<BlockEnd<false>>,
}

impl Ends {
    pub fn len(&self) -> usize {
        self.arm9.len() + self.arm7.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn arm7(&self) -> usize {
        self.arm7.len()
    }
}

#[derive(Debug, Default)]
pub struct Lockstep {
    pub(super) arm9: Track<true>,
    pub(super) arm7: Track<false>,
}

impl Lockstep {
    pub fn record(&mut self) {
        self.arm9.record();
        self.arm7.record();
    }

    pub fn take(&mut self) -> Ends {
        Ends {
            arm9: self.arm9.take(),
            arm7: self.arm7.take(),
        }
    }

    pub fn expect(&mut self, ends: Ends) {
        self.arm9.expect(ends.arm9);
        self.arm7.expect(ends.arm7);
    }

    pub fn retired(&self) -> u64 {
        self.arm9.retired() + self.arm7.retired()
    }

    pub fn verdict(&mut self) -> Result<(), String> {
        self.arm9.verdict("ARM9")?;
        self.arm7.verdict("ARM7")
    }
}
