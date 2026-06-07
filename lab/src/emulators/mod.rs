//! Concrete [`crate::emulator::FrameEmulator`] adapters.

mod citrine;
mod sameboy;

pub use citrine::CitrineEmulator;
pub use sameboy::SameBoyEmulator;
