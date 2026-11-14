mod citrine;
mod citrine_nds;
mod melonds;
mod mgba;
mod sameboy;

pub use citrine::CitrineEmulator;
pub use citrine_nds::CitrineNdsEmulator;
pub use melonds::{DS_CYCLES_PER_FRAME, DS_SAMPLE_RATE, DS_VIDEO, DsBios, MelonDsEmulator};
pub use mgba::MgbaEmulator;
pub use sameboy::SameBoyEmulator;
