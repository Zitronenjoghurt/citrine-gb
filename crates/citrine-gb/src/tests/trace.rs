use crate::debug::Debug;
use crate::debug::step::Step;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

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
