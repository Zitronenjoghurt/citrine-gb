use super::credit::{Counter, StepSource};
use crate::storage::Backend;

const FILE: &str = "phone-steps.bin";
const SAME_BOOT_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub count: u64,
    pub boot: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    at: u64,
    count: u64,
    boot: u64,
}

impl Anchor {
    const SIZE: usize = 24;

    fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..8].copy_from_slice(&self.at.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.count.to_le_bytes());
        bytes[16..].copy_from_slice(&self.boot.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: &[u8; Self::SIZE] = bytes.try_into().ok()?;
        let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or([0; 8]));
        Some(Self {
            at: word(0),
            count: word(8),
            boot: word(16),
        })
    }

    fn same_boot(self, reading: Reading) -> bool {
        self.boot.abs_diff(reading.boot) <= SAME_BOOT_SECS && reading.count >= self.count
    }
}

pub struct PhoneSteps {
    reading: Box<dyn FnMut() -> Option<Reading>>,
    anchor: Option<Anchor>,
    backend: Backend,
}

impl PhoneSteps {
    pub fn new(reading: Box<dyn FnMut() -> Option<Reading>>, backend: Backend) -> Self {
        let anchor = backend
            .read(super::store::NAMESPACE, FILE)
            .and_then(|bytes| Anchor::from_bytes(&bytes));
        Self {
            reading,
            anchor,
            backend,
        }
    }

    fn counted(&self, reading: Reading, from: u64) -> u64 {
        match self.anchor {
            Some(anchor) if anchor.same_boot(reading) && anchor.at >= from => {
                reading.count - anchor.count
            }
            Some(anchor) if anchor.same_boot(reading) => 0,
            _ if reading.boot >= from => reading.count,
            _ => 0,
        }
    }
}

impl StepSource for PhoneSteps {
    fn counter(&self) -> Counter {
        Counter::StepSensor
    }

    fn steps(&mut self, from: u64, to: u64) -> Option<u32> {
        let reading = (self.reading)()?;
        let steps = self.counted(reading, from);
        let anchor = Anchor {
            at: to,
            count: reading.count,
            boot: reading.boot,
        };
        if let Err(err) = self
            .backend
            .write(super::store::NAMESPACE, FILE, &anchor.to_bytes())
        {
            log::error!("failed to save the phone's step count: {err}");
        }
        self.anchor = Some(anchor);
        Some(u32::try_from(steps).unwrap_or(u32::MAX))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const BOOT: u64 = 1_000;

    fn root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("citrine-phone-steps-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn sensor(root: &std::path::Path, shared: &Rc<Cell<Option<Reading>>>) -> PhoneSteps {
        let shared = shared.clone();
        PhoneSteps::new(
            Box::new(move || shared.get()),
            Backend::at(root.to_path_buf()),
        )
    }

    fn at(count: u64) -> Option<Reading> {
        Some(Reading { count, boot: BOOT })
    }

    #[test]
    fn steps_count_from_the_last_reading() {
        let dir = root("count");
        let phone = Rc::new(Cell::new(at(5_000)));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(steps.steps(2_000, 2_010), Some(0));
        phone.set(at(5_120));
        assert_eq!(steps.steps(2_010, 2_020), Some(120));
        assert_eq!(steps.steps(2_020, 2_030), Some(0));
    }

    #[test]
    fn no_reading_credits_nothing_and_keeps_the_window() {
        let dir = root("none");
        let phone = Rc::new(Cell::new(None));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(steps.steps(2_000, 2_010), None);
    }

    #[test]
    fn steps_taken_while_the_app_was_closed_count_after_a_restart() {
        let dir = root("restart");
        let phone = Rc::new(Cell::new(at(300)));
        sensor(&dir, &phone).steps(2_000, 2_010);
        phone.set(at(1_300));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(steps.steps(2_010, 9_000), Some(1_000));
    }

    #[test]
    fn a_reboot_counts_the_steps_since_the_boot() {
        let dir = root("reboot");
        let phone = Rc::new(Cell::new(at(4_000)));
        let mut steps = sensor(&dir, &phone);
        steps.steps(2_000, 2_010);
        phone.set(Some(Reading {
            count: 250,
            boot: 3_000,
        }));
        assert_eq!(steps.steps(2_010, 4_000), Some(250));
    }

    #[test]
    fn a_window_another_device_moved_counts_nothing_twice() {
        let dir = root("moved");
        let phone = Rc::new(Cell::new(at(100)));
        let mut steps = sensor(&dir, &phone);
        steps.steps(2_000, 2_010);
        phone.set(at(900));
        assert_eq!(steps.steps(5_000, 5_010), Some(0));
        phone.set(at(950));
        assert_eq!(steps.steps(5_010, 5_020), Some(50));
    }
}
