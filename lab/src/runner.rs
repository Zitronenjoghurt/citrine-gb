//! Replay driver and the comparison engine.

use crate::emulator::{FRAME_BYTES, Frame, FrameEmulator, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::metric::{FrameMetric, Polarity};
use crate::recording::{InputEvent, Recording};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::thread;

/// Generous per-frame step ceiling, guarding against an emulator that never reaches vblank.
const MAX_STEPS_PER_FRAME: u64 = 1_000_000;

/// How many produced-but-not-yet-compared frames each emulator thread may run ahead. Bounds the
/// pipeline's live memory to a few frames per emulator instead of the whole run.
const PIPELINE_DEPTH: usize = 4;

/// Steps a single emulator forward one frame at a time, applying recorded input at the right cycle.
///
/// Shared by the eager [`replay`] (used in tests) and the streaming [`run_streaming`] engine so the
/// frame-timing logic lives in exactly one place.
struct FrameDriver<'a> {
    events: &'a [InputEvent],
    next_event: usize,
    steps_since_frame: u64,
}

impl<'a> FrameDriver<'a> {
    fn new(recording: &'a Recording) -> Self {
        Self {
            events: &recording.events,
            next_event: 0,
            steps_since_frame: 0,
        }
    }

    /// Drive `emu` until it completes one frame, applying any input events whose cycle has arrived.
    fn advance<E: FrameEmulator>(&mut self, emu: &mut E) -> anyhow::Result<()> {
        loop {
            let now = emu.total_cycles();
            while self.next_event < self.events.len() && self.events[self.next_event].cycle <= now {
                let event = &self.events[self.next_event];
                emu.set_button(event.button, event.pressed);
                self.next_event += 1;
            }

            if emu.step() {
                self.steps_since_frame = 0;
                return Ok(());
            }

            self.steps_since_frame += 1;
            if self.steps_since_frame > MAX_STEPS_PER_FRAME {
                anyhow::bail!(
                    "{} produced no frame within {MAX_STEPS_PER_FRAME} steps",
                    emu.name()
                );
            }
        }
    }
}

/// Drive `emu` through `recording`, collecting up to `max_frames` completed frames eagerly.
///
/// Retains every frame in memory; prefer [`run_streaming`] for large runs. Kept for tests and
/// callers that genuinely want the whole sequence.
pub fn replay<E: FrameEmulator>(
    emu: &mut E,
    rom: &[u8],
    boot_rom: Option<&[u8]>,
    recording: &Recording,
    max_frames: usize,
) -> anyhow::Result<Vec<Frame>> {
    emu.load(rom, boot_rom, recording.model)?;
    let mut driver = FrameDriver::new(recording);
    let mut frames = Vec::with_capacity(max_frames);
    while frames.len() < max_frames {
        driver.advance(emu)?;
        frames.push(emu.frame());
    }
    Ok(frames)
}

/// Per-frame metric scores, aligned positionally with [`ComparisonReport::metric_names`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameScores {
    pub index: usize,
    pub scores: Vec<f64>,
}

/// Aggregate statistics for one metric across all compared frames.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricSummary {
    pub name: String,
    /// Unit of the score for display (e.g. `"dB"`); empty for dimensionless metrics.
    pub unit: String,
    pub polarity: Polarity,
    pub mean: f64,
    /// Most-similar score observed (max if higher-is-better, else min).
    pub best: f64,
    /// Least-similar score observed.
    pub worst: f64,
}

/// The full result of comparing a reference run against a candidate run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComparisonReport {
    pub reference_name: String,
    pub candidate_name: String,
    pub metric_names: Vec<String>,
    pub frames: Vec<FrameScores>,
    pub summaries: Vec<MetricSummary>,
    /// Index of the first frame that is not byte-identical, if any.
    pub first_divergent_frame: Option<usize>,
    /// Index of the last frame that is not byte-identical, if any.
    pub last_divergent_frame: Option<usize>,
    /// Total number of compared frames that are not byte-identical.
    pub divergent_frame_count: usize,
    pub reference_frame_count: usize,
    pub candidate_frame_count: usize,
    pub compared_frame_count: usize,
}

/// Accumulates per-metric statistics incrementally so the streaming engine never has to retain a
/// score column in memory.
struct Aggregator {
    polarities: Vec<Polarity>,
    sums: Vec<f64>,
    bests: Vec<f64>,
    worsts: Vec<f64>,
    frames: Vec<FrameScores>,
    compared: usize,
    first_divergent: Option<usize>,
    last_divergent: Option<usize>,
    divergent: usize,
}

impl Aggregator {
    fn new(metrics: &[Box<dyn FrameMetric>], max_frames: usize) -> Self {
        let polarities: Vec<Polarity> = metrics.iter().map(|m| m.polarity()).collect();
        let (bests, worsts) = polarities
            .iter()
            .map(|p| match p {
                Polarity::HigherIsBetter => (f64::MIN, f64::MAX),
                Polarity::LowerIsBetter => (f64::MAX, f64::MIN),
            })
            .unzip();
        Self {
            sums: vec![0.0; metrics.len()],
            bests,
            worsts,
            polarities,
            frames: Vec::with_capacity(max_frames),
            compared: 0,
            first_divergent: None,
            last_divergent: None,
            divergent: 0,
        }
    }

    fn record(&mut self, index: usize, scores: Vec<f64>, diverged: bool) {
        for (k, &s) in scores.iter().enumerate() {
            self.sums[k] += s;
            match self.polarities[k] {
                Polarity::HigherIsBetter => {
                    self.bests[k] = self.bests[k].max(s);
                    self.worsts[k] = self.worsts[k].min(s);
                }
                Polarity::LowerIsBetter => {
                    self.bests[k] = self.bests[k].min(s);
                    self.worsts[k] = self.worsts[k].max(s);
                }
            }
        }
        self.frames.push(FrameScores { index, scores });
        self.compared += 1;
        if diverged {
            self.first_divergent.get_or_insert(index);
            self.last_divergent = Some(index);
            self.divergent += 1;
        }
    }

    fn finish(self, metrics: &[Box<dyn FrameMetric>], ref_name: &str, cand_name: &str) -> ComparisonReport {
        let n = self.compared;
        let summaries = metrics
            .iter()
            .enumerate()
            .map(|(k, m)| MetricSummary {
                name: m.name().to_string(),
                unit: m.unit().to_string(),
                polarity: self.polarities[k],
                mean: if n == 0 { 0.0 } else { self.sums[k] / n as f64 },
                best: if n == 0 { 0.0 } else { self.bests[k] },
                worst: if n == 0 { 0.0 } else { self.worsts[k] },
            })
            .collect();

        ComparisonReport {
            reference_name: ref_name.to_string(),
            candidate_name: cand_name.to_string(),
            metric_names: metrics.iter().map(|m| m.name().to_string()).collect(),
            frames: self.frames,
            summaries,
            first_divergent_frame: self.first_divergent,
            last_divergent_frame: self.last_divergent,
            divergent_frame_count: self.divergent,
            reference_frame_count: n,
            candidate_frame_count: n,
            compared_frame_count: n,
        }
    }
}

/// How the two frame sequences are paired for comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// Pair each reference frame with the candidate frame closest in emulated **cycle count**.
    ///
    /// Both emulators are fed identical input at identical cycles, so equal cycle counts mean equal
    /// emulated time. This nulls out the constant ~1-frame phase offset that LCD on/off transitions
    /// introduce during boot, so divergence reflects real rendering differences rather than which
    /// emulator happened to emit its Nth frame first.
    Cycle,
    /// Pair the reference's Nth emitted frame with the candidate's Nth emitted frame.
    ///
    /// Simpler, but sensitive to frame-emission phase: a single partial frame emitted by one side
    /// (e.g. at an LCD toggle) shifts every subsequent pair by one, inflating divergence.
    Emission,
}

/// Extra candidate frames to produce beyond `max_frames` in [`Alignment::Cycle`] mode, so the last
/// few reference frames still have a candidate frame that reaches their cycle even when the
/// candidate runs slightly behind. Comfortably larger than any observed boot phase offset.
const ALIGN_SLACK: usize = 16;

/// T-cycles per displayed frame on DMG/CGB (single speed). Used only to size the cycle-alignment
/// tolerance window, so the exact value is non-critical.
const FRAME_CYCLES: u64 = 70224;

/// A rendered frame tagged with the emulator's total cycle count at the moment it completed.
struct CycleFrame {
    cycle: u64,
    rgba: Vec<u8>,
}

/// Produce `count` successive frames from `emu` into `tx`, each tagged with its cycle count, while
/// recycling buffers returned via `recycle` so the steady state allocates nothing. Runs on its own
/// thread.
fn produce<E: FrameEmulator>(
    mut emu: E,
    rom: &[u8],
    boot_rom: Option<&[u8]>,
    recording: &Recording,
    count: usize,
    tx: SyncSender<CycleFrame>,
    recycle: Receiver<Vec<u8>>,
) -> anyhow::Result<()> {
    emu.load(rom, boot_rom, recording.model)?;
    let mut driver = FrameDriver::new(recording);
    for _ in 0..count {
        driver.advance(&mut emu)?;
        let mut rgba = recycle
            .try_recv()
            .unwrap_or_else(|_| Vec::with_capacity(FRAME_BYTES));
        emu.render_into(&mut rgba);
        // A send error means the consumer has gone away (early stop / error) — just bow out.
        if tx.send(CycleFrame { cycle: emu.total_cycles(), rgba }).is_err() {
            break;
        }
    }
    Ok(())
}

/// Compare a reference emulator against a candidate, frame by frame, without ever holding more than
/// a few frames in memory.
///
/// The two emulators run on their own threads (overlapping each other and the metric computation);
/// this thread pairs their output per `alignment`, scores each pair, and hands every compared frame
/// pair to `on_frame` (used for image dumps and progress reporting). `on_frame` receives the
/// *comparison* frames — greyscale-normalized when `normalize` is set, raw otherwise — plus whether
/// they diverge, indexed by the reference frame's emission position.
///
/// In [`Alignment::Cycle`], `tolerance` widens the match to a ±`tolerance`-frame window around the
/// nearest-cycle candidate: each metric keeps its best score over the window, and a frame counts as
/// divergent only if *no* candidate in the window is byte-identical. This absorbs the irreducible
/// sub-frame sampling skew (two independent emulators latch their framebuffers a few hundred cycles
/// apart, so a game update landing in that gap shows up one frame early/late). `tolerance` is
/// ignored in [`Alignment::Emission`].
pub fn run_streaming<R, C>(
    reference: R,
    candidate: C,
    rom: &[u8],
    boot_rom: Option<&[u8]>,
    recording: &Recording,
    metrics: &[Box<dyn FrameMetric>],
    max_frames: usize,
    normalize: bool,
    alignment: Alignment,
    tolerance: usize,
    mut on_frame: impl FnMut(usize, &Frame, &Frame, bool) -> anyhow::Result<()>,
) -> anyhow::Result<ComparisonReport>
where
    R: FrameEmulator + Send,
    C: FrameEmulator + Send,
{
    let ref_name = reference.name().to_string();
    let cand_name = candidate.name().to_string();
    let mut agg = Aggregator::new(metrics, max_frames);

    // The candidate runs a little past the reference in cycle mode so the tail stays bracketed,
    // plus the tolerance window's worth of look-ahead.
    let cand_count = match alignment {
        Alignment::Cycle => max_frames + ALIGN_SLACK + tolerance,
        Alignment::Emission => max_frames,
    };

    thread::scope(|scope| -> anyhow::Result<()> {
        let (ref_tx, ref_rx) = sync_channel::<CycleFrame>(PIPELINE_DEPTH);
        let (cand_tx, cand_rx) = sync_channel::<CycleFrame>(PIPELINE_DEPTH);
        let (ref_recycle_tx, ref_recycle_rx) = channel::<Vec<u8>>();
        let (cand_recycle_tx, cand_recycle_rx) = channel::<Vec<u8>>();

        let ref_handle = scope.spawn(move || {
            produce(reference, rom, boot_rom, recording, max_frames, ref_tx, ref_recycle_rx)
        });
        let cand_handle = scope.spawn(move || {
            produce(candidate, rom, boot_rom, recording, cand_count, cand_tx, cand_recycle_rx)
        });

        // Persistent scratch frames reused every iteration; nothing here is reallocated per frame.
        let empty = || Frame::new(SCREEN_WIDTH, SCREEN_HEIGHT, Vec::new());
        let mut ref_raw = empty();
        let mut cand_raw = empty();
        let mut ref_norm = empty();
        let mut cand_norm = empty();

        let polarities: Vec<Polarity> = metrics.iter().map(|m| m.polarity()).collect();
        // Update `best` (one score per metric) with a candidate's scores, keeping the most-similar
        // value per metric according to its polarity.
        let keep_best = |best: &mut [f64], scores: &[f64]| {
            for (k, &s) in scores.iter().enumerate() {
                if best[k].is_nan() {
                    best[k] = s;
                } else {
                    best[k] = match polarities[k] {
                        Polarity::HigherIsBetter => best[k].max(s),
                        Polarity::LowerIsBetter => best[k].min(s),
                    };
                }
            }
        };

        match alignment {
            Alignment::Emission => {
                for i in 0..max_frames {
                    // A closed channel means that producer stopped early (it errored); stop and let
                    // the join below surface the real error.
                    let (Ok(r), Ok(c)) = (ref_rx.recv(), cand_rx.recv()) else {
                        break;
                    };
                    ref_raw.rgba = r.rgba;
                    cand_raw.rgba = c.rgba;
                    let (rf, cf) = if normalize {
                        ref_raw.normalize_into(&mut ref_norm);
                        cand_raw.normalize_into(&mut cand_norm);
                        (&ref_norm, &cand_norm)
                    } else {
                        (&ref_raw, &cand_raw)
                    };
                    let scores: Vec<f64> = metrics.iter().map(|m| m.compare(rf, cf)).collect();
                    let diverged = rf.rgba != cf.rgba;
                    on_frame(i, rf, cf, diverged)?;
                    agg.record(i, scores, diverged);
                    let _ = ref_recycle_tx.send(std::mem::take(&mut ref_raw.rgba));
                    let _ = cand_recycle_tx.send(std::mem::take(&mut cand_raw.rgba));
                }
            }
            Alignment::Cycle => {
                // Nearest-cycle alignment with a ±`tolerance`-frame window. Both cycle streams rise
                // monotonically and reference cycles only grow, so we keep a sliding window of
                // candidate frames spanning roughly [r.cycle - tol, r.cycle + tol] and, for each
                // reference frame, take the best metric score over the window. A `tolerance` of 0
                // collapses to "compare against the single nearest-cycle candidate".
                use std::collections::VecDeque;
                // Window half-width in cycles: `tolerance` whole frames plus half a frame so the
                // nearest candidate (always within ±½ frame) is included even at `tolerance == 0`.
                let tol_cycles = tolerance as u64 * FRAME_CYCLES + FRAME_CYCLES / 2;

                let mut window: VecDeque<CycleFrame> = VecDeque::new();
                let mut pending: Option<CycleFrame> = None;
                let mut stream_done = false;

                for i in 0..max_frames {
                    let Ok(r) = ref_rx.recv() else { break };

                    // Extend the window until it reaches `tol_cycles` past the reference cycle.
                    while !stream_done
                        && window.back().is_none_or(|b| b.cycle <= r.cycle + tol_cycles)
                    {
                        match pending.take().or_else(|| cand_rx.recv().ok()) {
                            Some(nf) => window.push_back(nf),
                            None => stream_done = true,
                        }
                    }
                    // Retire candidates that fell more than `tol_cycles` behind (recycle them),
                    // always keeping at least one so the window is never empty.
                    while window.len() > 1 && window.front().unwrap().cycle + tol_cycles < r.cycle {
                        let _ = cand_recycle_tx.send(window.pop_front().unwrap().rgba);
                    }
                    if window.is_empty() {
                        break;
                    }

                    ref_raw.rgba = r.rgba;
                    let rf: &Frame = if normalize {
                        ref_raw.normalize_into(&mut ref_norm);
                        &ref_norm
                    } else {
                        &ref_raw
                    };

                    // Locate the nearest-cycle candidate (always exists), then score it together
                    // with the ±`tolerance` candidates on either side by emission index, keeping the
                    // best score per metric.
                    let mut nearest = 0usize;
                    let mut nearest_dist = u64::MAX;
                    for wi in 0..window.len() {
                        let dist = window[wi].cycle.abs_diff(r.cycle);
                        if dist < nearest_dist {
                            nearest_dist = dist;
                            nearest = wi;
                        }
                    }
                    let lo = nearest.saturating_sub(tolerance);
                    let hi = (nearest + tolerance).min(window.len() - 1);

                    let mut best = vec![f64::NAN; metrics.len()];
                    let mut any_identical = false;
                    // Representative candidate for image dumps: an exact match if one exists in the
                    // window, otherwise the nearest in cycle.
                    let mut rep_idx = nearest;
                    let mut rep_identical = false;
                    for wi in lo..=hi {
                        std::mem::swap(&mut cand_raw.rgba, &mut window[wi].rgba);
                        let cf: &Frame = if normalize {
                            cand_raw.normalize_into(&mut cand_norm);
                            &cand_norm
                        } else {
                            &cand_raw
                        };
                        let scores: Vec<f64> = metrics.iter().map(|m| m.compare(rf, cf)).collect();
                        keep_best(&mut best, &scores);
                        let identical = rf.rgba == cf.rgba;
                        any_identical |= identical;
                        if identical && !rep_identical {
                            rep_idx = wi;
                            rep_identical = true;
                        }
                        std::mem::swap(&mut cand_raw.rgba, &mut window[wi].rgba);
                    }

                    // Re-render the representative candidate for `on_frame` (dumps/progress).
                    std::mem::swap(&mut cand_raw.rgba, &mut window[rep_idx].rgba);
                    let cf: &Frame = if normalize {
                        cand_raw.normalize_into(&mut cand_norm);
                        &cand_norm
                    } else {
                        &cand_raw
                    };
                    on_frame(i, rf, cf, !any_identical)?;
                    std::mem::swap(&mut cand_raw.rgba, &mut window[rep_idx].rgba);

                    agg.record(i, best, !any_identical);
                    let _ = ref_recycle_tx.send(std::mem::take(&mut ref_raw.rgba));
                }
                for frame in window.drain(..) {
                    let _ = cand_recycle_tx.send(frame.rgba);
                }
                if let Some(p) = pending.take() {
                    let _ = cand_recycle_tx.send(p.rgba);
                }
            }
        }

        // Drop receivers and recycle senders so any producer still blocked on `send` (it ran ahead
        // of what we consumed) unblocks and exits — otherwise `join` would deadlock.
        drop(ref_rx);
        drop(cand_rx);
        drop(ref_recycle_tx);
        drop(cand_recycle_tx);

        ref_handle
            .join()
            .map_err(|_| anyhow::anyhow!("{ref_name} emulator thread panicked"))??;
        cand_handle
            .join()
            .map_err(|_| anyhow::anyhow!("{cand_name} emulator thread panicked"))??;
        Ok(())
    })?;

    Ok(agg.finish(metrics, &ref_name, &cand_name))
}
