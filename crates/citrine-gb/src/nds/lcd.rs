use super::Cpu;
use super::interrupt::Irq;

pub const LINE_CYCLES: u32 = 355 * 6;
pub const HBLANK_START: u32 = 48 + 256 * 6;
pub const LINES: u16 = 263;
pub const VISIBLE_LINES: u16 = 192;
pub const FRAME_CYCLES: u32 = LINE_CYCLES * LINES as u32;

const LAST_LINE: u16 = LINES - 1;
const VBLANK: u16 = 1 << 0;
const HBLANK: u16 = 1 << 1;
const MATCH: u16 = 1 << 2;
const VBLANK_IRQ: u16 = 1 << 3;
const HBLANK_IRQ: u16 = 1 << 4;
const MATCH_IRQ: u16 = 1 << 5;
const SETTING_HIGH: u16 = 1 << 7;
const WRITABLE: u16 = 0xFFB8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LcdEvent {
    Line(u16),
    HBlank(u16),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Lcd {
    line: u16,
    dot: u32,
    control: [u16; 2],
    #[cfg_attr(feature = "serde", serde(default))]
    next_line: Option<u16>,
}

fn side(cpu: Cpu) -> usize {
    match cpu {
        Cpu::Arm9 => 0,
        Cpu::Arm7 => 1,
    }
}

impl Lcd {
    pub fn line(&self) -> u16 {
        self.line
    }

    pub fn dot(&self) -> u32 {
        self.dot
    }

    pub fn mid_draw(&self) -> bool {
        self.line < VISIBLE_LINES && (self.line > 0 || self.dot > 0)
    }

    pub(crate) fn adopt_beam(&mut self, old: &Lcd) {
        self.line = old.line;
        self.dot = old.dot;
        self.next_line = old.next_line;
    }

    pub fn write_vcount(&mut self, value: u16, mask: u16) {
        let line = ((self.line & !mask) | (value & mask)) & 0x1FF;
        if mask != 0 && line < LINES {
            self.next_line = Some(line);
        }
    }

    pub fn in_vblank(&self) -> bool {
        (VISIBLE_LINES..LAST_LINE).contains(&self.line)
    }

    pub fn dispstat(&self, cpu: Cpu) -> u16 {
        let control = self.control[side(cpu)];
        let mut flags = 0;
        if self.in_vblank() {
            flags |= VBLANK;
        }
        if self.dot >= HBLANK_START {
            flags |= HBLANK;
        }
        if self.line == setting(control) {
            flags |= MATCH;
        }
        control | flags
    }

    pub fn write_dispstat(&mut self, cpu: Cpu, value: u16, mask: u16) {
        let mask = mask & WRITABLE;
        let control = &mut self.control[side(cpu)];
        *control = (*control & !mask) | (value & mask);
    }

    pub fn until_event(&self) -> u32 {
        if self.dot < HBLANK_START {
            HBLANK_START - self.dot
        } else {
            LINE_CYCLES - self.dot
        }
    }

    pub fn advance(&mut self, cycles: u32) -> Option<LcdEvent> {
        let until = self.until_event();
        if cycles < until {
            self.dot += cycles;
            return None;
        }
        if self.dot < HBLANK_START {
            self.dot = HBLANK_START;
            return Some(LcdEvent::HBlank(self.line));
        }
        self.dot = 0;
        self.line = match self.next_line.take() {
            Some(line) => line,
            None if self.line == LAST_LINE => 0,
            None => self.line + 1,
        };
        Some(LcdEvent::Line(self.line))
    }

    pub fn irqs(&self, event: LcdEvent, cpu: Cpu) -> Irq {
        let control = self.control[side(cpu)];
        let mut irq = Irq::empty();
        match event {
            LcdEvent::Line(line) => {
                if control & MATCH_IRQ != 0 && line == setting(control) {
                    irq |= Irq::VCOUNT;
                }
                if control & VBLANK_IRQ != 0 && line == VISIBLE_LINES {
                    irq |= Irq::VBLANK;
                }
            }
            LcdEvent::HBlank(_) => {
                if control & HBLANK_IRQ != 0 {
                    irq |= Irq::HBLANK;
                }
            }
        }
        irq
    }
}

fn setting(control: u16) -> u16 {
    control >> 8 | (control & SETTING_HIGH) << 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_to(lcd: &mut Lcd, line: u16, dot: u32) -> Vec<LcdEvent> {
        let mut events = Vec::new();
        while (lcd.line(), lcd.dot()) != (line, dot) {
            let step = if lcd.line() == line && lcd.dot() < dot {
                (dot - lcd.dot()).min(lcd.until_event())
            } else {
                lcd.until_event()
            };
            events.extend(lcd.advance(step));
        }
        events
    }

    #[test]
    fn a_frame_is_263_lines_of_2130_cycles() {
        assert_eq!(FRAME_CYCLES, 560_190);
        let mut lcd = Lcd::default();
        let mut cycles = 0;
        let mut lines = 0;
        loop {
            let step = lcd.until_event();
            cycles += step;
            if let Some(LcdEvent::Line(line)) = lcd.advance(step) {
                lines += 1;
                if line == 0 {
                    break;
                }
            }
        }
        assert_eq!((cycles, lines), (FRAME_CYCLES, 263));
    }

    #[test]
    fn hblank_starts_1584_cycles_into_each_line() {
        let mut lcd = Lcd::default();
        assert_eq!(lcd.advance(1583), None);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & HBLANK, 0);
        assert_eq!(lcd.advance(1), Some(LcdEvent::HBlank(0)));
        assert_eq!(lcd.dispstat(Cpu::Arm7) & HBLANK, HBLANK);
        assert_eq!(
            lcd.advance(LINE_CYCLES - HBLANK_START),
            Some(LcdEvent::Line(1))
        );
        assert_eq!(lcd.dispstat(Cpu::Arm9) & HBLANK, 0);
    }

    #[test]
    fn vblank_covers_lines_192_to_261() {
        let mut lcd = Lcd::default();
        run_to(&mut lcd, 191, 0);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & VBLANK, 0);
        run_to(&mut lcd, 192, 0);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & VBLANK, VBLANK);
        run_to(&mut lcd, 261, HBLANK_START);
        assert_eq!(lcd.dispstat(Cpu::Arm7) & VBLANK, VBLANK);
        run_to(&mut lcd, 262, 0);
        assert_eq!(lcd.dispstat(Cpu::Arm7) & VBLANK, 0);
    }

    #[test]
    fn each_cpu_has_its_own_enables_and_vcount_setting() {
        let mut lcd = Lcd::default();
        lcd.write_dispstat(Cpu::Arm9, 100 << 8 | VBLANK_IRQ, 0xFFFF);
        lcd.write_dispstat(Cpu::Arm7, 0x0100 | MATCH_IRQ | SETTING_HIGH, 0xFFFF);
        assert_eq!(lcd.dispstat(Cpu::Arm9), 100 << 8 | VBLANK_IRQ);
        assert_eq!(lcd.dispstat(Cpu::Arm7), 0x0100 | MATCH_IRQ | SETTING_HIGH);
        assert_eq!(lcd.irqs(LcdEvent::Line(192), Cpu::Arm9), Irq::VBLANK);
        assert_eq!(lcd.irqs(LcdEvent::Line(192), Cpu::Arm7), Irq::empty());
        assert_eq!(lcd.irqs(LcdEvent::Line(257), Cpu::Arm7), Irq::VCOUNT);
    }

    #[test]
    fn the_flag_bits_ignore_writes() {
        let mut lcd = Lcd::default();
        lcd.write_dispstat(Cpu::Arm9, 0xFFFF, 0xFFFF);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & (VBLANK | HBLANK | 1 << 6), 0);
    }

    #[test]
    fn the_vcount_flag_follows_the_line() {
        let mut lcd = Lcd::default();
        lcd.write_dispstat(Cpu::Arm9, 5 << 8, 0xFF00);
        run_to(&mut lcd, 5, 10);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & MATCH, MATCH);
        run_to(&mut lcd, 6, 0);
        assert_eq!(lcd.dispstat(Cpu::Arm9) & MATCH, 0);
    }

    fn lines_until(lcd: &mut Lcd, last: u16) -> Vec<u16> {
        let mut lines = Vec::new();
        while lines.last() != Some(&last) {
            if let Some(LcdEvent::Line(line)) = lcd.advance(lcd.until_event()) {
                lines.push(line);
            }
        }
        lines
    }

    #[test]
    fn a_vcount_write_numbers_the_next_line() {
        let mut lcd = Lcd::default();
        run_to(&mut lcd, 209, 100);
        lcd.write_vcount(206, 0xFFFF);
        assert_eq!(lcd.line(), 209);
        let lines = lines_until(&mut lcd, 192);
        assert_eq!(lines[..3], [206, 207, 208]);
        assert_eq!(lines.len(), (206..263).len() + (0..193).len());
    }

    #[test]
    fn a_vcount_write_past_the_last_line_is_dropped() {
        let mut lcd = Lcd::default();
        run_to(&mut lcd, 200, 0);
        lcd.write_vcount(263, 0xFFFF);
        assert_eq!(lines_until(&mut lcd, 201), [201]);
    }

    #[test]
    fn a_byte_write_keeps_the_high_bit_of_the_line() {
        let mut lcd = Lcd::default();
        run_to(&mut lcd, 258, 0);
        lcd.write_vcount(0x0004, 0x00FF);
        assert_eq!(lines_until(&mut lcd, 261), [260, 261]);
    }

    #[test]
    fn hblank_irqs_come_on_every_line_when_enabled() {
        let mut lcd = Lcd::default();
        lcd.write_dispstat(Cpu::Arm7, HBLANK_IRQ, 0xFFFF);
        assert_eq!(lcd.irqs(LcdEvent::HBlank(200), Cpu::Arm7), Irq::HBLANK);
        assert_eq!(lcd.irqs(LcdEvent::HBlank(200), Cpu::Arm9), Irq::empty());
    }
}
