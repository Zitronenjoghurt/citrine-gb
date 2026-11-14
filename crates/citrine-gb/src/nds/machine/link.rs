use super::Nds;
use crate::nds::ir::NoLink;
use crate::nds::rtc::CYCLES_PER_SECOND;
use crate::nds::wifi::frame::Frame;

const US_PER_SECOND: u128 = 1_000_000;
pub(crate) const QUIET_STEP_CYCLES: u32 = 2_111;
pub(crate) const BURST_STEP_CYCLES: u32 = 33;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AirPort {
    pub(crate) epoch: u64,
    pub(crate) outbox: Vec<Frame>,
    pub(crate) done: bool,
}

impl Nds {
    pub fn on_the_air(&self) -> bool {
        self.air.is_some()
    }

    pub fn join_the_air(&mut self) {
        self.air = Some(AirPort {
            epoch: self.now,
            ..AirPort::default()
        });
    }

    pub fn leave_the_air(&mut self) {
        self.buses.wifi_mut().forget_the_air();
        self.air = None;
    }

    pub fn air_us(&self) -> u64 {
        let epoch = self.air.as_ref().map_or(0, |air| air.epoch);
        let cycles = u128::from(self.now.wrapping_sub(epoch));
        (cycles * US_PER_SECOND / u128::from(CYCLES_PER_SECOND)) as u64
    }

    pub fn begin_air_frame(&mut self) {
        if let Some(air) = self.air.as_mut() {
            air.done = false;
        }
        self.steer_download();
    }

    pub fn step_on_air(&mut self) -> u64 {
        self.step_on_air_within(u64::MAX)
    }

    fn air_step_cycles(&self) -> u64 {
        if self.buses.wifi().on_air() || self.download_busy() {
            u64::from(BURST_STEP_CYCLES)
        } else {
            u64::from(QUIET_STEP_CYCLES)
        }
    }

    pub(crate) fn step_on_air_within(&mut self, room: u64) -> u64 {
        let start = self.now;
        let end = start + self.air_step_cycles().min(room);
        let mut done = false;
        while !done && self.now < end {
            self.slice_limit = (end - self.now) as u32;
            done = self.run_slice();
            self.buses.cart_mut().exchange_ir(&mut NoLink);
            self.send_on_air();
        }
        self.slice_limit = u32::MAX;
        if let Some(air) = self.air.as_mut() {
            air.done = done;
        }
        self.now - start
    }

    pub fn air_frame_done(&self) -> bool {
        self.air.as_ref().is_none_or(|air| air.done)
    }

    pub fn end_air_frame(&mut self) {
        self.end_frame();
        self.boot_download();
    }

    pub fn take_air_frames(&mut self) -> Vec<Frame> {
        self.air
            .as_mut()
            .map(|air| std::mem::take(&mut air.outbox))
            .unwrap_or_default()
    }

    pub fn hear_on_air(&mut self, frame: Frame) {
        let Some(frame) = self.hear_download(frame) else {
            return;
        };
        let now = self.air_us();
        let chip = self.buses.wifi_mut();
        if chip.powered() {
            chip.receive_at(now, frame);
        }
    }

    fn send_on_air(&mut self) {
        if self.send_download() {
            return;
        }
        let now = self.air_us();
        let at = self.buses.traffic_clock();
        let chip = self.buses.wifi_mut();
        if !chip.powered() {
            return;
        }
        chip.traffic_mut().set_clock(at);
        let frames = chip.take_frames_at(now);
        if let Some(air) = self.air.as_mut() {
            air.outbox.extend(frames);
        }
    }
}

#[cfg(feature = "persistence")]
impl crate::netplay::LinkPort for Nds {
    type Message = Frame;
    const STAMPS_AT_STEP_START: bool = true;
    const AIR_ROUND: Option<u64> = Some(QUIET_STEP_CYCLES as u64);

    fn plug_link(&mut self) {
        self.join_the_air();
    }

    fn unplug_link(&mut self) {
        self.leave_the_air();
    }

    fn link_plugged(&self) -> bool {
        self.on_the_air()
    }

    fn link_waiting(&self) -> bool {
        false
    }

    fn take_link_messages(&mut self) -> Vec<Frame> {
        self.take_air_frames()
    }

    fn receive_link_message(&mut self, frame: Frame) {
        self.hear_on_air(frame);
    }

    fn is_transfer(_frame: &Frame) -> bool {
        true
    }

    fn begin_linked_frame(&mut self) -> u64 {
        self.begin_air_frame();
        0
    }

    fn step_linked(&mut self) -> u64 {
        self.step_on_air()
    }

    fn step_linked_within(&mut self, room: u64) -> u64 {
        self.step_on_air_within(room)
    }

    fn takes_room(&self) -> bool {
        false
    }

    fn linked_frame_done(&self) -> bool {
        self.air_frame_done()
    }

    fn end_linked_frame(&mut self) {
        self.end_air_frame();
    }

    fn holds_its_frame(&self) -> bool {
        self.buses.sleeping()
    }
}
