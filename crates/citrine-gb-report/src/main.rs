#[cfg(feature = "gba")]
use citrine_gb::gba::bios::Bios;
use citrine_gb::machine::Firmware;
use citrine_gb::rom::Rom;
use citrine_gb_report::{Report, replay};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage:
  citrine-gb-report show <report.zip>
  citrine-gb-report replay <report.zip> --rom <game> [--gb-boot-rom F] [--gba-bios F]
                          [--nds-bios9 F] [--nds-bios7 F] [--nds-firmware F] [--png OUT]";

const LOG_TAIL: usize = 60;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("show") if args.len() == 2 => show(&args[1]),
        Some("replay") if args.len() >= 2 => Options::parse(&args[2..])
            .ok_or_else(|| USAGE.to_string())
            .and_then(|options| replay_report(&args[1], options)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn load(path: &str) -> Result<Report, String> {
    let bytes = std::fs::read(path).map_err(|err| format!("{path}: {err}"))?;
    Report::from_zip(&bytes).map_err(|err| format!("{path}: {err}"))
}

fn show(path: &str) -> Result<ExitCode, String> {
    let report = load(path)?;
    let manifest = serde_json::to_string_pretty(&report.manifest).map_err(|err| err.to_string())?;
    println!("{manifest}");
    let pressed = report
        .inputs
        .iter()
        .filter(|input| !input.keys.is_empty() || input.touch.is_some())
        .count();
    let resets = report.inputs.iter().filter(|input| input.reset).count();
    println!(
        "\ninputs: {} frames, {pressed} with a key or touch held, {resets} resets",
        report.inputs.len()
    );
    println!(
        "save: {}",
        report
            .save
            .as_ref()
            .map_or("none".to_string(), |save| format!("{} bytes", save.len()))
    );
    println!("\n== traffic\n{}", report.traffic);
    let lines: Vec<&str> = report.log.lines().collect();
    let tail = &lines[lines.len().saturating_sub(LOG_TAIL)..];
    println!("== log, last {} of {} lines", tail.len(), lines.len());
    for line in tail {
        println!("{line}");
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Default)]
struct Options {
    rom: Option<PathBuf>,
    gb_boot_rom: Option<PathBuf>,
    gba_bios: Option<PathBuf>,
    nds_bios9: Option<PathBuf>,
    nds_bios7: Option<PathBuf>,
    nds_firmware: Option<PathBuf>,
    png: Option<PathBuf>,
}

impl Options {
    fn parse(args: &[String]) -> Option<Self> {
        let mut options = Self::default();
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            let value = Some(PathBuf::from(args.next()?));
            match flag.as_str() {
                "--rom" => options.rom = value,
                "--gb-boot-rom" => options.gb_boot_rom = value,
                "--gba-bios" => options.gba_bios = value,
                "--nds-bios9" => options.nds_bios9 = value,
                "--nds-bios7" => options.nds_bios7 = value,
                "--nds-firmware" => options.nds_firmware = value,
                "--png" => options.png = value,
                _ => return None,
            }
        }
        options.rom.is_some().then_some(options)
    }

    #[cfg_attr(not(feature = "nds"), expect(unused_variables))]
    fn firmware(&self, report: &Report) -> Result<Firmware, String> {
        #[cfg_attr(not(feature = "gba"), expect(unused_mut))]
        let mut firmware = Firmware {
            gb_boot_rom: read_optional(&self.gb_boot_rom)?,
            ..Firmware::default()
        };
        #[cfg(feature = "gba")]
        if let Some(image) = read_optional(&self.gba_bios)? {
            firmware.gba_bios = Some(Bios::user(&image).map_err(|err| format!("{err:?}"))?);
        }
        #[cfg(feature = "nds")]
        {
            firmware.nds.arm9_bios = read_optional(&self.nds_bios9)?;
            firmware.nds.arm7_bios = read_optional(&self.nds_bios7)?;
            firmware.nds.firmware = read_optional(&self.nds_firmware)?;
            firmware.nds.user_settings = report.manifest.firmware.nds_user.clone();
        }
        Ok(firmware)
    }
}

fn read_optional(path: &Option<PathBuf>) -> Result<Option<Vec<u8>>, String> {
    path.as_ref()
        .map(|path| std::fs::read(path).map_err(|err| format!("{}: {err}", path.display())))
        .transpose()
}

fn replay_report(path: &str, options: Options) -> Result<ExitCode, String> {
    let report = load(path)?;
    let rom_path = options.rom.as_ref().ok_or(USAGE)?;
    let rom = std::fs::read(rom_path).map_err(|err| format!("{}: {err}", rom_path.display()))?;
    let firmware = options.firmware(&report)?;
    let replayed =
        replay(&report, &Rom::new(&rom), &firmware).map_err(|err| format!("{path}: {err}"))?;
    let expected = report
        .manifest
        .replay
        .as_ref()
        .map_or("", |info| info.end_digest.as_str());
    println!(
        "replayed {} frames, end digest {:016x}, report says {expected}: {}",
        replayed.frames,
        replayed.digest,
        if replayed.matches {
            "same"
        } else {
            "different"
        }
    );
    if let Some(png) = &options.png {
        let image = citrine_gb_report::Screen::from_rgba(
            &replayed.frame,
            replayed.video.width,
            replayed.video.height,
            0,
        )
        .map_err(|err| err.to_string())?;
        std::fs::write(png, image.png).map_err(|err| format!("{}: {err}", png.display()))?;
    }
    Ok(if replayed.matches {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}
