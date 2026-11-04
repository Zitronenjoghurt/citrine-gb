use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Bytes,
    Items(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
    pub unit: Unit,
}

impl Progress {
    pub fn bytes(done: u64, total: u64) -> Self {
        Self {
            done,
            total: Some(total),
            unit: Unit::Bytes,
        }
    }

    pub fn items(done: u64, total: u64, noun: &'static str) -> Self {
        Self {
            done,
            total: Some(total),
            unit: Unit::Items(noun),
        }
    }

    pub fn unknown() -> Self {
        Self {
            done: 0,
            total: None,
            unit: Unit::Bytes,
        }
    }

    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.filter(|t| *t > 0)?;
        Some((self.done.min(total) as f64 / total as f64) as f32)
    }

    pub fn is_done(&self) -> bool {
        self.total.is_some_and(|total| self.done >= total)
    }

    pub fn amount(&self) -> Option<String> {
        self.amount_at(self.done)
    }

    pub fn widest_amount(&self) -> Option<String> {
        self.amount_at(self.total?)
    }

    fn amount_at(&self, done: u64) -> Option<String> {
        let total = self.total?;
        let done = done.min(total);
        Some(match self.unit {
            Unit::Bytes => bytes_of(done, total),
            Unit::Items(noun) => format!("{done} of {total} {noun}"),
        })
    }
}

fn bytes_of(done: u64, total: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * KB;
    let (done, total) = (done as f64, total as f64);
    if total >= MB {
        format!("{:.1} of {:.1} MB", done / MB, total / MB)
    } else {
        format!("{:.0} of {:.0} KB", (done / KB).ceil(), (total / KB).ceil())
    }
}

#[derive(Clone, Default)]
pub struct Counter {
    done: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

impl Counter {
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn add(&self, amount: u64) {
        self.done.fetch_add(amount, Ordering::Relaxed);
    }

    pub fn bytes(&self) -> Progress {
        match self.total.load(Ordering::Relaxed) {
            0 => Progress::unknown(),
            total => Progress::bytes(self.done.load(Ordering::Relaxed), total),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_counted(
    reader: &mut impl std::io::Read,
    counter: &Counter,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        bytes.extend_from_slice(&chunk[..read]);
        counter.add(read as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_read_in_the_totals_unit() {
        let mb = 1024 * 1024;
        assert_eq!(
            Progress::bytes(mb * 16 / 5, 8 * mb).amount().as_deref(),
            Some("3.2 of 8.0 MB")
        );
        assert_eq!(
            Progress::bytes(40 * 1024, 64 * 1024).amount().as_deref(),
            Some("40 of 64 KB")
        );
        assert_eq!(
            Progress::bytes(1, 1000).amount().as_deref(),
            Some("1 of 1 KB")
        );
    }

    #[test]
    fn items_count_with_their_noun() {
        assert_eq!(
            Progress::items(3, 12, "files").amount().as_deref(),
            Some("3 of 12 files")
        );
    }

    #[test]
    fn an_unknown_size_has_no_amount_or_fraction() {
        let unknown = Progress::unknown();
        assert_eq!(unknown.amount(), None);
        assert_eq!(unknown.fraction(), None);
        assert!(!unknown.is_done());
    }

    #[test]
    fn the_fraction_rises_to_one_and_stays_there() {
        assert_eq!(Progress::bytes(0, 10).fraction(), Some(0.0));
        assert_eq!(Progress::bytes(5, 10).fraction(), Some(0.5));
        assert_eq!(Progress::bytes(12, 10).fraction(), Some(1.0));
        assert!(Progress::bytes(10, 10).is_done());
        assert_eq!(Progress::bytes(0, 0).fraction(), None);
    }

    #[test]
    fn reading_counts_every_byte() {
        let counter = Counter::default();
        let data = vec![7u8; 200_000];
        counter.set_total(data.len() as u64);
        let read = read_counted(&mut data.as_slice(), &counter).unwrap();
        assert_eq!(read, data);
        assert_eq!(counter.bytes(), Progress::bytes(200_000, 200_000));
    }

    #[test]
    fn a_counter_reports_from_another_thread() {
        let counter = Counter::default();
        assert_eq!(counter.bytes(), Progress::unknown());
        let worker = counter.clone();
        std::thread::spawn(move || {
            worker.set_total(100);
            worker.add(60);
        })
        .join()
        .unwrap();
        assert_eq!(counter.bytes(), Progress::bytes(60, 100));
    }
}
