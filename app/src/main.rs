use citrine_gb::rom::Rom;
use std::path::PathBuf;

fn main() {
    println!(
        "{:#?}",
        Rom::from_file(&PathBuf::from("./roms/crystal.gbc"))
            .unwrap()
            .header
    );
    println!(
        "{:#?}",
        Rom::from_file(&PathBuf::from("./roms/blue.gb"))
            .unwrap()
            .header
    );
}
