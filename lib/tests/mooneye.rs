use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use std::path::{Path, PathBuf};

const LD_B_B: u8 = 0x40;
const MAX_CYCLES: u32 = 30_000_000;
const PASS_REGS: [u8; 6] = [3, 5, 8, 13, 21, 34];
const FAIL_REGS: [u8; 6] = [0x42; 6];

const DMG_SUITE: &str =
    r"^(?:acceptance|emulator-only)/(?:.*/)?(?:[^/-]+|[^/]*-[^/-]*(?:G|dmg)[^/-]*)\.gb$";

fn build_root() -> String {
    let build = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/mooneye/build");
    if build.is_dir() {
        return build.to_string_lossy().into_owned();
    }

    let placeholder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/mooneye-unbuilt");
    std::fs::create_dir_all(&placeholder).ok();
    placeholder.to_string_lossy().into_owned()
}

fn run_rom(path: &Path, data: Vec<u8>) -> datatest_stable::Result<()> {
    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&rom)
        .map_err(|e| format!("failed to load {}: {e:?}", path.display()))?;

    while gb.cpu.ir != LD_B_B {
        gb.step();
        if gb.cycle_counter >= MAX_CYCLES {
            return Err(format!(
                "{} never reached the result marker within {MAX_CYCLES} cycles",
                path.display()
            )
            .into());
        }
    }

    let regs = [gb.cpu.b, gb.cpu.c, gb.cpu.d, gb.cpu.e, gb.cpu.h, gb.cpu.l];
    match regs {
        PASS_REGS => Ok(()),
        FAIL_REGS => {
            Err(format!("{} reported failure (B/C/D/E/H/L = 0x42)", path.display()).into())
        }
        _ => Err(format!(
            "{} stopped with unexpected registers {regs:02X?}",
            path.display()
        )
        .into()),
    }
}

datatest_stable::harness! {
    { test = run_rom, root = build_root(), pattern = DMG_SUITE },
}
