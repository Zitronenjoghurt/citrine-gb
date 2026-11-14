use crate::debug::code::{Address, Line};
use crate::debug::disassembly::Confidence;
use std::collections::VecDeque;
use std::fmt::Write;

pub const TRACE_CAPACITY: usize = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceEntry {
    pub pc: u16,
    pub at: Option<Address>,
    pub bytes: [u8; 3],
    pub registers: [u16; 5],
}

impl TraceEntry {
    pub fn text(&self) -> String {
        let at = match self.at {
            Some(at) => at.to_string(),
            None => format!("--:{:04X}", self.pc),
        };
        let line = Line::sm83(
            self.at.unwrap_or(Address::rom(0, 0)),
            self.bytes,
            Confidence::Fetched,
        );
        let bytes: Vec<String> = line.bytes().iter().map(|b| format!("{b:02X}")).collect();
        let [af, bc, de, hl, sp] = self.registers;
        format!(
            "{at}  {:<8}  {:<14}  AF={af:04X} BC={bc:04X} DE={de:04X} HL={hl:04X} SP={sp:04X}",
            bytes.join(" "),
            line.text(),
        )
    }
}

#[derive(Debug, Default)]
pub struct TraceLog {
    on: bool,
    entries: VecDeque<TraceEntry>,
}

impl TraceLog {
    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    pub fn clear(&mut self) {
        self.entries = VecDeque::new();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, index: usize) -> Option<&TraceEntry> {
        self.entries.get(index)
    }

    pub fn push(&mut self, entry: TraceEntry) {
        if self.entries.len() == TRACE_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn export(&self) -> String {
        let mut out = String::new();
        for entry in &self.entries {
            let _ = writeln!(out, "{}", entry.text());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::Debug;
    use crate::debug::step::Step;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    fn entry(pc: u16) -> TraceEntry {
        TraceEntry {
            pc,
            at: Some(Address::rom(0, u32::from(pc))),
            bytes: [0x00, 0x00, 0x00],
            registers: [0; 5],
        }
    }

    #[test]
    fn the_ring_keeps_the_last_30000_instructions() {
        let mut log = TraceLog::default();
        for pc in 0..=TRACE_CAPACITY as u32 + 4 {
            log.push(entry(pc as u16));
        }
        assert_eq!(log.len(), TRACE_CAPACITY);
        assert_eq!(log.entry(0).map(|e| e.pc), Some(5));
    }

    #[test]
    fn clearing_frees_the_ring() {
        let mut log = TraceLog::default();
        log.push(entry(1));
        log.clear();
        assert!(log.is_empty());
        assert_eq!(log.entries.capacity(), 0);
    }

    #[test]
    fn a_line_shows_the_address_bytes_instruction_and_registers() {
        let line = TraceEntry {
            pc: 0x0150,
            at: Some(Address::rom(0, 0x0150)),
            bytes: [0x3E, 0xF0, 0xE0],
            registers: [0x01B0, 0x0013, 0x00D8, 0x014D, 0xFFFE],
        }
        .text();
        assert_eq!(
            line,
            "00:0150  3E F0     LD A, F0        AF=01B0 BC=0013 DE=00D8 HL=014D SP=FFFE"
        );
    }

    const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tobu-trace.txt");

    fn tobu() -> GameBoy {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(TOBU)).expect("load");
        gb
    }

    #[test]
    fn the_trace_of_100_instructions_of_tobu_matches_the_fixture() {
        let mut gb = tobu();
        gb.trace().set_on(true);
        while gb.trace().len() < 100 {
            gb.step();
        }
        let trace = gb.trace().export();
        if std::env::var_os("CITRINE_BLESS").is_some() {
            std::fs::write(FIXTURE, &trace).expect("write fixture");
        }
        let fixture = std::fs::read_to_string(FIXTURE).expect("fixture");
        assert_eq!(trace, fixture);
    }

    #[test]
    fn tracing_is_off_by_default_and_records_nothing() {
        let mut gb = tobu();
        gb.run_frame();
        assert!(!gb.trace().is_on());
        assert!(gb.trace().is_empty());
    }

    #[test]
    fn a_traced_line_reads_like_the_code_list() {
        let mut gb = tobu();
        gb.trace().set_on(true);
        for _ in 0..3 {
            Debug::step(&mut gb, Step::Instruction);
        }
        let jump = gb.trace().entry(2).expect("three lines").text();
        assert!(jump.starts_with("00:0101  C3 50 01  JP 0150 "), "{jump}");
    }
}
