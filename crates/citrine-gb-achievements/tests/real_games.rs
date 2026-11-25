#![cfg(not(target_arch = "wasm32"))]

use citrine_gb_achievements::{Console, hash_rom};
use citrine_gb_test_roms as test_roms;

const RUBY: [&str; 10] = [
    "27c9f37193977828f9808f3f76ff8c76",
    "3c971ea747a4bbdbd305c6f85c1c99ff",
    "3e1b2065b3837c14ffbd03fe205fdfd7",
    "53d1a2027ab49df34a689faa1fb14726",
    "61623a55ac0ff4cb97db7c9a78a477ad",
    "7088f5789736f6fcedc687b385033003",
    "9aa6ac6fd369a686373a2e1bf8bb6aa3",
    "a943a52c790b966be2ed3140f46cd27f",
    "c014449536d7d32e5b6a3b91d10c0fe9",
    "e0503182a2e699678bcf25a6897a24d6",
];

const HEARTGOLD_SOULSILVER: [&str; 14] = [
    "1051ea6f1d599812a8a382b2fa4b02b2",
    "13816087674da5a8a9323aad3c304772",
    "560f12c326c221fbbec8a395d4a62d91",
    "65fd53a63254efeaa50e7f2c40b46192",
    "73b5a0fbcffeb7719af184170e057d34",
    "7a78d767cbcfce6262434d25aacd39bf",
    "858ae67fc5047ed4a8f4b820c40a8266",
    "89ee643f6cd30e5da9eea8b23476dd73",
    "90b23a8fd1970afd923132bd485c3b92",
    "c06cdec12d9aeb9a38fe9578638e5d55",
    "dfce09d0d107abfd669156f6e867578f",
    "e2c6eda71504be7e8ca6f5c77f0d253a",
    "e89a1c0c22f57a75a326f94c74044724",
    "eaffc2eaf15f9c6705cea3036bb7a3fa",
];

fn hash_of(test: &str, file: &str, console: Console) -> Option<String> {
    let rom = test_roms::read(test, test_roms::roms_dir().join(file))?;
    let hash = hash_rom(console, &rom).expect("hash");
    println!("{file}: {hash}");
    Some(hash)
}

#[test]
fn ruby_hashes_to_one_of_its_sets_hashes() {
    let test = "ruby_hashes_to_one_of_its_sets_hashes";
    if let Some(hash) = hash_of(test, "ruby.gba", Console::GameBoyAdvance) {
        assert!(RUBY.contains(&hash.as_str()), "{hash}");
    }
}

#[test]
fn heartgold_hashes_to_one_of_its_sets_hashes() {
    let test = "heartgold_hashes_to_one_of_its_sets_hashes";
    if let Some(hash) = hash_of(test, "hg.nds", Console::NintendoDs) {
        assert!(HEARTGOLD_SOULSILVER.contains(&hash.as_str()), "{hash}");
    }
}
