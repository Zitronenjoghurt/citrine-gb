use super::load::{GPU_PASSES, GpuClockKind};
use citrine_gb_nds_gpu::{FIRST_PASS_STAMP, Pass};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Duration;
use web_time::Instant;

const STAMPS: u32 = FIRST_PASS_STAMP + Pass::ALL.len() as u32;
const STAMP_BYTES: u64 = STAMPS as u64 * 8;
pub(crate) const BURST_VAR: &str = "CITRINE_GPU_BURST";
const BURST_EVERY: Duration = Duration::from_secs(2);
const BURST_BUDGET: Duration = Duration::from_millis(3);
const BURST_COPIES: u32 = 8;

struct Stamps {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period: f32,
}

pub(crate) struct GpuClock {
    stamps: Option<Stamps>,
    busy: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    resolving: bool,
    ready: Arc<AtomicBool>,
    done_ns: Arc<AtomicU64>,
    timing: bool,
    bursts: bool,
    copies: u32,
    measured: u32,
    last_burst: Option<Instant>,
    span: Option<Duration>,
    passes: Vec<Pass>,
}

pub(crate) enum GpuReading {
    Span {
        time: Duration,
        done: Duration,
        passes: [Duration; GPU_PASSES],
    },
    Work(Duration),
}

impl GpuClock {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let stamps = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| Stamps {
                query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("ds 3d gpu time"),
                    ty: wgpu::QueryType::Timestamp,
                    count: STAMPS,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ds 3d gpu time resolve"),
                    size: STAMP_BYTES,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ds 3d gpu time read"),
                    size: STAMP_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                period: queue.get_timestamp_period(),
            });
        Self {
            stamps,
            busy: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicBool::new(false)),
            resolving: false,
            ready: Arc::new(AtomicBool::new(false)),
            done_ns: Arc::new(AtomicU64::new(0)),
            timing: false,
            bursts: std::env::var_os(BURST_VAR).is_some(),
            copies: 0,
            measured: 0,
            last_burst: None,
            span: None,
            passes: Vec::new(),
        }
    }

    pub(crate) fn kind(&self) -> GpuClockKind {
        if self.stamps.is_some() {
            GpuClockKind::Stamps
        } else {
            GpuClockKind::SubmitToDone
        }
    }

    pub(crate) fn start(&mut self) -> Option<wgpu::QuerySet> {
        self.timing = !self.busy.load(Relaxed);
        if !self.timing {
            return None;
        }
        self.busy.store(true, Relaxed);
        let due = self.bursts
            && self.stamps.is_some()
            && self
                .last_burst
                .is_none_or(|last| last.elapsed() >= BURST_EVERY);
        self.copies = if due { burst_copies(self.span) } else { 0 };
        self.stamps.as_ref().map(|stamps| stamps.query_set.clone())
    }

    #[cfg(test)]
    pub(crate) fn burst(&mut self, on: bool) {
        self.bursts = on;
    }

    pub(crate) fn copies(&self) -> u32 {
        if self.timing { self.copies } else { 0 }
    }

    pub(crate) fn cancel(&mut self) {
        if self.timing {
            self.timing = false;
            self.copies = 0;
            self.busy.store(false, Relaxed);
        }
    }

    pub(crate) fn end_stamp(&self, copy: u32) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let stamps = self
            .stamps
            .as_ref()
            .filter(|_| self.timing && copy == self.copies)?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &stamps.query_set,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(1),
        })
    }

    fn resolve(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let Some(stamps) = self.stamps.as_ref() else {
            return;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ds 3d gpu time resolve"),
        });
        encoder.resolve_query_set(&stamps.query_set, 0..STAMPS, &stamps.resolve, 0);
        encoder.copy_buffer_to_buffer(&stamps.resolve, 0, &stamps.read, 0, STAMP_BYTES);
        queue.submit([encoder.finish()]);
        let ready = Arc::clone(&self.ready);
        let busy = Arc::clone(&self.busy);
        stamps
            .read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| match result {
                Ok(()) => ready.store(true, Relaxed),
                Err(_) => busy.store(false, Relaxed),
            });
        self.resolving = true;
    }

    pub(crate) fn submitted(&mut self, queue: &wgpu::Queue, sent: Instant, passes: Vec<Pass>) {
        if !self.timing {
            return;
        }
        self.passes = passes;
        self.timing = false;
        self.measured = self.copies;
        if self.copies > 0 {
            self.last_burst = Some(Instant::now());
        }
        self.copies = 0;
        match &self.stamps {
            Some(_) => {
                self.done.store(false, Relaxed);
                self.resolving = false;
                let done = Arc::clone(&self.done);
                let done_ns = Arc::clone(&self.done_ns);
                queue.on_submitted_work_done(move || {
                    let spent = u64::try_from(sent.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    done_ns.store(spent, Relaxed);
                    done.store(true, Relaxed);
                });
            }
            None => {
                let done_ns = Arc::clone(&self.done_ns);
                let ready = Arc::clone(&self.ready);
                queue.on_submitted_work_done(move || {
                    let spent = u64::try_from(sent.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    done_ns.store(spent, Relaxed);
                    ready.store(true, Relaxed);
                });
            }
        }
    }

    pub(crate) fn take(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<GpuReading> {
        if !self.busy.load(Relaxed) {
            return None;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if self.stamps.is_some() && !self.resolving && self.done.load(Relaxed) {
            self.resolve(device, queue);
            let _ = device.poll(wgpu::PollType::Poll);
        }
        if !self.ready.swap(false, Relaxed) {
            return None;
        }
        let mut passes = [Duration::ZERO; GPU_PASSES];
        let time = match &self.stamps {
            Some(stamps) => {
                let words: Vec<u64> = {
                    let view = stamps.read.slice(..).get_mapped_range();
                    let (words, _) = view.as_chunks::<8>();
                    words.iter().map(|word| u64::from_le_bytes(*word)).collect()
                };
                stamps.read.unmap();
                let ticks = |from: u64, to: u64| {
                    Duration::from_nanos(
                        (to.saturating_sub(from) as f64 * f64::from(stamps.period)) as u64,
                    )
                };
                passes = split(&words, &self.passes, ticks);
                ticks(words[0], words[1])
            }
            None => Duration::from_nanos(self.done_ns.load(Relaxed)),
        };
        self.busy.store(false, Relaxed);
        if self.measured == 0 {
            self.span = Some(time);
            let done = Duration::from_nanos(self.done_ns.load(Relaxed));
            return Some(GpuReading::Span { time, done, passes });
        }
        Some(GpuReading::Work(time / (self.measured + 1)))
    }
}

fn burst_copies(span: Option<Duration>) -> u32 {
    span.filter(|span| !span.is_zero()).map_or(0, |span| {
        u32::try_from(BURST_BUDGET.as_nanos() / span.as_nanos())
            .unwrap_or(u32::MAX)
            .min(BURST_COPIES)
    })
}

fn split(
    words: &[u64],
    passes: &[Pass],
    ticks: impl Fn(u64, u64) -> Duration,
) -> [Duration; GPU_PASSES] {
    let mut split = [Duration::ZERO; GPU_PASSES];
    let ends = &words[FIRST_PASS_STAMP as usize..][..passes.len()];
    if words[0] == 0 || words[1] == 0 || ends.contains(&0) {
        return split;
    }
    let mut last = words[0];
    for (pass, &end) in passes.iter().zip(ends) {
        let end = end.clamp(last, words[1].max(last));
        split[slot(*pass)] += ticks(last, end);
        last = end;
    }
    split[GPU_PASSES - 1] = ticks(last, words[1]);
    split
}

fn slot(pass: Pass) -> usize {
    match pass {
        Pass::Reference => 0,
        Pass::Upscaled => 1,
        Pass::Centers => 2,
        Pass::Edges => 3,
        Pass::Finish => 4,
        Pass::Present => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nanos(from: u64, to: u64) -> Duration {
        Duration::from_nanos(to - from)
    }

    #[test]
    fn pass_stamps_split_the_frame_between_its_passes() {
        let words = [100, 1000, 300, 600, 700, 750, 0, 0];
        let passes = [Pass::Reference, Pass::Centers, Pass::Edges, Pass::Present];
        let split = split(&words, &passes, nanos);
        assert_eq!(
            split.map(|time| time.as_nanos()),
            [200, 0, 300, 100, 0, 50, 250]
        );
        assert_eq!(split.iter().sum::<Duration>(), nanos(100, 1000));
    }

    #[test]
    fn a_burst_adds_at_most_its_budget_of_frames() {
        let span = |micros| Some(Duration::from_micros(micros));
        assert_eq!(burst_copies(None), 0);
        assert_eq!(burst_copies(span(0)), 0);
        assert_eq!(burst_copies(span(100)), BURST_COPIES);
        assert_eq!(burst_copies(span(1_000)), 3);
        assert_eq!(burst_copies(span(1_400)), 2);
        assert_eq!(burst_copies(span(3_000)), 1);
        assert_eq!(burst_copies(span(3_100)), 0);
        assert_eq!(burst_copies(span(15_000)), 0);
    }

    #[test]
    fn pass_stamps_out_of_order_never_split_more_than_the_frame() {
        let words = [100, 1000, 600, 300, 1100, 900, 0, 0];
        let passes = [Pass::Reference, Pass::Centers, Pass::Edges, Pass::Present];
        let split = split(&words, &passes, nanos);
        assert_eq!(split.map(|time| time.as_nanos()), [500, 0, 0, 400, 0, 0, 0]);
        assert_eq!(split.iter().sum::<Duration>(), nanos(100, 1000));
    }

    #[test]
    fn a_pass_that_ends_before_the_frame_starts_splits_from_the_start() {
        let words = [100, 1000, 50, 400, 800, 0, 0, 0];
        let passes = [Pass::Reference, Pass::Upscaled, Pass::Present];
        let split = split(&words, &passes, nanos);
        assert_eq!(
            split.map(|time| time.as_nanos()),
            [0, 300, 0, 0, 0, 400, 200]
        );
        assert_eq!(split.iter().sum::<Duration>(), nanos(100, 1000));
    }

    #[test]
    fn a_missing_pass_stamp_splits_nothing() {
        let words = [100, 1000, 300, 0, 700, 750, 0, 0];
        let passes = [Pass::Reference, Pass::Centers, Pass::Edges, Pass::Present];
        assert_eq!(split(&words, &passes, nanos), [Duration::ZERO; GPU_PASSES]);
    }
}
