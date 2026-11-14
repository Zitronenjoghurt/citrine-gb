use anyhow::{Context, Result};
use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_lab::emulator::FrameEmulator;
use citrine_lab::emulators::{DsBios, MelonDsEmulator};
use clap::Parser;
use std::path::PathBuf;

const MIX_RATE: u32 = 32_728;

#[derive(Parser)]
#[command(about = "Compare the DS audio of Citrine and melonDS sample by sample")]
struct Args {
    #[arg(long)]
    rom: PathBuf,
    #[arg(long, default_value_t = 600)]
    frames: usize,
    #[arg(long, default_value_t = 60)]
    skip_frames: usize,
    #[arg(long, default_value_t = 64)]
    max_lag: usize,
    #[arg(long, default_value = "0,1,4,16,64,256")]
    tolerances: String,
    #[arg(long, default_value_t = MIX_RATE)]
    sample_rate: u32,
    #[arg(long)]
    dump: Option<PathBuf>,
}

struct Comparison {
    lag: isize,
    pairs: Vec<(i16, i16)>,
}

impl Comparison {
    fn new(reference: &[i16], candidate: &[i16], skip: usize, max_lag: usize) -> Self {
        let lag = (-(max_lag as isize)..=max_lag as isize)
            .min_by_key(|&lag| mean_difference(&pairs(reference, candidate, skip, lag)))
            .unwrap_or(0);
        Self {
            lag,
            pairs: pairs(reference, candidate, skip, lag),
        }
    }

    fn within(&self, tolerance: u32) -> f64 {
        let close = self
            .pairs
            .iter()
            .filter(|(a, b)| a.abs_diff(*b) as u32 <= tolerance)
            .count();
        100.0 * close as f64 / self.pairs.len().max(1) as f64
    }

    fn largest(&self) -> u16 {
        self.pairs
            .iter()
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0)
    }
}

fn pairs(reference: &[i16], candidate: &[i16], skip: usize, lag: isize) -> Vec<(i16, i16)> {
    (skip..reference.len())
        .filter_map(|index| {
            let other = index as isize + 2 * lag;
            let other = usize::try_from(other).ok()?;
            Some((reference[index], *candidate.get(other)?))
        })
        .collect()
}

fn mean_difference(pairs: &[(i16, i16)]) -> u64 {
    let total: u64 = pairs.iter().map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total * 1000 / pairs.len().max(1) as u64
}

fn rms(samples: &[i16]) -> f64 {
    let total: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (total / samples.len().max(1) as f64).sqrt()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let rom = std::fs::read(&args.rom).with_context(|| format!("reading {:?}", args.rom))?;
    let mut melonds = MelonDsEmulator::new();
    melonds.set_sample_rate(args.sample_rate);
    melonds.load_nds(&rom, DsBios::FREE)?;
    let mut nds = Nds::new(rom.into(), NdsFirmware::default())?;
    nds.set_sample_rate(args.sample_rate);
    let (mut reference, mut candidate, mut buffer) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..args.frames {
        melonds.step();
        melonds.drain_audio(&mut reference);
        nds.run_frame();
        buffer.clear();
        nds.take_audio(&mut buffer);
        candidate.extend(buffer.iter().map(|&s| (s * 32768.0) as i16));
    }
    if let Some(dir) = &args.dump {
        std::fs::create_dir_all(dir)?;
        let bytes = |samples: &[i16]| {
            samples
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect::<Vec<u8>>()
        };
        std::fs::write(dir.join("melonds.s16"), bytes(&reference))?;
        std::fs::write(dir.join("citrine.s16"), bytes(&candidate))?;
    }
    let skip = args.skip_frames * 2 * args.sample_rate as usize / 60;
    let comparison = Comparison::new(&reference, &candidate, skip, args.max_lag);
    println!("rom: {}", args.rom.display());
    println!(
        "samples: melonds {} citrine {} (stereo pairs {} and {})",
        reference.len(),
        candidate.len(),
        reference.len() / 2,
        candidate.len() / 2
    );
    println!(
        "rms after frame {}: melonds {:.1} citrine {:.1}",
        args.skip_frames,
        rms(reference.get(skip..).unwrap_or_default()),
        rms(candidate.get(skip..).unwrap_or_default())
    );
    println!(
        "best lag {} samples, compared {} values, largest difference {}",
        comparison.lag,
        comparison.pairs.len(),
        comparison.largest()
    );
    for tolerance in args.tolerances.split(',') {
        let tolerance: u32 = tolerance.trim().parse()?;
        println!("within {tolerance}: {:.3}%", comparison.within(tolerance));
    }
    Ok(())
}
