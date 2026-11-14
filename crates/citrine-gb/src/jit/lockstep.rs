use crate::gba::cpu::Core;
use crate::gba::interrupt::Interrupts;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct BlockEnd<const V5: bool> {
    retired: u64,
    clock: u64,
    start: u32,
    core: Core<V5>,
    settled: Option<Interrupts>,
}

#[derive(Debug)]
pub struct Track<const V5: bool> {
    retired: u64,
    recording: bool,
    ends: Vec<BlockEnd<V5>>,
    expected: VecDeque<BlockEnd<V5>>,
    mismatch: Option<String>,
    settling: Option<Option<BlockEnd<V5>>>,
}

impl<const V5: bool> Default for Track<V5> {
    fn default() -> Self {
        Self {
            retired: 0,
            recording: false,
            ends: Vec::new(),
            expected: VecDeque::new(),
            mismatch: None,
            settling: None,
        }
    }
}

impl<const V5: bool> Track<V5> {
    pub fn record(&mut self) {
        self.recording = true;
    }

    pub fn take(&mut self) -> Vec<BlockEnd<V5>> {
        std::mem::take(&mut self.ends)
    }

    pub fn expect(&mut self, ends: Vec<BlockEnd<V5>>) {
        self.expected.extend(ends);
    }

    pub fn retired(&self) -> u64 {
        self.retired
    }

    pub fn verdict(&mut self, cpu: &str) -> Result<(), String> {
        if let Some(mismatch) = self.mismatch.take() {
            return Err(format!("{cpu} {mismatch}"));
        }
        match self.expected.front() {
            Some(end) => Err(format!(
                "the {cpu} interpreter stopped at {} instructions, the JIT block from {:08X} ended at {}",
                self.retired, end.start, end.retired
            )),
            None => Ok(()),
        }
    }

    pub fn ran(&mut self, start: u32, retired: u32, core: &Core<V5>, clock: u64) {
        self.retired += u64::from(retired);
        if self.recording {
            self.ends.push(BlockEnd {
                retired: self.retired,
                clock,
                start,
                core: core.clone(),
                settled: None,
            });
            self.settling = Some(None);
        }
    }

    pub fn stepped(&mut self, core: &Core<V5>, clock: u64) {
        self.retired += 1;
        let Some(end) = self.expected.front() else {
            return;
        };
        if end.retired != self.retired {
            return;
        }
        if self.mismatch.is_none() && (end.clock != clock || end.core != *core) {
            self.mismatch = Some(format!(
                "block from {:08X}, {} instructions in: JIT clock {} {:?}, interpreter clock {} {:?}",
                end.start, end.retired, end.clock, end.core, clock, core
            ));
        }
        self.settling = self.expected.pop_front().map(Some);
    }

    pub fn leaped<'a>(&mut self, count: u64, state: impl Fn(u64) -> (u64, &'a Core<V5>))
    where
        Core<V5>: 'a,
    {
        let start = self.retired;
        self.retired += count;
        while let Some(end) = self.expected.front() {
            if end.retired > self.retired || end.retired <= start {
                break;
            }
            let (clock, core) = state(end.retired - start);
            if self.mismatch.is_none() && (end.clock != clock || end.core != *core) {
                self.mismatch = Some(format!(
                    "block from {:08X}, {} instructions in, inside a skipped wait loop: JIT clock {} {:?}, skip clock {} {:?}",
                    end.start, end.retired, end.clock, end.core, clock, core
                ));
            }
            self.expected.pop_front();
        }
    }

    pub fn settle(&mut self, interrupts: Interrupts) {
        match self.settling.take() {
            Some(None) => {
                if let Some(end) = self.ends.last_mut() {
                    end.settled = Some(interrupts);
                }
            }
            Some(Some(end)) if self.mismatch.is_none() && end.settled != Some(interrupts) => {
                self.mismatch = Some(format!(
                    "block from {:08X}, {} instructions in, settled: JIT {:?}, interpreter {:?}",
                    end.start, end.retired, end.settled, interrupts
                ));
            }
            Some(Some(_)) | None => {}
        }
    }
}
