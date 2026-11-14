use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const INDEX: u8 = 0b011;
const FRESH: u8 = 0b100;

struct Shared<T> {
    slots: [UnsafeCell<T>; 3],
    middle: AtomicU8,
}

unsafe impl<T: Send> Sync for Shared<T> {}

pub struct Publisher<T> {
    shared: Arc<Shared<T>>,
    back: u8,
}

pub struct Reader<T> {
    shared: Arc<Shared<T>>,
    front: u8,
}

pub fn latest<T: Default>() -> (Publisher<T>, Reader<T>) {
    let shared = Arc::new(Shared {
        slots: std::array::from_fn(|_| UnsafeCell::new(T::default())),
        middle: AtomicU8::new(1),
    });
    (
        Publisher {
            shared: Arc::clone(&shared),
            back: 0,
        },
        Reader { shared, front: 2 },
    )
}

impl<T> Publisher<T> {
    pub fn publish(&mut self, write: impl FnOnce(&mut T)) {
        let slot = unsafe { &mut *self.shared.slots[usize::from(self.back)].get() };
        write(slot);
        self.back = self.shared.middle.swap(self.back | FRESH, Ordering::AcqRel) & INDEX;
    }
}

impl<T> Reader<T> {
    pub fn read(&mut self) -> Option<&T> {
        if self.shared.middle.load(Ordering::Relaxed) & FRESH == 0 {
            return None;
        }
        self.front = self.shared.middle.swap(self.front, Ordering::AcqRel) & INDEX;
        Some(unsafe { &*self.shared.slots[usize::from(self.front)].get() })
    }

    pub fn newest(&mut self) -> &T {
        if self.shared.middle.load(Ordering::Relaxed) & FRESH != 0 {
            self.front = self.shared.middle.swap(self.front, Ordering::AcqRel) & INDEX;
        }
        unsafe { &*self.shared.slots[usize::from(self.front)].get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reader_sees_nothing_until_a_publish_and_then_only_the_newest_once() {
        let (mut publisher, mut reader) = latest::<u32>();
        assert_eq!(reader.read(), None);
        publisher.publish(|slot| *slot = 1);
        publisher.publish(|slot| *slot = 2);
        publisher.publish(|slot| *slot = 3);
        assert_eq!(reader.read(), Some(&3));
        assert_eq!(reader.read(), None);
        publisher.publish(|slot| *slot = 4);
        assert_eq!(reader.read(), Some(&4));
    }

    #[test]
    fn newest_keeps_answering_with_the_last_value_it_saw() {
        let (mut publisher, mut reader) = latest::<u32>();
        assert_eq!(reader.newest(), &0);
        publisher.publish(|slot| *slot = 1);
        publisher.publish(|slot| *slot = 2);
        assert_eq!(reader.newest(), &2);
        assert_eq!(reader.newest(), &2);
        assert_eq!(reader.read(), None);
        publisher.publish(|slot| *slot = 3);
        assert_eq!(reader.newest(), &3);
    }

    #[test]
    fn a_reader_on_another_thread_never_sees_a_torn_or_older_frame() {
        let (mut publisher, mut reader) = latest::<Vec<u64>>();
        let frames = 20_000u64;
        let writer = std::thread::spawn(move || {
            for n in 1..=frames {
                publisher.publish(|frame| {
                    frame.clear();
                    frame.extend(std::iter::repeat_n(n, 64));
                });
            }
        });
        let mut last = 0;
        while last < frames {
            if let Some(frame) = reader.read() {
                let n = frame[0];
                assert!(frame.iter().all(|&v| v == n), "torn frame");
                assert!(n > last, "{n} after {last}");
                last = n;
            } else {
                std::hint::spin_loop();
            }
        }
        writer.join().unwrap();
    }
}
