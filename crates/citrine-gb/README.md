[![Crates.io](https://img.shields.io/crates/v/citrine-gb)](https://crates.io/crates/citrine-gb)

# citrine-gb

**WORK IN PROGRESS**\
The emulator cores of [Citrine](https://gb.lemon.industries): Game Boy and Game Boy Color, plus Game Boy Advance and Nintendo DS behind features.

## Usage

```toml
[dependencies]
citrine-gb = { version = "0.12", features = ["gba"] }
```

```rust
use citrine_gb::console::{detect, Console, Keys};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new("game.gba");
    let rom = Rom::from_file(path)?;
    let kind = detect(&rom.data, path.file_name().and_then(|name| name.to_str()))
        .ok_or("not a game")?;
    let mut machine = Machine::load(&rom, kind, &Firmware::default())?;
    let video = machine.video();
    let mut audio = Vec::new();
    loop {
        machine.set_keys(Keys::START);
        machine.run_frame();
        let pixels: &[u8] = machine.frame();
        assert_eq!(pixels.len(), video.width * video.height * 4);
        machine.take_audio(&mut audio);
        audio.clear();
    }
}
```

`frame()` is RGBA, 4 bytes a pixel, `video().width` by `video().height`; a DS frame holds both screens, and `video().screens` says where each one sits. `take_audio` appends interleaved stereo samples at `sample_rate()`. With no BIOS in `Firmware`, the GBA and DS cores boot the free BIOS replacements in `assets/`.

## Features

With no features the crate runs Game Boy and Game Boy Color games.

| Feature | What it adds |
| --- | --- |
| `gba` | The Game Boy Advance core (`citrine_gb::gba`, `Machine::Gba`). |
| `nds` | The Nintendo DS core (`citrine_gb::nds`, `Machine::Nds`); turns on `gba`. |
| `jit` | Compiles GBA and DS ARM code to native code with Cranelift, on x86-64 and aarch64 Linux and macOS and on x86-64 Windows; other targets keep the interpreter. Switch it with `set_jit(true)` on the `Gba` or `Nds` machine. Turns on `nds`. |
| `serde` | `Serialize` and `Deserialize` on the public data types, such as `Keys` and `ConsoleKind`. |
| `persistence` | Save states, battery saves in the common file formats and netplay (`citrine_gb::persistence`, `citrine_gb::netplay`); turns on `serde`. |
| `rom-id` | CRC32, SHA-1 and SHA-256 of a game (`citrine_gb::rom::id::RomId`, `Console::rom_id`). |

# Sources

- [Pan Docs by the EmuDev Community](https://gbdev.io/pandocs/)
- [Game Boy Emulator Development Guide by Ashiepaws](https://ashiepaws.github.io/GBEDG/)
- [Game Boy Official Programming Manual v1.1 by Nintendo](https://archive.org/details/GameBoyProgManVer1.1)
- [Game Boy: Complete Technical Reference by gekkio](https://gekkio.fi/files/gb-docs/gbctr.pdf)
- [Game Boy CPU Instructions by meganesu](https://meganesu.github.io/generate-gb-opcodes/)
- [Some CPU internals by SonoSoos](https://gist.github.com/SonoSooS/c0055300670d678b5ae8433e20bea595)
- [The Pixel FIFO by jsgroth](https://jsgroth.dev/blog/posts/gb-rewrite-pixel-fifo/)

## Testing

- [adtennant's CPU unit test cases](https://github.com/adtennant/GameboyCPUTests)
- [Blargg's Gameboy hardware test roms](https://github.com/retrio/gb-test-roms)
- [Matt Currie's Mealybug Tearoom test roms](https://github.com/mattcurrie/mealybug-tearoom-tests)
- [Matt Currie's dmg-acid2 test rom](https://github.com/mattcurrie/dmg-acid2)
- [Gekkio's Mooneye Test Suite](https://github.com/Gekkio/mooneye-test-suite)