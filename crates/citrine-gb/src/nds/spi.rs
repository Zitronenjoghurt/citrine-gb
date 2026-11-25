pub mod firmware;
pub mod power;
pub mod touch;

use crate::traffic::{Ended, Kind, Traffic};
use firmware::Firmware;
use power::{PowerManager, PowerModel};
use touch::Touch;

const BAUD: u16 = 0x0003;
const BUSY: u16 = 1 << 7;
const DEVICE: u16 = 0x0300;
const DEVICE_SHIFT: u16 = 8;
const HOLD: u16 = 1 << 11;
const IRQ_ENABLE: u16 = 1 << 14;
const ENABLE: u16 = 1 << 15;
const WRITABLE: u16 = BAUD | DEVICE | 1 << 10 | HOLD | IRQ_ENABLE | ENABLE;
const CYCLES_PER_BYTE_AT_4MHZ: u32 = 64;
const DATA_SHIFT: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SpiDevice {
    Power,
    Firmware,
    Touch,
    Reserved,
}

impl SpiDevice {
    fn from_control(control: u16) -> Self {
        match (control & DEVICE) >> DEVICE_SHIFT {
            0 => Self::Power,
            1 => Self::Firmware,
            2 => Self::Touch,
            _ => Self::Reserved,
        }
    }
}

pub trait SpiPort {
    fn transfer(&mut self, device: SpiDevice, byte: u8) -> u8;
    fn deselect(&mut self, device: SpiDevice);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpiBus {
    control: u16,
    data: u8,
    incoming: u8,
    busy_cycles: u32,
    selected: Option<SpiDevice>,
    interrupt: bool,
}

impl SpiBus {
    pub fn read_control(&self) -> u16 {
        let busy = if self.busy() { BUSY } else { 0 };
        self.control | busy
    }

    pub fn write_control(&mut self, value: u16, port: &mut impl SpiPort) {
        self.control = value & WRITABLE;
        if let Some(device) = self.selected
            && (self.control & ENABLE == 0 || SpiDevice::from_control(self.control) != device)
        {
            port.deselect(device);
            self.selected = None;
        }
    }

    pub fn read_data(&self) -> u8 {
        self.data
    }

    pub fn write_data(&mut self, value: u8, port: &mut impl SpiPort) {
        if self.control & ENABLE == 0 || self.busy() {
            return;
        }
        let device = SpiDevice::from_control(self.control);
        self.selected = Some(device);
        self.incoming = port.transfer(device, value);
        if self.control & HOLD == 0 {
            port.deselect(device);
            self.selected = None;
        }
        self.busy_cycles = CYCLES_PER_BYTE_AT_4MHZ << (self.control & BAUD);
    }

    pub fn busy(&self) -> bool {
        self.busy_cycles > 0
    }

    pub fn until_event(&self) -> Option<u32> {
        self.busy().then_some(self.busy_cycles)
    }

    pub fn selected(&self) -> Option<SpiDevice> {
        self.selected
    }

    pub fn step(&mut self, cycles: u32) {
        if self.busy_cycles == 0 {
            return;
        }
        self.busy_cycles = self.busy_cycles.saturating_sub(cycles);
        if self.busy_cycles == 0 {
            self.data = self.incoming;
            if self.control & IRQ_ENABLE != 0 {
                self.interrupt = true;
            }
        }
    }

    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpiDevices {
    pub power: PowerManager,
    pub firmware: Firmware,
    pub touch: Touch,
}

impl SpiDevices {
    fn transfer(&mut self, device: SpiDevice, byte: u8, now: u64) -> u8 {
        match device {
            SpiDevice::Power => self.power.transfer(byte),
            SpiDevice::Firmware => self.firmware.transfer(byte),
            SpiDevice::Touch => self.touch.transfer(byte, now),
            SpiDevice::Reserved => 0,
        }
    }

    fn deselect(&mut self, device: SpiDevice) {
        match device {
            SpiDevice::Power => self.power.deselect(),
            SpiDevice::Firmware => self.firmware.deselect(),
            SpiDevice::Touch => self.touch.deselect(),
            SpiDevice::Reserved => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Spi {
    bus: SpiBus,
    devices: SpiDevices,
    #[cfg_attr(feature = "serde", serde(skip))]
    traffic: Traffic,
    #[cfg_attr(feature = "serde", serde(skip))]
    armed: Armed,
}

#[derive(Debug, Clone, Copy, Default)]
struct Armed {
    on: bool,
    since: u32,
}

impl PartialEq for Armed {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for Armed {}

impl std::hash::Hash for Armed {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl Spi {
    pub fn new(firmware: Firmware, model: PowerModel) -> Self {
        Self {
            bus: SpiBus::default(),
            devices: SpiDevices {
                power: PowerManager::new(model),
                touch: Touch::new(firmware.calibration()),
                firmware,
            },
            traffic: Traffic::default(),
            armed: Armed::default(),
        }
    }

    pub fn read(&self, offset: usize) -> u32 {
        if offset & !3 == 0 {
            u32::from(self.bus.read_control()) | u32::from(self.bus.read_data()) << DATA_SHIFT
        } else {
            0
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32, now: u64) {
        if offset & !3 != 0 {
            return;
        }
        let mut port = LoggedSpi {
            devices: &mut self.devices,
            traffic: &mut self.traffic,
            now,
            ended: Ended::ControlWrite,
        };
        if mask & 0xFFFF != 0 {
            let control = (u32::from(self.bus.read_control()) & !mask) | (value & mask);
            self.bus.write_control(control as u16, &mut port);
        }
        if mask >> DATA_SHIFT & 0xFF != 0 {
            port.ended = Ended::HoldOff;
            let was_busy = self.bus.busy();
            self.bus.write_data((value >> DATA_SHIFT) as u8, &mut port);
            self.armed.on |= !was_busy && self.bus.busy();
        }
    }

    pub fn traffic(&self) -> &Traffic {
        &self.traffic
    }

    pub fn traffic_mut(&mut self) -> &mut Traffic {
        &mut self.traffic
    }

    pub fn step(&mut self, cycles: u32) {
        self.bus.step(cycles);
    }

    pub fn until_event(&self) -> Option<u32> {
        self.bus.until_event()
    }

    pub fn armed(&self) -> bool {
        self.armed.on
    }

    pub fn armed_since(&mut self, cycles: u32) {
        self.armed.since = cycles;
    }

    pub fn step_slice(&mut self, cycles: u32) {
        if !self.armed.on {
            self.bus.step(cycles);
        }
    }

    pub fn settle(&mut self) {
        let armed = std::mem::take(&mut self.armed);
        if armed.on {
            self.bus.step(armed.since);
        }
    }

    pub fn take_interrupt(&mut self) -> bool {
        self.bus.take_interrupt()
    }

    pub fn devices(&self) -> &SpiDevices {
        &self.devices
    }

    pub fn devices_mut(&mut self) -> &mut SpiDevices {
        &mut self.devices
    }
}

struct LoggedSpi<'a> {
    devices: &'a mut SpiDevices,
    traffic: &'a mut Traffic,
    now: u64,
    ended: Ended,
}

impl SpiPort for LoggedSpi<'_> {
    fn transfer(&mut self, device: SpiDevice, byte: u8) -> u8 {
        let got = self.devices.transfer(device, byte, self.now);
        self.traffic
            .byte(Kind::Spi, device as u8, self.now, byte, got);
        got
    }

    fn deselect(&mut self, device: SpiDevice) {
        self.devices.deselect(device);
        self.traffic.close(self.ended);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log {
        sent: Vec<(SpiDevice, u8)>,
        deselected: Vec<SpiDevice>,
    }

    impl SpiPort for Log {
        fn transfer(&mut self, device: SpiDevice, byte: u8) -> u8 {
            self.sent.push((device, byte));
            !byte
        }

        fn deselect(&mut self, device: SpiDevice) {
            self.deselected.push(device);
        }
    }

    fn control(device: u16, hold: bool) -> u16 {
        ENABLE | device << DEVICE_SHIFT | if hold { HOLD } else { 0 }
    }

    fn finish(bus: &mut SpiBus) {
        bus.step(u32::MAX);
    }

    #[test]
    fn a_disabled_bus_sends_nothing() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_data(0x12, &mut log);
        assert!(log.sent.is_empty());
        assert!(!bus.busy());
    }

    #[test]
    fn device_select_picks_the_chip() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        for (bits, device) in [
            (0, SpiDevice::Power),
            (1, SpiDevice::Firmware),
            (2, SpiDevice::Touch),
            (3, SpiDevice::Reserved),
        ] {
            bus.write_control(control(bits, false), &mut log);
            bus.write_data(0x40, &mut log);
            finish(&mut bus);
            assert_eq!(log.sent.last(), Some(&(device, 0x40)));
        }
    }

    #[test]
    fn data_reads_the_reply_once_the_transfer_ends() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(1, true), &mut log);
        bus.write_data(0x0F, &mut log);
        assert_eq!(bus.read_data(), 0);
        finish(&mut bus);
        assert_eq!(bus.read_data(), 0xF0);
    }

    #[test]
    fn busy_lasts_eight_bits_at_the_baud_rate() {
        let mut log = Log::default();
        for (baud, cycles) in [(0, 64), (1, 128), (2, 256), (3, 512)] {
            let mut bus = SpiBus::default();
            bus.write_control(control(2, false) | baud, &mut log);
            bus.write_data(0, &mut log);
            assert_ne!(bus.read_control() & BUSY, 0);
            bus.step(cycles - 1);
            assert!(bus.busy(), "baud {baud}");
            bus.step(1);
            assert_eq!(bus.read_control() & BUSY, 0, "baud {baud}");
        }
    }

    #[test]
    fn writes_while_busy_are_dropped() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(1, true), &mut log);
        bus.write_data(1, &mut log);
        bus.write_data(2, &mut log);
        assert_eq!(log.sent, [(SpiDevice::Firmware, 1)]);
    }

    #[test]
    fn hold_keeps_the_chip_selected_until_a_byte_without_it() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(1, true), &mut log);
        bus.write_data(3, &mut log);
        finish(&mut bus);
        bus.write_data(0, &mut log);
        finish(&mut bus);
        assert!(log.deselected.is_empty());
        assert_eq!(bus.selected(), Some(SpiDevice::Firmware));
        bus.write_control(control(1, false), &mut log);
        bus.write_data(0, &mut log);
        assert_eq!(log.deselected, [SpiDevice::Firmware]);
        assert_eq!(bus.selected(), None);
    }

    #[test]
    fn without_hold_every_byte_deselects() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(0, false), &mut log);
        bus.write_data(0x80, &mut log);
        finish(&mut bus);
        bus.write_data(0x00, &mut log);
        assert_eq!(log.deselected, [SpiDevice::Power, SpiDevice::Power]);
    }

    #[test]
    fn disabling_the_bus_or_switching_device_deselects() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(1, true), &mut log);
        bus.write_data(3, &mut log);
        finish(&mut bus);
        bus.write_control(control(1, true), &mut log);
        assert!(log.deselected.is_empty());
        bus.write_control(control(2, true), &mut log);
        assert_eq!(log.deselected, [SpiDevice::Firmware]);
        bus.write_data(0x90, &mut log);
        finish(&mut bus);
        bus.write_control(0, &mut log);
        assert_eq!(log.deselected, [SpiDevice::Firmware, SpiDevice::Touch]);
    }

    #[test]
    fn the_interrupt_fires_at_the_end_when_enabled() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(control(2, false), &mut log);
        bus.write_data(0, &mut log);
        finish(&mut bus);
        assert!(!bus.take_interrupt());
        bus.write_control(control(2, false) | IRQ_ENABLE, &mut log);
        bus.write_data(0, &mut log);
        bus.step(10);
        assert!(!bus.take_interrupt());
        finish(&mut bus);
        assert!(bus.take_interrupt());
        assert!(!bus.take_interrupt());
    }

    #[test]
    fn control_reads_back_its_writable_bits() {
        let mut bus = SpiBus::default();
        let mut log = Log::default();
        bus.write_control(0xFFFF, &mut log);
        assert_eq!(bus.read_control(), WRITABLE);
    }

    fn spi() -> Spi {
        Spi::new(Firmware::default(), PowerModel::Ds)
    }

    fn exchange(spi: &mut Spi, device: u16, byte: u8, hold: bool) -> u8 {
        spi.write(0, u32::from(control(device, hold)), 0xFFFF, 0);
        spi.write(0, u32::from(byte) << DATA_SHIFT, 0xFF << DATA_SHIFT, 0);
        spi.step(u32::MAX);
        (spi.read(0) >> DATA_SHIFT) as u8
    }

    #[test]
    fn the_firmware_reads_through_the_bus() {
        let mut spi = spi();
        let expected = spi.devices().firmware.bytes()[0x3_FE00..0x3_FE04].to_vec();
        for byte in [0x03, 0x03, 0xFE, 0x00] {
            exchange(&mut spi, 1, byte, true);
        }
        let mut bytes: Vec<u8> = (0..3).map(|_| exchange(&mut spi, 1, 0, true)).collect();
        bytes.push(exchange(&mut spi, 1, 0, false));
        assert_eq!(bytes, expected);
    }

    #[test]
    fn the_touch_screen_reads_through_the_bus() {
        let mut spi = spi();
        spi.devices_mut().touch.set_pen(Some((64, 32)));
        exchange(&mut spi, 2, 0xD0, true);
        let high = exchange(&mut spi, 2, 0, true);
        let low = exchange(&mut spi, 2, 0, false);
        assert_eq!((u16::from(high) << 8 | u16::from(low)) >> 3, 64 << 4);
    }

    #[test]
    fn the_power_manager_writes_through_the_bus() {
        let mut spi = spi();
        exchange(&mut spi, 0, 0x00, true);
        exchange(&mut spi, 0, 0x08, false);
        assert!(spi.devices().power.upper_backlight());
        assert!(!spi.devices().power.lower_backlight());
        exchange(&mut spi, 0, 0x80, true);
        assert_eq!(exchange(&mut spi, 0, 0, false), 0x08);
    }

    #[test]
    fn a_word_write_sets_control_then_sends_the_byte() {
        let mut spi = spi();
        let word = u32::from(control(1, true) | IRQ_ENABLE) | 0x9F << DATA_SHIFT;
        spi.write(0, word, 0x00FF_FFFF, 0);
        assert_ne!(spi.read(0) & u32::from(BUSY), 0);
        spi.step(u32::MAX);
        assert!(spi.take_interrupt());
        exchange(&mut spi, 1, 0, true);
        assert_eq!(exchange(&mut spi, 1, 0, false), 0x40);
    }

    #[test]
    fn the_touch_calibration_comes_from_the_firmware() {
        let mut image = Firmware::default().bytes().to_vec();
        image.truncate(0x100);
        image.resize(firmware::FIRMWARE_BYTES, 0xFF);
        let mut spi = Spi::new(Firmware::new(&image), PowerModel::Ds);
        spi.devices_mut().touch.set_pen(Some((10, 10)));
        exchange(&mut spi, 2, 0xD0, true);
        let high = exchange(&mut spi, 2, 0, true);
        let low = exchange(&mut spi, 2, 0, false);
        assert_eq!((u16::from(high) << 8 | u16::from(low)) >> 3, 10 << 4);
    }
}
