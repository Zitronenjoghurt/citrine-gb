use super::Nds;
use crate::console::{Console, ConsoleKind, Keys, Timing, Touch, VideoSpec};
#[cfg(feature = "persistence")]
use crate::error::GbResult;
#[cfg(feature = "persistence")]
use crate::persistence::battery_save::BatterySave;
#[cfg(feature = "persistence")]
use crate::persistence::desmume_footer;
#[cfg(feature = "persistence")]
use crate::persistence::state::StateBody;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use std::sync::Arc;

const CHANNEL_NAMES: [&str; 0] = [];

impl Console for Nds {
    fn kind(&self) -> ConsoleKind {
        ConsoleKind::Nds
    }

    fn video(&self) -> VideoSpec {
        ConsoleKind::Nds.video()
    }

    fn timing(&self) -> Timing {
        ConsoleKind::Nds.timing()
    }

    fn run_frame(&mut self) {
        Nds::run_frame(self);
    }

    fn frame(&self) -> &[u8] {
        Nds::frame(self)
    }

    fn take_audio(&mut self, out: &mut Vec<f32>) {
        Nds::take_audio(self, out);
    }

    fn sample_rate(&self) -> u32 {
        Nds::sample_rate(self)
    }

    fn set_sample_rate(&mut self, rate: u32) {
        Nds::set_sample_rate(self, rate);
    }

    fn keys(&self) -> Keys {
        Nds::keys(self)
    }

    fn set_keys(&mut self, keys: Keys) {
        Nds::set_keys(self, keys);
    }

    fn set_touch(&mut self, touch: Option<Touch>) {
        Nds::set_touch(self, touch);
    }

    fn set_mic(&mut self, samples: &[i16]) {
        Nds::set_mic(self, samples);
    }

    fn mic_in_use(&self) -> bool {
        Nds::mic_in_use(self)
    }

    fn soft_reset(&mut self) {
        Nds::soft_reset(self);
    }

    fn audio_channels(&self) -> &'static [&'static str] {
        &CHANNEL_NAMES
    }

    fn set_channel_enabled(&mut self, _index: usize, _enabled: bool) {}

    fn rom_bytes(&self) -> Arc<[u8]> {
        Nds::rom_bytes(self)
    }

    fn title(&self) -> String {
        Nds::title(self)
    }

    fn has_battery(&self) -> bool {
        self.buses().cart().aux().has_save()
    }

    fn has_clock(&self) -> bool {
        false
    }

    #[cfg(feature = "persistence")]
    fn battery(&mut self, force: bool, _now: u64) -> Option<BatterySave> {
        let backup = self.buses_mut().cart_mut().aux_mut().backup_mut()?;
        if !backup.dirty() && !force {
            return None;
        }
        let ram = backup.bytes().to_vec();
        if !force {
            backup.clear_dirty();
        }
        Some(BatterySave::raw(ram))
    }

    #[cfg(feature = "persistence")]
    fn battery_from_file(&self, data: &[u8]) -> BatterySave {
        BatterySave::raw(desmume_footer::strip(data).to_vec())
    }

    #[cfg(feature = "persistence")]
    fn put_battery(&mut self, save: BatterySave, _now: u64) {
        self.buses_mut().cart_mut().aux_mut().load(&save.ram);
    }

    #[cfg(feature = "persistence")]
    fn save_state(&mut self, now: u64) -> GbResult<Vec<u8>> {
        let body = self.state_body(now)?;
        body.pack(body.quality(false))
    }

    #[cfg(feature = "persistence")]
    fn state_body(&mut self, _now: u64) -> GbResult<StateBody> {
        self.finish_picture();
        Ok(StateBody {
            kind: ConsoleKind::Nds,
            data: self.encode_state()?,
        })
    }

    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId {
        self.rom_id
    }

    #[cfg(feature = "rom-id")]
    fn game_sha1(&self) -> [u8; 20] {
        self.game_sha1
    }
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use super::*;
    use crate::gba::wait::Width;
    use crate::nds::machine::NdsFirmware;
    use crate::nds::machine::test_rom::{ARM9_RAM, BRANCH_TO_SELF, rom};

    const READ_FOUR_AT_0100: [u32; 32] = [
        0xE59F_0064,
        0xE59F_1064,
        0xE1C0_10B0,
        0xE3A0_2003,
        0xEB00_0010,
        0xE3A0_2001,
        0xEB00_000E,
        0xE3A0_2000,
        0xEB00_000C,
        0xE28F_604C,
        0xE3A0_5004,
        0xE3A0_2000,
        0xEB00_0008,
        0xE5D0_3002,
        0xE4C6_3001,
        0xE255_5001,
        0x1AFF_FFF9,
        0xE3A0_1902,
        0xE1C0_10B0,
        0xE59F_1020,
        0xE58F_1024,
        0xEAFF_FFFE,
        0xE5C0_2002,
        0xE1D0_30B0,
        0xE313_0080,
        0x1AFF_FFFC,
        0xE12F_FF1E,
        0x0400_01A0,
        0x0000_A040,
        0x600D_F00D,
        0,
        0,
    ];
    const RESULT: u32 = ARM9_RAM + 0x78;
    const DONE: u32 = ARM9_RAM + 0x7C;
    const MELONDS_SAV_LEN: usize = 0x1_0000;

    fn melonds_sav() -> Vec<u8> {
        let mut sav = vec![0xFF; MELONDS_SAV_LEN];
        sav[0x100..0x104].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        sav
    }

    fn dsv(raw: &[u8]) -> Vec<u8> {
        let mut file = raw.to_vec();
        file.extend_from_slice(
            b"|<--Snip above here to create a raw sav by excluding this DeSmuME savedata footer:",
        );
        file.extend_from_slice(&[0; 24]);
        file.extend_from_slice(b"|-DESMUME SAVE-|");
        file
    }

    fn read_by_the_game(file: &[u8]) -> u32 {
        let mut nds = Nds::new(
            rom(&READ_FOUR_AT_0100, &[BRANCH_TO_SELF]),
            NdsFirmware::default(),
        )
        .unwrap();
        let save = nds.battery_from_file(file);
        nds.put_battery(save, 0);
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(DONE, Width::Word), 0x600D_F00D);
        nds.buses.arm9().read(RESULT, Width::Word)
    }

    #[test]
    fn the_game_reads_a_melonds_sav() {
        assert_eq!(read_by_the_game(&melonds_sav()), 0x7856_3412);
    }

    #[test]
    fn the_game_reads_a_desmume_dsv() {
        assert_eq!(read_by_the_game(&dsv(&melonds_sav())), 0x7856_3412);
    }

    #[test]
    fn a_save_comes_back_raw_and_only_when_written() {
        let mut nds = Nds::new(
            rom(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]),
            NdsFirmware::default(),
        )
        .unwrap();
        let save = nds.battery_from_file(&dsv(&melonds_sav()));
        nds.put_battery(save, 0);
        assert!(nds.has_battery());
        assert_eq!(nds.battery(false, 0), None);
        let exported = nds.battery(true, 0).unwrap();
        assert_eq!(exported.to_file(), melonds_sav());
    }

    #[test]
    fn a_soft_reset_keeps_the_save() {
        let mut nds = Nds::new(
            rom(&READ_FOUR_AT_0100, &[BRANCH_TO_SELF]),
            NdsFirmware::default(),
        )
        .unwrap();
        nds.put_battery(BatterySave::raw(melonds_sav()), 0);
        nds.soft_reset();
        nds.run_frame();
        assert_eq!(nds.buses.arm9().read(RESULT, Width::Word), 0x7856_3412);
    }
}
