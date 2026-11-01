use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::rom::Rom;
use std::collections::HashSet;
use std::path::PathBuf;

fn link_roms() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/link");
    let mut roms: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("gb" | "gbc")
            )
        })
        .collect();
    roms.sort();
    roms
}

#[test]
fn every_link_test_rom_boots_and_draws() {
    let roms = link_roms();
    assert!(!roms.is_empty());
    for path in roms {
        let rom = Rom::new(&std::fs::read(&path).unwrap());
        let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
        gb.load_rom(&rom).unwrap();
        for _ in 0..300 {
            gb.run_frame();
        }
        let colours: HashSet<&[u8]> = gb.frame().as_slice().chunks(4).collect();
        assert!(colours.len() > 1, "{} drew a blank screen", path.display());
    }
}
