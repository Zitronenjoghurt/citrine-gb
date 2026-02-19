use citrine_gb::gb::GameBoy;
use citrine_gb::rom::Rom;
use std::path::PathBuf;

fn main() {
    let blue = Rom::from_file(&PathBuf::from("./roms/blue.gb")).unwrap();
    let crystal = Rom::from_file(&PathBuf::from("./roms/crystal.gbc")).unwrap();

    let mut gb = GameBoy::new_cgb();

    gb.load_rom(&blue).unwrap();
    assert!(!gb.cgb);

    gb.load_rom(&crystal).unwrap();
    assert!(gb.cgb);

    println!("{:#?}", blue.header().unwrap());
    println!("{:#?}", crystal.header().unwrap());
}
