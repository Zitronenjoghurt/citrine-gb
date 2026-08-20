use crate::emulator::{FRAME_BYTES, Frame, FrameEmulator, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::metric::{FrameMetric, Polarity};
use crate::recording::{InputEvent, Recording};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::thread;

const MAX_STEPS_PER_FRAME: u64 = 1_000_000;

const PIPELINE_DEPTH: usize = 4;

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

/// Eager replay: retains every frame in memory. Prefer [`run_streaming`] for large runs.
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct FrameScores {
    pub index: usize,
    pub scores: Vec<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricSummary {
    pub name: String,
    pub unit: String,
    pub polarity: Polarity,
    pub mean: f64,
    /// Max if higher-is-better, else min.
    pub best: f64,
    pub worst: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ComparisonReport {
    pub reference_name: String,
    pub candidate_name: String,
    pub metric_names: Vec<String>,
    pub frames: Vec<FrameScores>,
    pub summaries: Vec<MetricSummary>,
    pub first_divergent_frame: Option<usize>,
    pub last_divergent_frame: Option<usize>,
    pub divergent_frame_count: usize,
    pub reference_frame_count: usize,
    pub candidate_frame_count: usize,
    pub compared_frame_count: usize,
}

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

    fn finish(
        self,
        metrics: &[Box<dyn FrameMetric>],
        ref_name: &str,
        cand_name: &str,
    ) -> ComparisonReport {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    /// Pair by nearest emulated cycle, cancelling the phase offset LCD on/off transitions cause.
    Cycle,
    /// Pair the Nth emitted frame of each; one extra partial frame shifts every following pair.
    Emission,
}

/// Candidate look-ahead, so the tail reference frames still have one that reaches their cycle.
const ALIGN_SLACK: usize = 16;

const FRAME_CYCLES: u64 = 70224;

struct CycleFrame {
    cycle: u64,
    rgba: Vec<u8>,
}

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
        // The consumer went away (early stop / error).
        if tx
            .send(CycleFrame {
                cycle: emu.total_cycles(),
                rgba,
            })
            .is_err()
        {
            break;
        }
    }
    Ok(())
}

fn frame_distance(a: &[u8], b: &[u8]) -> u64 {
    let mut sum = 0u64;
    for i in (0..a.len()).step_by(4) {
        for c in 0..3 {
            sum += (a[i + c] as i32 - b[i + c] as i32).unsigned_abs() as u64;
        }
    }
    sum
}

/// Compare two emulators frame by frame, holding only a few frames in memory. Each runs on its own
/// thread; this thread pairs their output per `alignment` and scores it.
///
/// In [`Alignment::Cycle`], `tolerance` widens the match to a ±`tolerance`-frame window: each metric
/// keeps its best score over the window and a frame diverges only if no candidate in it is
/// byte-identical, absorbing the sub-frame sampling skew between two independent emulators.
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
            produce(
                reference,
                rom,
                boot_rom,
                recording,
                max_frames,
                ref_tx,
                ref_recycle_rx,
            )
        });
        let cand_handle = scope.spawn(move || {
            produce(
                candidate,
                rom,
                boot_rom,
                recording,
                cand_count,
                cand_tx,
                cand_recycle_rx,
            )
        });

        let empty = || Frame::new(SCREEN_WIDTH, SCREEN_HEIGHT, Vec::new());
        let mut ref_raw = empty();
        let mut cand_raw = empty();
        let mut ref_norm = empty();
        let mut cand_norm = empty();

        let polarities: Vec<Polarity> = metrics.iter().map(|m| m.polarity()).collect();
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
                    // Producer errored; let the join surface it.
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
                // Both cycle streams rise monotonically, so a sliding window suffices. The extra
                // half frame keeps the nearest candidate in range even at `tolerance == 0`.
                use std::collections::VecDeque;
                let tol_cycles = tolerance as u64 * FRAME_CYCLES + FRAME_CYCLES / 2;

                let mut window: VecDeque<CycleFrame> = VecDeque::new();
                let mut pending: Option<CycleFrame> = None;
                let mut stream_done = false;

                for i in 0..max_frames {
                    let Ok(r) = ref_rx.recv() else { break };

                    while !stream_done
                        && window
                            .back()
                            .is_none_or(|b| b.cycle <= r.cycle + tol_cycles)
                    {
                        match pending.take().or_else(|| cand_rx.recv().ok()) {
                            Some(nf) => window.push_back(nf),
                            None => stream_done = true,
                        }
                    }
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
                    // Dumps show the most visually similar candidate, not the nearest in cycle —
                    // that one is often the timing-shifted frame tolerance exists to look past.
                    let mut rep_idx = nearest;
                    let mut rep_identical = false;
                    let mut rep_dist = u64::MAX;
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
                        if identical {
                            if !rep_identical {
                                rep_idx = wi;
                                rep_identical = true;
                            }
                        } else if !rep_identical {
                            let dist = frame_distance(&rf.rgba, &cf.rgba);
                            if dist < rep_dist {
                                rep_dist = dist;
                                rep_idx = wi;
                            }
                        }
                        std::mem::swap(&mut cand_raw.rgba, &mut window[wi].rgba);
                    }

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

        // Unblocks any producer parked in `send`; without this the joins below deadlock.
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
