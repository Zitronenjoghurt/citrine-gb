use crate::debug::registers::{BitField, Register, RegisterGroup};
use crate::gba::machine::Gba;
use crate::gba::wait::Width;

const IO: u32 = 0x0400_0000;

const BG_MODES: &[(u32, &str)] = &[
    (0, "0 tiled"),
    (1, "1 tiled"),
    (2, "2 tiled"),
    (3, "3 bitmap"),
    (4, "4 bitmap"),
    (5, "5 bitmap"),
];

const DISPCNT: &[BitField] = &[
    BitField::wide("BG mode", 0, 3).named(BG_MODES),
    BitField::wide("Frame", 4, 1).named(FRAMES),
    BitField::bit("OAM in HBlank", 5),
    BitField::bit("OBJ 1D", 6),
    BitField::bit("Forced blank", 7),
    BitField::bit("BG0", 8),
    BitField::bit("BG1", 9),
    BitField::bit("BG2", 10),
    BitField::bit("BG3", 11),
    BitField::bit("OBJ", 12),
    BitField::bit("WIN0", 13),
    BitField::bit("WIN1", 14),
    BitField::bit("OBJ window", 15),
];

const DISPSTAT: &[BitField] = &[
    BitField::bit("VBlank", 0),
    BitField::bit("HBlank", 1),
    BitField::bit("VCount match", 2),
    BitField::bit("VBlank IRQ", 3),
    BitField::bit("HBlank IRQ", 4),
    BitField::bit("VCount IRQ", 5),
    BitField::wide("VCount line", 8, 8),
];

const BGCNT: &[BitField] = &[
    BitField::wide("Priority", 0, 2),
    BitField::wide("Tile base", 2, 2),
    BitField::bit("Mosaic", 6),
    BitField::bit("256 colours", 7),
    BitField::wide("Map base", 8, 5),
    BitField::bit("Wrap", 13),
    BitField::wide("Size", 14, 2),
];

const FRAMES: &[(u32, &str)] = &[(0, "0"), (1, "1")];

const STEPS: &[(u32, &str)] = &[
    (0, "increment"),
    (1, "decrement"),
    (2, "fixed"),
    (3, "reload"),
];

const TIMINGS: &[(u32, &str)] = &[(0, "now"), (1, "VBlank"), (2, "HBlank"), (3, "special")];

const DMACNT_H: &[BitField] = &[
    BitField::wide("Dest", 5, 2).named(STEPS),
    BitField::wide("Source", 7, 2).named(STEPS),
    BitField::bit("Repeat", 9),
    BitField::bit("32-bit", 10),
    BitField::bit("Game Pak DRQ", 11),
    BitField::wide("Start", 12, 2).named(TIMINGS),
    BitField::bit("IRQ", 14),
    BitField::bit("On", 15),
];

const PRESCALERS: &[(u32, &str)] = &[
    (0, "1 cycle"),
    (1, "64 cycles"),
    (2, "256 cycles"),
    (3, "1024 cycles"),
];

const TMCNT_H: &[BitField] = &[
    BitField::wide("Tick", 0, 2).named(PRESCALERS),
    BitField::bit("Count-up", 2),
    BitField::bit("IRQ", 6),
    BitField::bit("On", 7),
];

const INTERRUPTS: &[BitField] = &[
    BitField::bit("VBlank", 0),
    BitField::bit("HBlank", 1),
    BitField::bit("VCount", 2),
    BitField::bit("Timer 0", 3),
    BitField::bit("Timer 1", 4),
    BitField::bit("Timer 2", 5),
    BitField::bit("Timer 3", 6),
    BitField::bit("Serial", 7),
    BitField::bit("DMA 0", 8),
    BitField::bit("DMA 1", 9),
    BitField::bit("DMA 2", 10),
    BitField::bit("DMA 3", 11),
    BitField::bit("Keypad", 12),
    BitField::bit("Game Pak", 13),
];

const IME: &[BitField] = &[BitField::bit("On", 0)];

const WAITCNT: &[BitField] = &[
    BitField::wide("SRAM wait", 0, 2),
    BitField::wide("WS0 first", 2, 2),
    BitField::bit("WS0 second", 4),
    BitField::wide("WS1 first", 5, 2),
    BitField::bit("WS1 second", 7),
    BitField::wide("WS2 first", 8, 2),
    BitField::bit("WS2 second", 10),
    BitField::bit("Prefetch", 14),
];

const KEYINPUT: &[BitField] = &[
    BitField::bit("A", 0),
    BitField::bit("B", 1),
    BitField::bit("Select", 2),
    BitField::bit("Start", 3),
    BitField::bit("Right", 4),
    BitField::bit("Left", 5),
    BitField::bit("Up", 6),
    BitField::bit("Down", 7),
    BitField::bit("R", 8),
    BitField::bit("L", 9),
];

const BGCNT_NAMES: [&str; 4] = ["BG0CNT", "BG1CNT", "BG2CNT", "BG3CNT"];

const DMA_NAMES: [[&str; 4]; 4] = [
    ["DMA0SAD", "DMA0DAD", "DMA0CNT_L", "DMA0CNT_H"],
    ["DMA1SAD", "DMA1DAD", "DMA1CNT_L", "DMA1CNT_H"],
    ["DMA2SAD", "DMA2DAD", "DMA2CNT_L", "DMA2CNT_H"],
    ["DMA3SAD", "DMA3DAD", "DMA3CNT_L", "DMA3CNT_H"],
];

const TIMER_NAMES: [[&str; 3]; 4] = [
    ["TM0CNT_L", "TM0 reload", "TM0CNT_H"],
    ["TM1CNT_L", "TM1 reload", "TM1CNT_H"],
    ["TM2CNT_L", "TM2 reload", "TM2CNT_H"],
    ["TM3CNT_L", "TM3 reload", "TM3CNT_H"],
];

fn half(name: &'static str, value: u16) -> Register {
    Register::new(name, value, 16)
}

impl Gba {
    fn io16(&self, offset: u32) -> u16 {
        self.bus.read(IO + offset, Width::Half) as u16
    }

    pub(super) fn io_registers(&self, out: &mut Vec<RegisterGroup>) {
        let mut display = vec![
            half("DISPCNT", self.io16(0x000)).with_fields(DISPCNT),
            half("DISPSTAT", self.io16(0x004)).with_fields(DISPSTAT),
            half("VCOUNT", self.io16(0x006)),
        ];
        display.extend(
            BGCNT_NAMES
                .iter()
                .enumerate()
                .map(|(bg, &name)| half(name, self.io16(0x008 + 2 * bg as u32)).with_fields(BGCNT)),
        );
        out.push(RegisterGroup {
            name: "Display",
            rows: display,
        });
        let dma = self.bus.dma();
        out.push(RegisterGroup {
            name: "DMA",
            rows: DMA_NAMES
                .iter()
                .enumerate()
                .flat_map(|(channel, &[sad, dad, count, control])| {
                    [
                        Register::new(sad, dma.source(channel), 32),
                        Register::new(dad, dma.dest(channel), 32),
                        half(count, dma.count(channel)),
                        half(control, dma.control(channel)).with_fields(DMACNT_H),
                    ]
                })
                .collect(),
        });
        let timers = self.bus.timers();
        out.push(RegisterGroup {
            name: "Timers",
            rows: TIMER_NAMES
                .iter()
                .enumerate()
                .flat_map(|(timer, &[counter, reload, control])| {
                    [
                        half(counter, timers.counter(timer)),
                        half(reload, timers.reload(timer)),
                        half(control, timers.control(timer)).with_fields(TMCNT_H),
                    ]
                })
                .collect(),
        });
        out.push(RegisterGroup {
            name: "Interrupts",
            rows: vec![
                half("IE", self.io16(0x200)).with_fields(INTERRUPTS),
                half("IF", self.io16(0x202)).with_fields(INTERRUPTS),
                half("IME", self.io16(0x208)).with_fields(IME),
            ],
        });
        out.push(RegisterGroup {
            name: "System",
            rows: vec![
                half("WAITCNT", self.io16(0x204)).with_fields(WAITCNT),
                half("KEYINPUT", self.io16(0x130)).with_fields(KEYINPUT),
            ],
        });
    }
}
