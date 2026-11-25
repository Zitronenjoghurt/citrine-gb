use serde::Serializer;
use std::cell::Cell;

thread_local! {
    static HASHING: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "persistence")]
struct Hashing(bool);

#[cfg(feature = "persistence")]
impl Drop for Hashing {
    fn drop(&mut self) {
        HASHING.set(self.0);
    }
}

#[cfg(feature = "persistence")]
pub(crate) fn hashing<R>(hash: impl FnOnce() -> R) -> R {
    let _restore = Hashing(HASHING.replace(true));
    hash()
}

pub(crate) fn serialize<S: Serializer>(dirty: &bool, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_bool(*dirty && !HASHING.get())
}
