//! Reporters render a [`ComparisonReport`] to some output, and [`FrameDumper`] writes PNGs of
//! diverging frames as they stream past. Implement [`Reporter`] to add a summary format.

use crate::emulator::{Frame, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::metric::Polarity;
use crate::runner::ComparisonReport;
use std::path::{Path, PathBuf};

/// Consumes a finished comparison result and emits it somewhere (stdout, a file, ...).
pub trait Reporter {
    fn emit(&self, report: &ComparisonReport) -> anyhow::Result<()>;
}

/// Prints a per-frame metric table and an aggregate summary to stdout.
pub struct ConsoleReporter {
    /// Cap on per-frame rows printed (the summary always reflects all frames). `None` = all rows.
    pub max_rows: Option<usize>,
}

impl Default for ConsoleReporter {
    fn default() -> Self {
        Self { max_rows: Some(30) }
    }
}

impl Reporter for ConsoleReporter {
    fn emit(&self, report: &ComparisonReport) -> anyhow::Result<()> {
        println!(
            "\n{} (reference) vs {} (candidate)",
            report.reference_name, report.candidate_name
        );
        println!(
            "frames: {} reference, {} candidate, {} compared",
            report.reference_frame_count, report.candidate_frame_count, report.compared_frame_count
        );
        if report.reference_frame_count != report.candidate_frame_count {
            println!("  ! frame counts differ");
        }
        match (report.first_divergent_frame, report.last_divergent_frame) {
            (Some(first), Some(last)) => println!(
                "divergent frames: {} / {} (first {first}, last {last})",
                report.divergent_frame_count, report.compared_frame_count
            ),
            _ => println!(
                "divergent frames: none (all {} identical)",
                report.compared_frame_count
            ),
        }

        // Header — annotate each column with its unit (e.g. `psnr (dB)`) where it has one.
        print!("\n{:>6}", "frame");
        for s in &report.summaries {
            let label = if s.unit.is_empty() {
                s.name.clone()
            } else {
                format!("{} ({})", s.name, s.unit)
            };
            print!("  {label:>12}");
        }
        println!();

        let shown = self.max_rows.unwrap_or(report.frames.len());
        for f in report.frames.iter().take(shown) {
            print!("{:>6}", f.index);
            for s in &f.scores {
                print!("  {s:>12.5}");
            }
            println!();
        }
        if report.frames.len() > shown {
            println!("  ... {} more frames", report.frames.len() - shown);
        }

        println!("\nsummary:");
        for s in &report.summaries {
            let arrow = match s.polarity {
                Polarity::HigherIsBetter => "↑",
                Polarity::LowerIsBetter => "↓",
            };
            let u = if s.unit.is_empty() {
                String::new()
            } else {
                format!(" {}", s.unit)
            };
            println!(
                "  {:<10} {arrow}  mean={:.5}{u}  best={:.5}{u}  worst={:.5}{u}",
                s.name, s.mean, s.best, s.worst
            );
        }
        Ok(())
    }
}

/// Writes the full report to a JSON file.
pub struct JsonReporter {
    pub path: PathBuf,
}

impl Reporter for JsonReporter {
    fn emit(&self, report: &ComparisonReport) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(report)?;
        std::fs::write(&self.path, json)?;
        println!("wrote JSON report to {}", self.path.display());
        Ok(())
    }
}

/// Streaming PNG dumper: fed every compared frame pair as the run progresses, it writes
/// `reference`, `candidate` and a `diff` PNG for the frames worth keeping. Because it consumes
/// frames on the fly, the runner never has to retain the whole sequence in memory.
pub struct FrameDumper {
    dir: PathBuf,
    /// Cap on how many frames to dump (avoids thousands of files). `None` = all.
    max_dumps: Option<usize>,
    /// Dump every compared frame; when `false`, only frames that diverge are dumped.
    all_frames: bool,
    dumped: usize,
    /// Number of dump-worthy frames skipped after hitting `max_dumps`.
    skipped: usize,
}

impl FrameDumper {
    pub fn new(dir: PathBuf, max_dumps: Option<usize>, all_frames: bool) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            max_dumps,
            all_frames,
            dumped: 0,
            skipped: 0,
        })
    }

    /// Offer one compared frame pair to the dumper. Cheap to call for every frame; it writes only
    /// the ones it cares about and respects the dump cap.
    pub fn handle(
        &mut self,
        index: usize,
        reference: &Frame,
        candidate: &Frame,
        diverged: bool,
    ) -> anyhow::Result<()> {
        if !self.all_frames && !diverged {
            return Ok(());
        }
        if self.dumped >= self.max_dumps.unwrap_or(usize::MAX) {
            self.skipped += 1;
            return Ok(());
        }
        write_png(
            &self.dir.join(format!("frame_{index:05}_ref.png")),
            &reference.rgba,
        )?;
        write_png(
            &self.dir.join(format!("frame_{index:05}_cand.png")),
            &candidate.rgba,
        )?;
        write_png(
            &self.dir.join(format!("frame_{index:05}_diff.png")),
            &diff_rgba(&reference.rgba, &candidate.rgba),
        )?;
        self.dumped += 1;
        Ok(())
    }

    /// Print a one-line summary of what was written. Call after the run completes.
    pub fn finish(&self) {
        let kind = if self.all_frames {
            "frame"
        } else {
            "diverging frame"
        };
        println!(
            "png-diff: dumped {} {kind}(s) to {}",
            self.dumped,
            self.dir.display()
        );
        if self.skipped > 0 {
            println!(
                "png-diff: {} more {kind}(s) NOT dumped (raise --max-dumps or pass 0 for no limit)",
                self.skipped
            );
        }
    }
}

/// Amplified per-channel absolute difference, on a black background, for visual inspection.
fn diff_rgba(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; a.len()];
    for i in (0..a.len()).step_by(4) {
        for c in 0..3 {
            let d = (a[i + c] as i32 - b[i + c] as i32).unsigned_abs();
            out[i + c] = (d.saturating_mul(4)).min(255) as u8;
        }
        out[i + 3] = 0xFF;
    }
    out
}

fn write_png(path: &Path, rgba: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}
