#[cfg_attr(not(target_arch = "wasm32"), path = "spawn/native.rs")]
#[cfg_attr(
    all(target_arch = "wasm32", target_feature = "atomics"),
    path = "spawn/web.rs"
)]
#[cfg_attr(
    all(target_arch = "wasm32", not(target_feature = "atomics")),
    path = "spawn/plain.rs"
)]
mod imp;

pub(crate) use imp::{Handle, cores, missing, supported, thread};
