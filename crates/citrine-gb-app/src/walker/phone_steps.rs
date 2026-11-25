use super::credit::{Counter, StepSource, Taken};
use crate::storage::Folders;

const FILE: &str = "phone-steps.bin";
const LOG: &str = "phone-steps.log";
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
}

pub struct PhoneSteps {
    reading: Box<dyn FnMut() -> Option<Reading>>,
    anchor: Option<Anchor>,
    backend: Folders,
}

impl PhoneSteps {
    pub fn new(reading: Box<dyn FnMut() -> Option<Reading>>, backend: Folders) -> Self {
        let anchor = backend
            .read(super::store::NAMESPACE, FILE)
            .and_then(|bytes| Anchor::from_bytes(&bytes));
        Self {
            reading,
            anchor,
            backend,
        }
    }

    fn logged(&self, to: u64) -> Vec<Anchor> {
        let since = self.anchor.map_or(0, |anchor| anchor.at);
        let bytes = self
            .backend
            .read(super::store::NAMESPACE, LOG)
            .unwrap_or_default();
        let mut logged: Vec<Anchor> = bytes
            .as_chunks::<{ Anchor::SIZE }>()
            .0
            .iter()
            .filter_map(|bytes| Anchor::from_bytes(bytes))
            .filter(|point| point.at > since && point.at <= to)
            .collect();
        logged.sort_by_key(|point| point.at);
        logged
    }
}

fn split(last: Option<Anchor>, points: &[Anchor], from: u64) -> (Vec<Taken>, Option<Anchor>) {
    let mut last = last;
    let mut taken = Vec::new();
    for &point in points {
        let steps = match last {
            Some(last) if last.boot.abs_diff(point.boot) <= SAME_BOOT_SECS => {
                if point.count < last.count {
                    continue;
                }
                if last.at >= from {
                    point.count - last.count
                } else {
                    0
                }
            }
            _ if point.boot >= from => point.count,
            _ => 0,
        };
        last = Some(point);
        if steps > 0 {
            taken.push(Taken {
                at: point.at,
                steps: u32::try_from(steps).unwrap_or(u32::MAX),
            });
        }
    }
    (taken, last)
}

impl StepSource for PhoneSteps {
    fn counter(&self) -> Counter {
        Counter::StepSensor
    }

    fn steps(&mut self, from: u64, to: u64) -> Option<Vec<Taken>> {
        let mut points = self.logged(to);
        if let Some(reading) = (self.reading)() {
            points.push(Anchor {
                at: to,
                count: reading.count,
                boot: reading.boot,
            });
        }
        if points.is_empty() {
            return None;
        }
        let (taken, last) = split(self.anchor, &points, from);
        let anchor = last.map(|last| Anchor { at: to, ..last })?;
        if let Err(err) = self
            .backend
            .write(super::store::NAMESPACE, FILE, &anchor.to_bytes())
        {
            log::error!("failed to save the phone's step count: {err}");
        }
        self.anchor = Some(anchor);
        Some(taken)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const BOOT: u64 = 1_000;

    fn root(name: &str) -> std::path::PathBuf {
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-phone-steps-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn sensor(root: &std::path::Path, shared: &Rc<Cell<Option<Reading>>>) -> PhoneSteps {
        let shared = shared.clone();
        PhoneSteps::new(
            Box::new(move || shared.get()),
            Folders::at(root.to_path_buf()),
        )
    }

    fn at(count: u64) -> Option<Reading> {
        Some(Reading { count, boot: BOOT })
    }

    fn total(taken: Option<Vec<Taken>>) -> Option<u32> {
        taken.map(|taken| taken.iter().map(|taken| taken.steps).sum())
    }

    fn log(dir: &std::path::Path, points: &[(u64, u64)]) {
        let bytes: Vec<u8> = points
            .iter()
            .flat_map(|&(at, count)| {
                Anchor {
                    at,
                    count,
                    boot: BOOT,
                }
                .to_bytes()
            })
            .collect();
        Folders::at(dir.to_path_buf())
            .write(super::super::store::NAMESPACE, LOG, &bytes)
            .unwrap();
    }

    fn taken(at: u64, steps: u32) -> Taken {
        Taken { at, steps }
    }

    #[test]
    fn steps_count_from_the_last_reading() {
        let dir = root("count");
        let phone = Rc::new(Cell::new(at(5_000)));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(total(steps.steps(2_000, 2_010)), Some(0));
        phone.set(at(5_120));
        assert_eq!(total(steps.steps(2_010, 2_020)), Some(120));
        assert_eq!(total(steps.steps(2_020, 2_030)), Some(0));
    }

    #[test]
    fn no_reading_credits_nothing_and_keeps_the_window() {
        let dir = root("none");
        let phone = Rc::new(Cell::new(None));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(total(steps.steps(2_000, 2_010)), None);
    }

    #[test]
    fn steps_taken_while_the_app_was_closed_count_after_a_restart() {
        let dir = root("restart");
        let phone = Rc::new(Cell::new(at(300)));
        total(sensor(&dir, &phone).steps(2_000, 2_010));
        phone.set(at(1_300));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(total(steps.steps(2_010, 9_000)), Some(1_000));
    }

    #[test]
    fn a_reboot_counts_the_steps_since_the_boot() {
        let dir = root("reboot");
        let phone = Rc::new(Cell::new(at(4_000)));
        let mut steps = sensor(&dir, &phone);
        total(steps.steps(2_000, 2_010));
        phone.set(Some(Reading {
            count: 250,
            boot: 3_000,
        }));
        assert_eq!(total(steps.steps(2_010, 4_000)), Some(250));
    }

    #[test]
    fn a_window_another_device_moved_counts_nothing_twice() {
        let dir = root("moved");
        let phone = Rc::new(Cell::new(at(100)));
        let mut steps = sensor(&dir, &phone);
        total(steps.steps(2_000, 2_010));
        phone.set(at(900));
        assert_eq!(total(steps.steps(5_000, 5_010)), Some(0));
        phone.set(at(950));
        assert_eq!(total(steps.steps(5_010, 5_020)), Some(50));
    }

    #[test]
    fn steps_logged_while_the_app_was_closed_keep_their_minutes() {
        let dir = root("logged");
        let phone = Rc::new(Cell::new(at(300)));
        total(sensor(&dir, &phone).steps(2_000, 2_010));
        log(&dir, &[(1_900, 250), (2_100, 500), (90_000, 900)]);
        phone.set(at(1_000));
        let mut steps = sensor(&dir, &phone);
        assert_eq!(
            steps.steps(2_010, 100_000),
            Some(vec![
                taken(2_100, 200),
                taken(90_000, 400),
                taken(100_000, 100)
            ])
        );
        assert_eq!(steps.steps(100_000, 100_010), Some(vec![]));
    }

    #[test]
    fn the_log_alone_credits_while_the_sensor_has_no_reading_yet() {
        let dir = root("log-only");
        let phone = Rc::new(Cell::new(at(300)));
        total(sensor(&dir, &phone).steps(2_000, 2_010));
        log(&dir, &[(2_070, 340), (2_130, 330), (2_190, 420)]);
        phone.set(None);
        let mut steps = sensor(&dir, &phone);
        assert_eq!(
            steps.steps(2_010, 2_200),
            Some(vec![taken(2_070, 40), taken(2_190, 80)])
        );
        assert_eq!(steps.steps(2_200, 2_210), None);
    }
}
