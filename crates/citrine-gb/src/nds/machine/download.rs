use super::link::AirPort;
use super::{Nds, NdsFirmware};
use crate::console::Keys;
use crate::error::GbResult;
use crate::nds::download::Download;
use crate::nds::wifi::frame::Frame;
use std::sync::Arc;

const ROM_BYTES: usize = 0x600;
const TITLE: &[u8] = b"DOWNLOAD";
const ARM9_OFFSET: u32 = 0x200;
const ARM7_OFFSET: u32 = 0x400;
const ARM9_RAM: u32 = 0x0200_0000;
const ARM7_RAM: u32 = 0x0380_0000;
const ARM9_BINARY: usize = 0x20;
const ARM7_BINARY: usize = 0x30;
const ARM9_HALTS: [u32; 3] = [0xE3A0_0000, 0xEE07_0F90, 0xEAFF_FFFD];
const ARM7_HALTS: [u32; 4] = [0xE3A0_0301, 0xE3A0_1080, 0xE5C0_1301, 0xEAFF_FFFD];

impl Nds {
    pub fn download_play(firmware: NdsFirmware) -> GbResult<Self> {
        let mut nds = Self::new(waiting_rom(), firmware)?;
        let image = super::firmware_image(&nds.firmware);
        nds.download = Some(Box::new(Download::new(
            image.mac(),
            &image.nickname(),
            image.favorite_color(),
        )));
        Ok(nds)
    }

    pub fn download(&self) -> Option<&Download> {
        self.download.as_deref()
    }

    pub(super) fn steer_download(&mut self) {
        let accept = self.keys().contains(Keys::A);
        if let Some(download) = self.download.as_mut()
            && accept
        {
            download.accept();
        }
    }

    pub(super) fn download_busy(&self) -> bool {
        self.download
            .as_ref()
            .is_some_and(|download| download.busy())
    }

    pub(super) fn hear_download(&mut self, frame: Frame) -> Option<Frame> {
        let Some(download) = self.download.as_mut() else {
            return Some(frame);
        };
        download.receive(frame);
        None
    }

    pub(super) fn send_download(&mut self) -> bool {
        let now = self.air_us();
        let Some(download) = self.download.as_mut() else {
            return false;
        };
        download.run_until(now);
        let frames = download.take_frames();
        if let Some(air) = self.air.as_mut() {
            air.outbox.extend(frames);
        }
        true
    }

    pub(super) fn boot_download(&mut self) {
        let Some(image) = self
            .download
            .as_mut()
            .filter(|download| download.ready())
            .and_then(|download| download.take_image())
        else {
            return;
        };
        let rom: Arc<[u8]> = image.rom().into();
        let parent = image.parent.boot_param();
        let Ok(mut child) = Self::boot(
            &crate::rom::Rom::from(rom),
            self.firmware.clone(),
            Some(&parent),
        ) else {
            return;
        };
        let elapsed = self
            .now
            .wrapping_sub(self.air.as_ref().map_or(0, |air| air.epoch));
        child.air = self.air.take().map(|air| AirPort {
            epoch: child.now.wrapping_sub(elapsed),
            ..air
        });
        *child.buses.rtc_mut() = self.buses.rtc().clone();
        child.buses.adopt_traffic(&mut self.buses);
        child.buses.adopt_beam(&self.buses);
        child.frames = self.frames;
        child.blind = self.blind;
        child.buses.set_skip_draw(self.blind);
        child.buses.set_sound_muted(self.buses.sound_muted());
        child.audio.set_sample_rate(self.audio.sample_rate());
        child.set_keys(self.keys());
        #[cfg(jit)]
        {
            child.jit = self.jit.as_deref().map(|jit| Box::new(jit.fork()));
            child.jit_cache = self.jit_cache.clone();
        }
        *self = child;
    }
}

fn waiting_rom() -> Arc<[u8]> {
    let mut rom = vec![0; ROM_BYTES];
    rom[..TITLE.len()].copy_from_slice(TITLE);
    let words =
        |code: &[u32]| -> Vec<u8> { code.iter().flat_map(|word| word.to_le_bytes()).collect() };
    for (at, offset, ram, code) in [
        (ARM9_BINARY, ARM9_OFFSET, ARM9_RAM, words(&ARM9_HALTS)),
        (ARM7_BINARY, ARM7_OFFSET, ARM7_RAM, words(&ARM7_HALTS)),
    ] {
        for (index, value) in [offset, ram, ram, code.len() as u32]
            .into_iter()
            .enumerate()
        {
            rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
        }
        rom[offset as usize..offset as usize + code.len()].copy_from_slice(&code);
    }
    rom.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nds::download::advert::tests::{advert, beacon};
    use crate::nds::download::image::tests::with_code;
    use crate::nds::download::tests::{
        CMD_BYTES, ack, assoc_answer, auth_answer, cmd, packet, packets, payload_of, rsa,
    };
    use crate::nds::download::{BOOT_DELAY_US, Progress};
    use crate::nds::lcd::FRAME_CYCLES;
    use crate::nds::rtc::CYCLES_PER_SECOND;

    const MARK_AT: usize = 0x10_0000;
    const BOOT_INDICATOR: usize = 0x3F_FC40;
    const CHILD_ARM9: [u32; 4] = [0xE3A0_0621, 0xE3A0_105A, 0xE580_1000, 0xEAFF_FFFE];
    const CHILD_ARM7: [u32; 1] = [0xEAFF_FFFE];
    const SETTLE_US: u64 = 5_000;

    struct Air {
        nds: Nds,
        sequence: u16,
        frame_ends: Vec<u64>,
    }

    impl Air {
        fn new() -> Self {
            let mut nds = Nds::download_play(NdsFirmware::default()).unwrap();
            nds.join_the_air();
            nds.begin_air_frame();
            Self {
                nds,
                sequence: 0,
                frame_ends: Vec::new(),
            }
        }

        fn soon(&self) -> u64 {
            self.nds.air_us() + 100
        }

        fn run_us(&mut self, us: u64) -> Vec<Frame> {
            let until = self.nds.air_us() + us;
            let mut sent = Vec::new();
            while self.nds.air_us() < until {
                self.nds.step_on_air();
                sent.extend(self.nds.take_air_frames());
                if self.nds.air_frame_done() {
                    self.frame_ends.push(self.nds.air_us());
                    self.nds.end_air_frame();
                    self.nds.begin_air_frame();
                }
            }
            sent
        }

        fn hear(&mut self, frame: Frame) -> Vec<Frame> {
            let wait = frame.end() - self.nds.air_us() + SETTLE_US;
            self.nds.hear_on_air(frame);
            self.run_us(wait)
        }

        fn ask(&mut self, command: u8, data: &[u8]) -> Vec<Frame> {
            let frame = cmd(self.sequence, 0b10, command, data, self.soon());
            self.sequence += 2;
            self.hear(frame)
        }
    }

    fn receive(air: &mut Air, payload: &[u8]) {
        let advert = advert("Mario Kart DS", "Host");
        for fragment in 0..=9 {
            let start = air.soon();
            assert!(
                air.hear(beacon(&advert, fragment, 0, CMD_BYTES, start))
                    .is_empty()
            );
        }
        assert!(matches!(
            air.nds.download().unwrap().progress(),
            Progress::Offered(_)
        ));
        air.nds.set_keys(Keys::A);
        air.run_us(20_000);
        air.nds.set_keys(Keys::empty());
        let mac = air.nds.download().unwrap().mac();
        let start = air.soon();
        let auth = air.hear(beacon(&advert, 0, 0, CMD_BYTES, start));
        assert_eq!(auth.len(), 1);
        let start = air.soon();
        assert_eq!(air.hear(auth_answer(mac, start)).len(), 1);
        let start = air.soon();
        assert!(air.hear(assoc_answer(mac, 1, start)).is_empty());
        let rsa = [rsa(payload).as_slice(), &[0; 5]].concat();
        assert_eq!(air.ask(3, &rsa).len(), 1);
        for number in 0..packets(payload) {
            assert_eq!(air.ask(4, &packet(payload, number)).len(), 1);
        }
        assert_eq!(air.ask(5, &[0; 5]).len(), 1);
        assert!(air.nds.download().is_some());
        let start = air.soon();
        assert_eq!(air.hear(ack(start)).len(), 1);
    }

    #[test]
    fn a_waiting_console_boots_the_child_it_receives() {
        let child = with_code(&CHILD_ARM9, &CHILD_ARM7);
        let mut air = Air::new();
        receive(&mut air, &payload_of(&child));
        let before = air.nds.air_us();
        air.run_us(BOOT_DELAY_US - 40_000);
        assert!(air.nds.download().is_some());
        air.run_us(80_000);
        assert!(air.nds.download().is_none());
        assert!(air.nds.on_the_air() && air.nds.air_us() > before);
        let ram = air.nds.buses().main_ram();
        assert_eq!(ram[BOOT_INDICATOR], 2);
        assert_eq!(
            &ram[BOOT_INDICATOR + 2..BOOT_INDICATOR + 0x40],
            child.parent.boot_param().as_slice()
        );
        assert_eq!(ram[MARK_AT], 0x5A);
    }

    #[test]
    fn the_child_keeps_the_waiting_consoles_frame_timing() {
        let child = with_code(&CHILD_ARM9, &CHILD_ARM7);
        let mut air = Air::new();
        receive(&mut air, &payload_of(&child));
        air.run_us(BOOT_DELAY_US + 100_000);
        assert!(air.nds.download().is_none());
        let frame_us = u64::from(FRAME_CYCLES) * 1_000_000 / u64::from(CYCLES_PER_SECOND);
        for pair in air.frame_ends.windows(2) {
            let gap = pair[1] - pair[0];
            assert!(
                gap.abs_diff(frame_us) < 100,
                "a frame of {gap} us ends at {} us",
                pair[1]
            );
        }
    }

    #[test]
    fn a_waiting_console_shows_no_offer_until_a_host_calls() {
        let mut air = Air::new();
        air.run_us(50_000);
        assert!(matches!(
            air.nds.download().unwrap().progress(),
            Progress::Listening
        ));
        assert!(air.nds.take_air_frames().is_empty());
    }
}
