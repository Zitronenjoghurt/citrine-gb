use crate::ReadMemory;
use crate::WriteMemory;
use crate::console::Rollback;
use crate::debug::tiles::Picture;
use crate::gb::bus::CpuBusInterface;
use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::{GameBoy, GbModel};
use crate::netplay::{Advance, LinkPort, Session};
use crate::rom::Rom;
use citrine_gb_threads::{Mode, Threads};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct TestBus {
    pub(crate) data: HashMap<u16, u8>,
    pub(crate) history: Vec<(u16, u8, String)>,
}

impl CpuBusInterface for TestBus {
    fn cycle(&mut self) {}

    fn read(&mut self, addr: u16) -> u8 {
        let value = self
            .data
            .get(&addr)
            .copied()
            .unwrap_or_else(|| panic!("No data for address {:04X}", addr));
        self.history.push((addr, value, "read".to_string()));
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.data.insert(addr, value);
        self.history.push((addr, value, "write".to_string()));
    }
}

impl ICInterface for TestBus {
    fn request_interrupt(&mut self, _interrupt: Interrupt) {}
}

#[cfg(feature = "debug")]
impl crate::debug::DebuggerInterface for TestBus {}

pub fn skip(test: &str, reason: &str) {
    let _ = writeln!(std::io::stderr(), "\nskipped {test}: {reason}");
}

pub(crate) fn rom(cgb_flag: u8, code: &[u8]) -> Rom {
    let mut data = vec![0u8; 0x8000];
    data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    data[0x0143] = cgb_flag;
    data[0x0150..0x0150 + code.len()].copy_from_slice(code);
    let end = 0x0150 + code.len();
    data[end..end + 2].copy_from_slice(&[0x18, 0xFE]);
    Rom::new(&data)
}

pub(crate) fn boot(model: GbModel, cgb_flag: u8, code: &[u8]) -> GameBoy {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom(&rom(cgb_flag, code)).expect("load");
    gb
}

pub(crate) fn read(gb: &mut GameBoy, addr: u16) -> u8 {
    gb.with_bus(|bus| bus.read_naive(addr))
}

pub(crate) fn write(gb: &mut GameBoy, addr: u16, value: u8) {
    gb.with_bus(|bus| bus.write_naive(addr, value))
}

pub(crate) fn cycles(gb: &mut GameBoy, count: u32) {
    gb.with_bus(|bus| {
        for _ in 0..count {
            bus.cycle();
        }
    })
}

#[path = "../tests/archives/mod.rs"]
mod archives;
pub mod state_fuzz;

#[derive(Deserialize)]
pub(crate) struct Sources {
    sources: BTreeMap<String, archives::Source>,
}

pub(crate) struct LedgerRoms {
    archives: Vec<(zip::ZipArchive<File>, HashMap<String, String>)>,
}

impl LedgerRoms {
    pub(super) fn load() -> Result<Self, String> {
        let manifest: Sources = archives::read_manifest()?;
        let mut archives = Vec::new();
        for (name, source) in &manifest.sources {
            let mut archive = archives::fetch(name, source)?;
            let index = rom_index(&source.sha256, &mut archive)?;
            archives.push((archive, index));
        }
        Ok(Self { archives })
    }

    pub(super) fn find(&mut self, sha: &str) -> Result<Option<Vec<u8>>, String> {
        for (archive, index) in &mut self.archives {
            if let Some(path) = index.get(sha) {
                return archives::read_entry(archive, path).map(Some);
            }
        }
        Ok(None)
    }
}

pub(crate) fn parse_index(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(sha, path)| (sha.to_uppercase(), path.to_string()))
        .collect()
}

pub(crate) fn rom_index(
    archive_sha: &str,
    archive: &mut zip::ZipArchive<File>,
) -> Result<HashMap<String, String>, String> {
    let dir = archives::cache_dir();
    let path = dir.join(format!("{archive_sha}.roms"));
    if let Ok(text) = fs::read_to_string(&path) {
        return Ok(parse_index(&text));
    }

    let mut text = String::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if !(name.ends_with(".gb") || name.ends_with(".gbc")) {
            continue;
        }
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|e| format!("{name} in archive: {e}"))?;
        text.push_str(&format!("{} {name}\n", archives::sha256_hex(&data)));
    }

    let partial = dir.join(format!("{archive_sha}.{}.roms.part", std::process::id()));
    fs::write(&partial, &text).map_err(|e| format!("{}: {e}", partial.display()))?;
    fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(parse_index(&text))
}

pub(crate) const COUNT: u16 = 300;

pub(crate) const NOW: u64 = 1_790_000_000;

pub(crate) fn trade_program(sc: u8, first: u8, step: u8) -> Vec<u8> {
    let [low, high] = COUNT.to_le_bytes();
    let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00, 0xAF, 0xEA, 0x00, 0xDF];
    code.extend_from_slice(&[0x3E, 0x10, 0xE0, 0x00, 0xF0, 0x00, 0xCB, 0x47, 0x20, 0xFA]);
    code.extend_from_slice(&[0x21, 0x00, 0xA0, 0x11, low, high, 0x06, first]);
    let top = code.len();
    code.extend_from_slice(&[0x78, 0xE0, 0x01]);
    if sc == 0x81 {
        code.extend_from_slice(&[0x0E, 0x28, 0x0D, 0x20, 0xFD]);
    }
    code.extend_from_slice(&[0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
    code.extend_from_slice(&[0xF0, 0x01, 0x22, 0x78, 0xC6, step, 0x47]);
    code.extend_from_slice(&[0x1B, 0x7A, 0xB3, 0x20]);
    let back = top as i32 - (code.len() as i32 + 1);
    code.push(back as i8 as u8);
    code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0x00, 0xDF]);
    code
}

pub(crate) fn cart(cgb_flag: u8, kind: u8, code: &[u8]) -> Rom {
    let mut data = rom(cgb_flag, code).data.to_vec();
    data[0x0147] = kind;
    data[0x0149] = 0x02;
    Rom::new(&data)
}

pub(crate) fn red() -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&cart(0x00, 0x03, &trade_program(0x81, 0x10, 1)))
        .unwrap();
    gb
}

pub(crate) fn sent(first: u8, step: u8) -> Vec<u8> {
    (0..COUNT)
        .map(|i| first.wrapping_add((i as u8).wrapping_mul(step)))
        .collect()
}

pub(crate) fn done(gb: &GameBoy) -> bool {
    gb.memory.read_naive(0xDF00) == 0xAA
}

pub(crate) fn saved(gb: &mut GameBoy) -> Vec<u8> {
    let dump = gb.poll_sram_dump(true, NOW).expect("a battery save");
    dump.as_slice()[..COUNT as usize].to_vec()
}

thread_local! {
    static THREADS: Threads = Threads::new(Mode::from_env());
}

pub(crate) trait Step {
    fn step(&mut self) -> Advance;
}

impl<C: Rollback + LinkPort, const N: usize> Step for Session<C, N> {
    fn step(&mut self) -> Advance {
        THREADS.with(|threads| threads.split(|split| self.advance_on(split)))
    }
}

pub(crate) fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(format!("{name}.png"))
}

pub(crate) fn read_png(path: &PathBuf) -> Picture {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .expect("png header");
    let mut rgba = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut rgba).expect("png frame");
    rgba.truncate(info.buffer_size());
    Picture {
        width: info.width as usize,
        height: info.height as usize,
        rgba,
    }
}

pub(crate) fn matches_fixture(name: &str, picture: &Picture) {
    let path = fixture(name);
    if std::env::var_os("CITRINE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("fixture dir")).expect("fixture dir");
        std::fs::write(&path, picture.png()).expect("write fixture");
    }
    let committed = read_png(&path);
    assert_eq!(
        (committed.width, committed.height),
        (picture.width, picture.height),
        "{name}"
    );
    assert!(
        committed.rgba == picture.rgba,
        "{name} differs from {path:?}"
    );
}
