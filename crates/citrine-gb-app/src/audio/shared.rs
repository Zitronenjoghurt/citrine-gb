use super::queue::QueueStats;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
#[derive(Default)]
struct Header {
    written: AtomicU32,
    read: AtomicU32,
    held: AtomicU32,
    paused: AtomicU32,
    quick_resume: AtomicU32,
}

struct Ring {
    header: Header,
    samples: Box<[AtomicU32]>,
}

impl Ring {
    fn capacity(&self) -> u32 {
        (self.samples.len() / 2) as u32
    }

    fn filled(&self) -> u32 {
        let written = self.header.written.load(Ordering::Acquire);
        written.wrapping_sub(self.header.read.load(Ordering::Acquire))
    }
}

pub struct RingWriter {
    ring: Arc<Ring>,
    stats: Arc<QueueStats>,
}

pub struct RingReader {
    ring: Arc<Ring>,
}

pub struct SharedRing {
    ring: Arc<Ring>,
}

impl SharedRing {
    pub fn new(capacity_frames: usize) -> Self {
        Self {
            ring: Arc::new(Ring {
                header: Header::default(),
                samples: (0..capacity_frames.next_power_of_two() * 2)
                    .map(|_| AtomicU32::new(0))
                    .collect(),
            }),
        }
    }

    #[cfg(all(threaded, target_arch = "wasm32"))]
    pub fn capacity_frames(&self) -> usize {
        self.ring.capacity() as usize
    }

    pub fn header_address(&self) -> usize {
        &self.ring.header as *const Header as usize
    }

    pub fn samples_address(&self) -> usize {
        self.ring.samples.as_ptr() as usize
    }

    pub fn split(self, stats: Arc<QueueStats>) -> (RingWriter, RingReader) {
        (
            RingWriter {
                ring: Arc::clone(&self.ring),
                stats,
            },
            RingReader { ring: self.ring },
        )
    }
}

impl RingWriter {
    pub fn write(&mut self, samples: &[f32]) {
        let ring = &*self.ring;
        let frames = (samples.len() / 2) as u32;
        let capacity = ring.capacity();
        let written = ring.header.written.load(Ordering::Relaxed);
        let take = frames.min(capacity - ring.filled());
        for (i, frame) in samples
            .as_chunks::<2>()
            .0
            .iter()
            .take(take as usize)
            .enumerate()
        {
            let slot = (written.wrapping_add(i as u32) % capacity) as usize * 2;
            ring.samples[slot].store(frame[0].to_bits(), Ordering::Relaxed);
            ring.samples[slot + 1].store(frame[1].to_bits(), Ordering::Relaxed);
        }
        ring.header
            .written
            .store(written.wrapping_add(take), Ordering::Release);
        if take < frames {
            self.stats.add_overrun((frames - take) as usize);
        }
    }

    pub fn queued_frames(&self) -> usize {
        (self.ring.filled() + self.ring.header.held.load(Ordering::Relaxed)) as usize
    }

    #[cfg(all(threaded, target_arch = "wasm32"))]
    pub fn is_playing(&self) -> bool {
        self.stats.is_playing()
    }

    pub fn set_paused(&self, paused: bool) {
        self.ring
            .header
            .paused
            .store(u32::from(paused), Ordering::Relaxed);
    }

    pub fn set_quick_resume(&self, quick: bool) {
        self.ring
            .header
            .quick_resume
            .store(u32::from(quick), Ordering::Relaxed);
    }

    pub fn stats(&self) -> &Arc<QueueStats> {
        &self.stats
    }
}

impl RingReader {
    pub fn drain(&self, out: &mut Vec<f32>) {
        let ring = &*self.ring;
        let capacity = ring.capacity();
        let read = ring.header.read.load(Ordering::Relaxed);
        let filled = ring.filled();
        for i in 0..filled {
            let slot = (read.wrapping_add(i) % capacity) as usize * 2;
            out.push(f32::from_bits(ring.samples[slot].load(Ordering::Relaxed)));
            out.push(f32::from_bits(
                ring.samples[slot + 1].load(Ordering::Relaxed),
            ));
        }
        ring.header
            .read
            .store(read.wrapping_add(filled), Ordering::Release);
    }

    pub fn set_held(&self, frames: usize) {
        self.ring
            .header
            .held
            .store(frames as u32, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.ring.header.paused.load(Ordering::Relaxed) != 0
    }

    pub fn quick_resume(&self) -> bool {
        self.ring.header.quick_resume.load(Ordering::Relaxed) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(frames: usize) -> (RingWriter, RingReader) {
        SharedRing::new(frames).split(Arc::new(QueueStats::default()))
    }

    fn drained(reader: &RingReader) -> Vec<f32> {
        let mut out = Vec::new();
        reader.drain(&mut out);
        out
    }

    #[test]
    fn frames_come_out_in_the_order_they_went_in_across_the_wrap() {
        let (mut writer, reader) = ring(4);
        writer.write(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        assert_eq!(drained(&reader), [1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        writer.write(&[4.0, -4.0, 5.0, -5.0, 6.0, -6.0]);
        assert_eq!(drained(&reader), [4.0, -4.0, 5.0, -5.0, 6.0, -6.0]);
        assert!(drained(&reader).is_empty());
    }

    #[test]
    fn a_full_ring_keeps_what_it_has_and_counts_the_rest_as_an_overrun() {
        let (mut writer, reader) = ring(2);
        writer.write(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
        assert_eq!(writer.stats().overruns(), 1);
        assert_eq!(writer.stats().overrun_frames(), 1);
        assert_eq!(drained(&reader), [1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn the_writer_counts_the_ring_and_what_the_reader_still_holds() {
        let (mut writer, reader) = ring(8);
        writer.write(&[0.5; 6]);
        assert_eq!(writer.queued_frames(), 3);
        reader.drain(&mut Vec::new());
        reader.set_held(5);
        assert_eq!(writer.queued_frames(), 5);
        writer.write(&[0.5; 2]);
        assert_eq!(writer.queued_frames(), 6);
    }

    #[test]
    fn the_ring_holds_a_power_of_two_so_the_counters_wrap_cleanly() {
        let (mut writer, reader) = ring(3);
        writer.write(&[0.0; 10]);
        assert_eq!(writer.queued_frames(), 4);
        assert_eq!(drained(&reader).len(), 8);
    }

    #[test]
    fn the_reader_sees_the_writer_pause() {
        let (writer, reader) = ring(2);
        assert!(!reader.paused());
        writer.set_paused(true);
        assert!(reader.paused());
        writer.set_paused(false);
        assert!(!reader.paused());
    }

    #[test]
    fn the_reader_sees_the_writer_ask_for_a_quick_resume() {
        let (writer, reader) = ring(2);
        assert!(!reader.quick_resume());
        writer.set_quick_resume(true);
        assert!(reader.quick_resume());
        writer.set_quick_resume(false);
        assert!(!reader.quick_resume());
    }

    #[test]
    fn the_worklet_finds_the_counters_and_samples_at_the_addresses_the_ring_gives() {
        let ring = SharedRing::new(8);
        let header = ring.header_address() as *const u32;
        let samples = ring.samples_address() as *const f32;
        let (mut writer, reader) = ring.split(Arc::new(QueueStats::default()));
        writer.write(&[0.5, -0.5]);
        reader.set_held(7);
        writer.set_paused(true);
        writer.set_quick_resume(true);
        let counter = |i| unsafe { header.add(i).read_volatile() };
        assert_eq!(
            [counter(0), counter(1), counter(2), counter(3), counter(4)],
            [1, 0, 7, 1, 1]
        );
        assert_eq!(
            unsafe { [samples.read(), samples.add(1).read()] },
            [0.5, -0.5]
        );
    }
}
