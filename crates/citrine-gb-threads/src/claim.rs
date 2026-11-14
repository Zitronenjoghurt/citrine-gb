use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Claim<T> {
    busy: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Claim<T> {}

struct Release<'a>(&'a AtomicBool);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl<T> Claim<T> {
    pub fn new(value: T) -> Self {
        Self {
            busy: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }

    pub fn try_with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()?;
        let _release = Release(&self.busy);
        Some(f(unsafe { &mut *self.value.get() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_claim_lends_its_value_and_refuses_a_second_hold_instead_of_waiting() {
        let claim = Claim::new(1u32);
        let nested = claim.try_with(|value| {
            *value += 1;
            claim.try_with(|_| ())
        });
        assert_eq!(nested, Some(None));
        assert_eq!(claim.try_with(|value| *value), Some(2));
    }

    #[test]
    fn into_inner_gives_back_the_value_with_every_change() {
        let claim = Claim::new(vec![1u8]);
        claim.try_with(|value| value.push(2));
        assert_eq!(claim.into_inner(), [1, 2]);
    }

    #[test]
    fn a_panic_inside_frees_the_claim() {
        let claim = Claim::new(0u32);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            claim.try_with(|_| panic!("inside"));
        }));
        assert!(caught.is_err());
        assert_eq!(claim.try_with(|value| *value), Some(0));
    }

    #[test]
    fn threads_never_hold_it_at_once() {
        let claim = Arc::new(Claim::new(0u64));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let claim = Arc::clone(&claim);
                std::thread::spawn(move || {
                    let mut done = 0;
                    while done < 1000 {
                        if claim.try_with(|value| *value += 1).is_some() {
                            done += 1;
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(claim.try_with(|value| *value), Some(4000));
    }
}
