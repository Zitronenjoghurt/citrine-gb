use super::Gba;
#[cfg(feature = "persistence")]
use crate::console::Rollback;
use crate::console::{Console, ConsoleKind, Keys, Timing, VideoSpec};
#[cfg(feature = "persistence")]
use crate::error::GbResult;
#[cfg(feature = "persistence")]
use crate::gba::save::{Backup, SaveType};
use crate::gba::sound::CHANNEL_NAMES;
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::seiko_footer::{SEIKO_FOOTER_LEN, SeikoFooter};
#[cfg(feature = "persistence")]
use crate::persistence::state::{GBA_SAVE_QUALITY, StateBody};
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;

const TITLE: std::ops::Range<usize> = 0xA0..0xAC;
#[cfg(feature = "persistence")]
const CHIP_BLOCK: usize = 512;

impl Console for Gba {
    fn kind(&self) -> ConsoleKind {
        ConsoleKind::Gba
    }

    fn video(&self) -> VideoSpec {
        self.kind().video()
    }

    fn timing(&self) -> Timing {
        self.kind().timing()
    }

    fn run_frame(&mut self) {
        Gba::run_frame(self);
    }

    fn frame(&self) -> &[u8] {
        Gba::frame(self)
    }

    fn take_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.sound_mut().take_audio(out);
    }

    fn sample_rate(&self) -> u32 {
        self.bus.sound().sample_rate()
    }

    fn set_sample_rate(&mut self, rate: u32) {
        self.bus.sound_mut().set_sample_rate(rate);
    }

    fn keys(&self) -> Keys {
        self.keys
    }

    fn set_keys(&mut self, keys: Keys) {
        Gba::set_keys(self, keys);
    }

    fn soft_reset(&mut self) {
        Gba::soft_reset(self);
    }

    fn audio_channels(&self) -> &'static [&'static str] {
        &CHANNEL_NAMES
    }

    fn set_channel_enabled(&mut self, index: usize, enabled: bool) {
        self.bus.sound_mut().set_channel_enabled(index, enabled);
    }

    fn rom_bytes(&self) -> Vec<u8> {
        self.bus.rom().to_vec()
    }

    fn title(&self) -> String {
        let name = self.bus.rom().get(TITLE).unwrap_or_default();
        let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        String::from_utf8_lossy(&name[..end]).trim().to_string()
    }

    fn has_battery(&self) -> bool {
        true
    }

    fn has_clock(&self) -> bool {
        self.cart_devices().rtc
    }

    #[cfg(feature = "persistence")]
    fn battery(&mut self, force: bool, now: u64) -> Option<BatterySave> {
        let backup = self.bus.backup();
        if (!backup.dirty() && !force) || backup.bytes().is_empty() {
            return None;
        }
        let ram = backup.bytes().to_vec();
        let chip = Some(backup.kind()).filter(|&kind| kind != SaveType::Auto);
        if !force {
            self.bus.backup_mut().clear_dirty();
        }
        let seiko = self.bus.cart().clock_footer(now);
        Some(BatterySave {
            ram,
            clock: None,
            seiko,
            chip,
        })
    }

    #[cfg(feature = "persistence")]
    fn battery_from_file(&self, data: &[u8]) -> BatterySave {
        if data.len() % CHIP_BLOCK == SEIKO_FOOTER_LEN {
            let (ram, footer) = data.split_at(data.len() - SEIKO_FOOTER_LEN);
            return BatterySave {
                ram: ram.to_vec(),
                clock: None,
                seiko: SeikoFooter::decode(footer),
                chip: None,
            };
        }
        BatterySave {
            ram: data.to_vec(),
            clock: None,
            seiko: None,
            chip: None,
        }
    }

    #[cfg(feature = "persistence")]
    fn put_battery(&mut self, save: BatterySave, now: u64) {
        if let Some(kind) = save.chip
            && self.bus.backup().kind() == SaveType::Auto
        {
            *self.bus.backup_mut() = Backup::new(kind);
        }
        self.bus.backup_mut().load(&save.ram);
        if let Some(footer) = save.seiko
            && self.cart_devices().rtc
        {
            self.bus.cart_mut().load_clock(&footer, now);
        }
    }

    #[cfg(feature = "persistence")]
    fn save_state(&mut self, now: u64) -> GbResult<Vec<u8>> {
        self.state_body(now)?.pack(GBA_SAVE_QUALITY)
    }

    #[cfg(feature = "persistence")]
    fn state_body(&mut self, _now: u64) -> GbResult<StateBody> {
        Ok(StateBody {
            kind: ConsoleKind::Gba,
            data: self.encode_state()?,
        })
    }

    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId {
        self.rom_id
    }
}
