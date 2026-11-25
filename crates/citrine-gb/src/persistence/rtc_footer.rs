use crate::gb::cartridge::RtcRegisters;

pub const RTC_FOOTER_LEN: usize = 48;
const RTC_FOOTER_32_BIT_LEN: usize = 44;
const SAMEBOY_LEGACY_LEN: usize = 13;
const REGISTERS_LEN: usize = 20;

pub const RTC_FILE_LEN: usize = 32;
const RTC_FILE_OLD_LEN: usize = 40;
const RTC_FILE_LATCHED: usize = 5;
const RTC_FILE_TIMESTAMP: usize = 16;

pub const EARLIEST_TIMESTAMP: u64 = 852_076_800;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct RtcFooter {
    pub real: RtcRegisters,
    pub latched: RtcRegisters,
    pub timestamp: u64,
}

impl RtcFooter {
    pub fn encode(&self) -> [u8; RTC_FOOTER_LEN] {
        let mut out = [0u8; RTC_FOOTER_LEN];
        for (i, value) in self
            .real
            .to_bytes()
            .into_iter()
            .chain(self.latched.to_bytes())
            .enumerate()
        {
            out[i * 4] = value;
        }
        out[REGISTERS_LEN * 2..].copy_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        match data.len() {
            RTC_FOOTER_LEN => Some(Self {
                real: registers_at(data, 0),
                latched: registers_at(data, REGISTERS_LEN),
                timestamp: u64::from_le_bytes(data[REGISTERS_LEN * 2..].try_into().ok()?),
            }),
            RTC_FOOTER_32_BIT_LEN => Some(Self {
                real: registers_at(data, 0),
                latched: registers_at(data, REGISTERS_LEN),
                timestamp: u32::from_le_bytes(data[REGISTERS_LEN * 2..].try_into().ok()?) as u64,
            }),
            SAMEBOY_LEGACY_LEN => {
                let real = RtcRegisters::from_bytes(data[..5].try_into().ok()?);
                Some(Self {
                    real,
                    latched: real,
                    timestamp: u64::from_le_bytes(data[5..].try_into().ok()?),
                })
            }
            _ => None,
        }
    }

    pub fn decode_rtc_file(data: &[u8]) -> Option<Self> {
        if data.len() != RTC_FILE_LEN && data.len() != RTC_FILE_OLD_LEN {
            return None;
        }
        Some(Self {
            real: RtcRegisters::from_bytes(data[..RTC_FILE_LATCHED].try_into().ok()?),
            latched: RtcRegisters::from_bytes(
                data[RTC_FILE_LATCHED..RTC_FILE_LATCHED * 2]
                    .try_into()
                    .ok()?,
            ),
            timestamp: u64::from_le_bytes(
                data[RTC_FILE_TIMESTAMP..RTC_FILE_TIMESTAMP + 8]
                    .try_into()
                    .ok()?,
            ),
        })
    }

    pub fn encode_rtc_file(&self) -> [u8; RTC_FILE_LEN] {
        let mut out = [0u8; RTC_FILE_LEN];
        out[..RTC_FILE_LATCHED].copy_from_slice(&self.real.to_bytes());
        out[RTC_FILE_LATCHED..RTC_FILE_LATCHED * 2].copy_from_slice(&self.latched.to_bytes());
        out[RTC_FILE_TIMESTAMP..RTC_FILE_TIMESTAMP + 8]
            .copy_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn seconds_until(&self, now: u64) -> u64 {
        if self.timestamp < EARLIEST_TIMESTAMP {
            return 0;
        }
        now.saturating_sub(self.timestamp)
    }
}

fn registers_at(data: &[u8], offset: usize) -> RtcRegisters {
    let mut bytes = [0u8; 5];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = data[offset + i * 4];
    }
    RtcRegisters::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::console::Console;
    use crate::gb::cartridge::{RAM_BANK_SIZE, RTC_CYCLES_PER_SECOND};
    use crate::gb::{GameBoy, GbModel};
    use crate::persistence::rtc_footer::{RTC_FILE_LEN, RTC_FOOTER_LEN, RtcFooter};
    use crate::persistence::sram_dump::SramDump;
    use crate::rom::Rom;

    const NOW: u64 = 1_761_400_000;

    fn footer() -> RtcFooter {
        RtcFooter {
            real: RtcRegisters::from_bytes([12, 34, 5, 0xAB, 0x41]),
            latched: RtcRegisters::from_bytes([11, 34, 5, 0xAB, 0x01]),
            timestamp: NOW,
        }
    }

    #[test]
    fn encodes_registers_as_little_endian_words_then_the_timestamp() {
        let bytes = footer().encode();
        assert_eq!(
            bytes[..20],
            [
                12, 0, 0, 0, 34, 0, 0, 0, 5, 0, 0, 0, 0xAB, 0, 0, 0, 0x41, 0, 0, 0
            ]
        );
        assert_eq!(
            bytes[20..40],
            [
                11, 0, 0, 0, 34, 0, 0, 0, 5, 0, 0, 0, 0xAB, 0, 0, 0, 0x01, 0, 0, 0
            ]
        );
        assert_eq!(bytes[40..], NOW.to_le_bytes());
    }

    #[test]
    fn round_trips() {
        assert_eq!(RtcFooter::decode(&footer().encode()), Some(footer()));
    }

    #[test]
    fn reads_the_44_byte_footer_with_a_32_bit_timestamp() {
        let mut bytes = footer().encode()[..44].to_vec();
        bytes[40..44].copy_from_slice(&(NOW as u32).to_le_bytes());
        assert_eq!(RtcFooter::decode(&bytes), Some(footer()));
    }

    #[test]
    fn reads_the_old_13_byte_sameboy_footer() {
        let mut bytes = vec![12, 34, 5, 0xAB, 0x41];
        bytes.extend_from_slice(&NOW.to_le_bytes());
        let decoded = RtcFooter::decode(&bytes).expect("decoded");
        assert_eq!(decoded.real, footer().real);
        assert_eq!(decoded.latched, footer().real);
        assert_eq!(decoded.timestamp, NOW);
    }

    #[test]
    fn rejects_other_lengths() {
        assert_eq!(RtcFooter::decode(&[]), None);
        assert_eq!(RtcFooter::decode(&[0; 47]), None);
    }

    #[test]
    fn unused_bits_are_dropped() {
        let mut bytes = [0xFFu8; RTC_FOOTER_LEN];
        bytes[40..].copy_from_slice(&NOW.to_le_bytes());
        let decoded = RtcFooter::decode(&bytes).expect("decoded");
        assert_eq!(decoded.real.to_bytes(), [0x3F, 0x3F, 0x1F, 0xFF, 0xC1]);
    }

    #[test]
    fn encodes_the_rtc_file_as_sameboys_rtc_section() {
        let bytes = footer().encode_rtc_file();
        assert_eq!(bytes[..5], [12, 34, 5, 0xAB, 0x41]);
        assert_eq!(bytes[5..10], [11, 34, 5, 0xAB, 0x01]);
        assert_eq!(bytes[10..16], [0; 6]);
        assert_eq!(bytes[16..24], NOW.to_le_bytes());
        assert_eq!(bytes[24..], [0; 8]);
    }

    #[test]
    fn round_trips_the_rtc_file() {
        assert_eq!(
            RtcFooter::decode_rtc_file(&footer().encode_rtc_file()),
            Some(footer())
        );
    }

    #[test]
    fn reads_the_40_byte_rtc_file_of_sameboy_before_0_15() {
        let mut bytes = footer().encode_rtc_file().to_vec();
        bytes.resize(40, 0);
        bytes[24] = 1;
        assert_eq!(RtcFooter::decode_rtc_file(&bytes), Some(footer()));
    }

    #[test]
    fn rejects_rtc_files_of_other_lengths() {
        assert_eq!(RtcFooter::decode_rtc_file(&[0; 31]), None);
        assert_eq!(RtcFooter::decode_rtc_file(&footer().encode()), None);
    }

    #[test]
    fn elapsed_time_ignores_timestamps_from_the_future_or_before_1997() {
        let mut footer = footer();
        assert_eq!(footer.seconds_until(NOW + 90), 90);
        assert_eq!(footer.seconds_until(NOW - 5), 0);
        footer.timestamp = 1000;
        assert_eq!(footer.seconds_until(NOW), 0);
    }

    const HOUR: u64 = 60 * 60;

    fn rtc_rom(cartridge_type: u8, ram_size_code: u8) -> Vec<u8> {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = cartridge_type;
        data[0x0148] = 0x00;
        data[0x0149] = ram_size_code;
        data
    }

    fn loaded(data: &[u8]) -> GameBoy {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(data)).expect("load");
        gb
    }

    fn rtc_cart() -> GameBoy {
        loaded(&rtc_rom(0x10, 0x02))
    }

    fn set_clock(gb: &mut GameBoy, registers: [u8; 5]) {
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0x4000, 0x0C);
        gb.cartridge.write_naive(0xA000, 0x40);
        for (register, value) in (0x08..=0x0C).zip(registers) {
            gb.cartridge.write_naive(0x4000, register);
            gb.cartridge.write_naive(0xA000, value);
        }
    }

    fn read_clock(gb: &mut GameBoy) -> [u8; 5] {
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0x6000, 0x00);
        gb.cartridge.write_naive(0x6000, 0x01);
        [0x08, 0x09, 0x0A, 0x0B, 0x0C].map(|register| {
            gb.cartridge.write_naive(0x4000, register);
            gb.cartridge.read_naive(0xA000)
        })
    }

    fn write_ram(gb: &mut GameBoy, bytes: &[u8]) {
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0x4000, 0x00);
        for (i, byte) in bytes.iter().enumerate() {
            gb.cartridge.write_naive(0xA000 + i as u16, *byte);
        }
    }

    fn read_ram(gb: &mut GameBoy, len: usize) -> Vec<u8> {
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0x4000, 0x00);
        (0..len)
            .map(|i| gb.cartridge.read_naive(0xA000 + i as u16))
            .collect()
    }

    #[test]
    fn clock_and_rumble_carts_with_a_battery_get_battery_saves() {
        for cartridge_type in [0x0F, 0x10, 0x1E] {
            let mut gb = loaded(&rtc_rom(cartridge_type, 0x02));
            assert!(gb.cartridge.has_battery(), "type {cartridge_type:02X}");
            assert!(gb.poll_sram_dump(true, NOW).is_some());
        }
    }

    #[test]
    fn a_reset_keeps_battery_ram_and_the_clock() {
        let mut gb = rtc_cart();
        write_ram(&mut gb, b"save data");
        set_clock(&mut gb, [12, 34, 5, 6, 0]);
        gb.soft_reset();
        assert_eq!(read_ram(&mut gb, 9), b"save data");
        assert_eq!(read_clock(&mut gb), [12, 34, 5, 6, 0]);
    }

    #[test]
    fn a_reset_clears_ram_without_a_battery() {
        let mut gb = loaded(&rtc_rom(0x12, 0x02));
        assert!(!gb.cartridge.has_battery());
        write_ram(&mut gb, b"scratch");
        gb.soft_reset();
        assert_eq!(read_ram(&mut gb, 7), vec![0; 7]);
    }

    #[test]
    fn the_clock_runs_with_emulated_time() {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [58, 0, 0, 0, 0]);
        gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
        assert_eq!(read_clock(&mut gb), [0, 1, 0, 0, 0]);
    }

    #[test]
    fn the_clock_keeps_real_time_in_double_speed() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        gb.load_rom(&Rom::new(&rtc_rom(0x10, 0x02))).expect("load");
        gb.cgb.double_speed = true;
        set_clock(&mut gb, [58, 0, 0, 0, 0]);
        gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
        assert_eq!(read_clock(&mut gb), [59, 0, 0, 0, 0]);
        gb.run_cycles(2 * RTC_CYCLES_PER_SECOND);
        assert_eq!(read_clock(&mut gb), [0, 1, 0, 0, 0]);
    }

    #[test]
    fn battery_saves_end_with_the_clock_footer() {
        let mut gb = rtc_cart();
        write_ram(&mut gb, b"save data");
        set_clock(&mut gb, [12, 34, 5, 6, 0]);

        let dump = gb.poll_sram_dump(true, NOW).expect("battery");
        let data = dump.as_slice();
        assert_eq!(data.len(), RAM_BANK_SIZE + RTC_FOOTER_LEN);
        assert_eq!(&data[..9], b"save data");

        let footer = RtcFooter::decode(&data[RAM_BANK_SIZE..]).expect("footer");
        assert_eq!(footer.real.to_bytes(), [12, 34, 5, 6, 0]);
        assert_eq!(footer.latched, footer.real);
        assert_eq!(footer.timestamp, NOW);
    }

    #[test]
    fn the_footer_carries_the_running_time_in_both_register_sets() {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [1, 0, 0, 0, 0]);
        read_clock(&mut gb);
        gb.run_cycles(3 * RTC_CYCLES_PER_SECOND);

        let dump = gb.poll_sram_dump(true, NOW).expect("battery");
        let footer = RtcFooter::decode(&dump.as_slice()[RAM_BANK_SIZE..]).expect("footer");
        assert_eq!(footer.real.to_bytes(), [4, 0, 0, 0, 0]);
        assert_eq!(footer.latched, footer.real);
    }

    #[test]
    fn a_clock_only_cart_saves_just_the_footer() {
        let mut gb = loaded(&rtc_rom(0x0F, 0x00));
        let dump = gb.poll_sram_dump(true, NOW).expect("battery");
        assert_eq!(dump.as_slice().len(), RTC_FOOTER_LEN);
    }

    #[test]
    fn loading_a_save_catches_up_with_the_time_since_it_was_written() {
        let mut gb = rtc_cart();
        write_ram(&mut gb, b"kept");
        set_clock(&mut gb, [0, 30, 22, 0x10, 0]);
        let dump = gb.poll_sram_dump(true, NOW).expect("battery");

        let mut later = rtc_cart();
        later.put_sram_dump(SramDump::from_slice(dump.as_slice()), NOW + 3 * HOUR);

        assert_eq!(read_clock(&mut later), [0, 30, 1, 0x11, 0]);
        assert_eq!(read_ram(&mut later, 4), b"kept");
    }

    #[test]
    fn a_halted_clock_does_not_catch_up() {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [1, 2, 3, 4, 0x40]);
        let dump = gb.poll_sram_dump(true, NOW).expect("battery");

        let mut later = rtc_cart();
        later.put_sram_dump(SramDump::from_slice(dump.as_slice()), NOW + 3 * HOUR);
        assert_eq!(read_clock(&mut later), [1, 2, 3, 4, 0x40]);
    }

    fn ticking_cart() -> GameBoy {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [1, 2, 3, 4, 0]);
        read_clock(&mut gb);
        gb.run_cycles(3 * RTC_CYCLES_PER_SECOND + 12_345);
        gb
    }

    #[test]
    fn a_save_state_keeps_the_clock_to_the_cycle() {
        let mut gb = ticking_cart();
        let state = gb.dump_full(NOW).expect("state");

        let back = GameBoy::from_dump(&state, &Rom::new(&rtc_rom(0x10, 0x02)), NOW).expect("load");
        assert_eq!(back.cartridge.rtc(), gb.cartridge.rtc());
        assert_eq!(back.check_hash(), gb.check_hash());
    }

    #[test]
    fn a_save_state_loaded_later_catches_up_and_keeps_the_part_second() {
        let mut gb = ticking_cart();
        let state = gb.dump_full(NOW).expect("state");

        let mut back = GameBoy::from_dump(&state, &Rom::new(&rtc_rom(0x10, 0x02)), NOW + 3 * HOUR)
            .expect("load");
        let rtc = back.cartridge.rtc().expect("clock").clone();
        assert_eq!(
            rtc.sub_second_cycles,
            gb.cartridge.rtc().expect("clock").sub_second_cycles
        );
        assert_eq!(rtc.latched, gb.cartridge.rtc().expect("clock").latched);
        assert_eq!(read_clock(&mut back), [4, 2, 6, 4, 0]);
    }

    #[test]
    fn a_save_without_a_footer_still_loads() {
        let mut old = vec![0u8; RAM_BANK_SIZE];
        old[..3].copy_from_slice(b"old");

        let mut gb = rtc_cart();
        gb.put_sram_dump(SramDump::from_slice(&old), NOW);

        assert_eq!(read_ram(&mut gb, 3), b"old");
        assert_eq!(read_clock(&mut gb), [0, 0, 0, 0, 0]);
    }

    #[test]
    fn carts_without_a_clock_save_only_their_ram() {
        let mut gb = loaded(&rtc_rom(0x13, 0x02));
        let dump = gb.poll_sram_dump(true, NOW).expect("battery");
        assert_eq!(dump.as_slice().len(), RAM_BANK_SIZE);
    }

    #[test]
    fn snapshots_keep_the_clock_and_catch_up_on_load() {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [10, 0, 0, 0, 0]);
        let rom = rtc_rom(0x10, 0x02);

        let state = gb.dump_full(NOW).expect("dump");

        let mut same_time = GameBoy::from_dump(&state, &Rom::new(&rom), NOW).expect("restore");
        assert_eq!(read_clock(&mut same_time), [10, 0, 0, 0, 0]);

        let mut later = GameBoy::from_dump(&state, &Rom::new(&rom), NOW + 65).expect("restore");
        assert_eq!(read_clock(&mut later), [15, 1, 0, 0, 0]);
    }

    #[test]
    fn snapshots_and_exports_do_not_swallow_a_pending_battery_write() {
        let mut gb = rtc_cart();
        write_ram(&mut gb, b"x");
        gb.dump_full(NOW).expect("dump");
        gb.poll_sram_dump(true, NOW).expect("export");
        assert!(gb.poll_sram_dump(false, NOW).is_some());
        assert!(gb.poll_sram_dump(false, NOW).is_none());
    }

    #[test]
    fn the_sameboy_fixture_imports_with_its_clock() {
        let data = include_bytes!("../../tests/fixtures/sameboy-rtc.sav");
        assert_eq!(data.len(), RAM_BANK_SIZE + RTC_FOOTER_LEN);

        let footer = RtcFooter::decode(&data[RAM_BANK_SIZE..]).expect("footer");
        let mut gb = rtc_cart();
        gb.put_sram_dump(SramDump::from_slice(data), footer.timestamp);

        assert_eq!(read_clock(&mut gb), [42, 17, 9, 0x2C, 0x41]);
        assert_eq!(read_ram(&mut gb, 8), b"CITRINE!");
    }

    #[test]
    fn the_sameboy_libretro_pair_imports_with_its_clock() {
        let srm = include_bytes!("../../tests/fixtures/sameboy-libretro.srm");
        let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
        assert_eq!(rtc.len(), RTC_FILE_LEN);

        let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
        let mut gb = rtc_cart();
        gb.put_sram_dump(SramDump::from_slice(srm), footer.timestamp);
        assert!(
            gb.cartridge
                .put_rtc_footer(footer, footer.timestamp + 2 * HOUR)
        );

        assert_eq!(read_clock(&mut gb), [42, 17, 9, 0x2C, 0x41]);
        assert_eq!(read_ram(&mut gb, 8), b"CITRINE!");
    }

    #[test]
    fn a_running_sameboy_libretro_clock_catches_up_into_the_next_day() {
        let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro-rtc3test.rtc");
        let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
        assert_eq!(footer.real.to_bytes(), [59, 59, 23, 0xFF, 0x00]);

        let mut gb = loaded(&rtc_rom(0x0F, 0x00));
        assert!(gb.cartridge.put_rtc_footer(footer, footer.timestamp + 1));
        assert_eq!(read_clock(&mut gb), [0, 0, 0, 0x00, 0x01]);
    }

    #[test]
    fn the_rtc_file_export_matches_what_sameboy_libretro_writes() {
        let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
        let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");

        let mut gb = rtc_cart();
        gb.cartridge.put_rtc_footer(footer, footer.timestamp);
        let exported = gb
            .cartridge
            .rtc_footer(footer.timestamp)
            .expect("clock")
            .encode_rtc_file();
        assert_eq!(exported[..24], rtc[..24]);
        assert_eq!(RtcFooter::decode_rtc_file(&exported), Some(footer));
    }

    #[test]
    fn a_cartridge_without_a_clock_refuses_an_rtc_file() {
        let rtc = include_bytes!("../../tests/fixtures/sameboy-libretro.rtc");
        let footer = RtcFooter::decode_rtc_file(rtc).expect("rtc file");
        let mut gb = loaded(&rtc_rom(0x03, 0x02));
        assert!(!gb.cartridge.put_rtc_footer(footer, footer.timestamp));
        assert!(gb.cartridge.rtc_footer(footer.timestamp).is_none());
    }

    #[test]
    fn a_battery_save_keeps_the_clock_apart_from_the_ram() {
        let mut gb = rtc_cart();
        set_clock(&mut gb, [5, 4, 3, 2, 0]);
        let save = gb.battery(true, NOW).expect("battery");
        assert_eq!(save.ram.len(), RAM_BANK_SIZE);
        assert!(save.clock.is_some());

        let file = save.to_file();
        assert_eq!(file.len(), save.ram.len() + RTC_FOOTER_LEN);
        assert!(file == gb.poll_sram_dump(true, NOW).expect("dump").as_slice());
        assert_eq!(gb.battery_from_file(&file), save);

        let mut later = rtc_cart();
        later.put_battery(later.battery_from_file(&file), NOW);
        assert_eq!(later.battery(true, NOW), Some(save));
    }

    #[test]
    fn a_battery_save_without_a_clock_is_only_ram() {
        let mut gb = loaded(&rtc_rom(0x13, 0x02));
        let save = gb.battery(true, NOW).expect("battery");
        assert_eq!(save.clock, None);
        assert_eq!(save.to_file(), save.ram);
        assert_eq!(gb.battery_from_file(&save.ram), save);
    }

    #[test]
    fn owed_clock_cycles_land_before_each_cartridge_write() {
        let mut lazy = rtc_cart();
        let mut eager = rtc_cart();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..3000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let arg = (seed >> 8) as u8;
            let writes: &[(u16, u8)] = match seed % 6 {
                0 => &[(0x6000, 0x00), (0x6000, 0x01)],
                1 => &[(0x4000, 0x0C), (0xA000, arg & 0x40)],
                2 => &[(0x4000, 0x08), (0xA000, arg % 60)],
                _ => &[],
            };
            for gb in [&mut lazy, &mut eager] {
                for &(addr, value) in writes {
                    gb.cartridge.write_naive(addr, value);
                }
                if seed % 6 == 3 {
                    gb.cartridge.rtc_mut().unwrap().sub_second_cycles = RTC_CYCLES_PER_SECOND - 64;
                }
            }
            for _ in 0..(seed >> 16) % 128 {
                lazy.cartridge.cycle();
                eager.cartridge.cycle();
                eager.cartridge.settle();
            }
            assert_eq!(read_clock(&mut lazy), read_clock(&mut eager));
        }
        lazy.cartridge.settle();
        assert_eq!(lazy.cartridge.rtc(), eager.cartridge.rtc());
    }
}
