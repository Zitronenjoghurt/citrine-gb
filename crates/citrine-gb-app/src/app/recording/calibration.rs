use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;
use web_time::Instant;

pub const EVERY: Duration = Duration::from_secs(1);
const WORKGROUPS: u32 = 32;
const STAMP_BYTES: u64 = 16;
const SHADER: &str = "
@group(0) @binding(0) var<storage, read_write> word: atomic<u32>;

@compute @workgroup_size(64)
fn calibrate(@builtin(global_invocation_id) id: vec3<u32>) {
    var x = id.x + 1u;
    for (var i = 0u; i < 1024u; i++) {
        x ^= x << 13u;
        x ^= x >> 17u;
        x ^= x << 5u;
    }
    atomicXor(&word, x);
}
";

struct Kit {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period: f32,
}

#[derive(Default)]
pub struct Calibration {
    kit: Option<Option<Kit>>,
    sent: Option<Instant>,
    waiting: bool,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
}

impl Calibration {
    pub fn due(&self, now: Instant) -> bool {
        !self.waiting
            && self
                .sent
                .is_none_or(|sent| now.saturating_duration_since(sent) >= EVERY)
    }

    pub fn send(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, now: Instant) {
        if !self.due(now) {
            return;
        }
        let Some(kit) = self.kit.get_or_insert_with(|| Kit::new(device, queue)) else {
            return;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu calibration"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu calibration"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &kit.query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            });
            pass.set_pipeline(&kit.pipeline);
            pass.set_bind_group(0, &kit.bind_group, &[]);
            pass.dispatch_workgroups(WORKGROUPS, 1, 1);
        }
        encoder.resolve_query_set(&kit.query_set, 0..2, &kit.resolve, 0);
        encoder.copy_buffer_to_buffer(&kit.resolve, 0, &kit.read, 0, STAMP_BYTES);
        queue.submit([encoder.finish()]);
        self.ready.store(false, Relaxed);
        self.failed.store(false, Relaxed);
        let ready = Arc::clone(&self.ready);
        let failed = Arc::clone(&self.failed);
        kit.read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| match result {
                Ok(()) => ready.store(true, Relaxed),
                Err(_) => failed.store(true, Relaxed),
            });
        self.waiting = true;
        self.sent = Some(now);
    }

    pub fn take(&mut self, device: &wgpu::Device) -> Option<Duration> {
        let period = self.kit.as_ref()?.as_ref()?.period;
        self.stamps(device)
            .and_then(|[start, end]| span(start, end, period))
    }

    fn stamps(&mut self, device: &wgpu::Device) -> Option<[u64; 2]> {
        if !self.waiting {
            return None;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        if self.failed.swap(false, Relaxed) {
            self.waiting = false;
            if let Some(Some(kit)) = &self.kit {
                kit.read.unmap();
            }
            return None;
        }
        if !self.ready.swap(false, Relaxed) {
            return None;
        }
        self.waiting = false;
        let kit = self.kit.as_ref()?.as_ref()?;
        let words: Vec<u64> = {
            let view = kit.read.slice(..).get_mapped_range();
            let (words, _) = view.as_chunks::<8>();
            words.iter().map(|word| u64::from_le_bytes(*word)).collect()
        };
        kit.read.unmap();
        Some([words[0], words[1]])
    }
}

impl Kit {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu calibration"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu calibration"),
            layout: None,
            module: &module,
            entry_point: Some("calibrate"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let word = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu calibration word"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu calibration"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: word.as_entire_binding(),
            }],
        });
        Some(Self {
            pipeline,
            bind_group,
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("gpu calibration"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu calibration resolve"),
                size: STAMP_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu calibration read"),
                size: STAMP_BYTES,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            period: queue.get_timestamp_period(),
        })
    }
}

fn span(start: u64, end: u64, period: f32) -> Option<Duration> {
    (start != 0 && end > start)
        .then(|| Duration::from_nanos(((end - start) as f64 * f64::from(period)) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_needs_both_stamps_in_order() {
        assert_eq!(span(100, 600, 2.0), Some(Duration::from_nanos(1000)));
        assert_eq!(span(0, 600, 1.0), None);
        assert_eq!(span(600, 600, 1.0), None);
        assert_eq!(span(700, 600, 1.0), None);
    }

    #[test]
    fn one_dispatch_a_second() {
        let start = Instant::now();
        let mut calibration = Calibration::default();
        assert!(calibration.due(start));
        calibration.sent = Some(start);
        assert!(!calibration.due(start + Duration::from_millis(999)));
        assert!(calibration.due(start + EVERY));
        calibration.waiting = true;
        assert!(!calibration.due(start + EVERY * 3));
    }

    #[test]
    #[ignore = "needs a GPU with timestamp queries"]
    fn the_calibration_dispatch_reads_back_a_span_after_the_first() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("adapter");
        let descriptor = wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&descriptor)).expect("device");
        let mut calibration = Calibration::default();
        let period = queue.get_timestamp_period();
        let mut spans = Vec::new();
        let start = Instant::now();
        for second in 0..5 {
            calibration.send(&device, &queue, start + EVERY * second);
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("poll");
            let stamps = calibration.stamps(&device);
            let read = stamps.and_then(|[start, end]| span(start, end, period));
            eprintln!("calibration {second}: stamps {stamps:?} span {read:?}");
            spans.push(read);
        }
        eprintln!("calibration spans {spans:?}");
        assert!(spans[1..].iter().all(Option::is_some));
    }
}
