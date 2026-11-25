use super::gpu_clock::{GpuClock, GpuReading};
use super::load::GpuTotals;
use crate::app::ui_state::ds_3d::{DistantTextures, Ds3d, SmoothEdges, TextureFilter};
use crate::media::recording::{ScreenRecorder, Sharp, Ticket};
use citrine_gb::nds::machine::Nds;
use citrine_gb::nds::video::{Composite, HEIGHT, Mix, WIDTH};
use citrine_gb_nds_gpu::{Antialiasing, Mipmaps, Renderer, Route, Textures, Upscale};
use eframe::egui_wgpu::RenderState;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering::Relaxed};
use std::sync::{Arc, OnceLock};
use web_time::Instant;

const LAYER_BYTES: u32 = 16;
const READS: usize = 3;
const MAPPING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Upscaled {
    pub(crate) texture: egui::TextureId,
    pub(crate) on_top: bool,
}

impl Upscaled {
    pub(crate) fn screen_y(self) -> usize {
        if self.on_top { 0 } else { HEIGHT }
    }
}

pub(crate) struct Upscaler {
    state: RenderState,
    renderer: Renderer,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    layers: wgpu::Texture,
    packed: Vec<u8>,
    target: Option<Target>,
    clock: Option<GpuClock>,
    burst: bool,
    reads: VecDeque<Readback>,
    spare: Vec<wgpu::Buffer>,
}

struct Readback {
    buffer: wgpu::Buffer,
    scale: u32,
    state: Arc<AtomicU8>,
    ticket: Ticket,
    frame: Vec<u8>,
    screen_y: usize,
}

struct Target {
    scale: u32,
    view: wgpu::TextureView,
    id: egui::TextureId,
}

impl Upscaler {
    pub(crate) fn new(state: RenderState) -> Option<Self> {
        static ROUTE: OnceLock<Result<Route, String>> = OnceLock::new();
        let Ok(route) = ROUTE.get_or_init(|| {
            first_route(|route| citrine_gb_nds_gpu::probe(&state.adapter, &state.device, route))
        }) else {
            return None;
        };
        Self::on_route(state, *route)
    }

    fn on_route(state: RenderState, route: Route) -> Option<Self> {
        let device = state.device.clone();
        citrine_gb_nds_gpu::set_up(&device, || Self::build(state, route))
            .inspect_err(|error| {
                log::warn!("DS 3D draws on the CPU, its GPU set-up failed: {error}")
            })
            .ok()
    }

    fn build(state: RenderState, route: Route) -> Self {
        let device = &state.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("upscale.wgsl"));
        let entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ds 3d composite"),
            entries: &[
                entry(0, wgpu::TextureSampleType::Float { filterable: false }),
                entry(1, wgpu::TextureSampleType::Uint),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ds 3d composite"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ds 3d composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(TARGET_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let layers = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ds 3d layers"),
            size: extent(1),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let renderer = Renderer::with_route(device, route);
        let clock = GpuClock::new(&state.device, &state.queue);
        Self {
            state,
            renderer,
            pipeline,
            layout,
            layers,
            packed: vec![0; WIDTH * HEIGHT * LAYER_BYTES as usize],
            target: None,
            clock: Some(clock),
            burst: false,
            reads: VecDeque::new(),
            spare: Vec::new(),
        }
    }

    pub(crate) fn timed(&mut self, totals: &mut GpuTotals) {
        let Some(clock) = self.clock.as_mut() else {
            return;
        };
        totals.kind = Some(clock.kind());
        match clock.take(&self.state.device, &self.state.queue) {
            Some(GpuReading::Span { time, done, passes }) => {
                totals.time += time;
                totals.done += done;
                for (total, pass) in totals.passes.iter_mut().zip(passes) {
                    *total += pass;
                }
                totals.frames += 1;
            }
            Some(GpuReading::Work(work)) => totals.work = Some(work),
            None => {}
        }
    }

    pub(crate) fn draw(&mut self, nds: &Nds, choice: Ds3d) -> Option<Upscaled> {
        let composite = nds.composite().filter(|composite| composite.upscalable())?;
        let scene = nds.three_d_scene();
        let scale = choice.resolution.scale();
        let upscale = Upscale {
            scale,
            antialiasing: antialiasing(choice.smooth_edges),
            textures: textures(choice.texture_filter),
            mipmaps: mipmaps(choice.distant_textures),
        };
        let device = &self.state.device;
        let queue = &self.state.queue;
        self.renderer
            .stamp_passes(self.clock.as_mut().and_then(GpuClock::start));
        let commands = self.renderer.encode(
            device,
            queue,
            scene.frame,
            &scene.registers,
            scene.textures,
            upscale,
        );
        pack(composite, &mut self.packed);
        queue.write_texture(
            self.layers.as_image_copy(),
            &self.packed,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH as u32 * LAYER_BYTES),
                rows_per_image: Some(HEIGHT as u32),
            },
            extent(1),
        );
        let Some(three_d) = self.renderer.view() else {
            queue.submit([commands]);
            if let Some(clock) = self.clock.as_mut() {
                clock.cancel();
            }
            return None;
        };
        let layers = self
            .layers
            .create_view(&wgpu::TextureViewDescriptor::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ds 3d composite"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(three_d),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&layers),
                },
            ],
        });
        let target = target(&mut self.target, &self.state, scale);
        let copies = self.clock.as_ref().map_or(0, GpuClock::copies);
        self.burst = copies > 0;
        let mut buffers = vec![commands];
        for copy in 0..=copies {
            if copy > 0 {
                buffers.extend(self.renderer.encode_again(device, None));
            }
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ds 3d composite"),
            });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ds 3d composite"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: self.clock.as_ref().and_then(|clock| clock.end_stamp(copy)),
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..3, 0..1);
            }
            buffers.push(encoder.finish());
        }
        let sent = Instant::now();
        queue.submit(buffers);
        if let Some(clock) = self.clock.as_mut() {
            clock.submitted(queue, sent, self.renderer.passes());
        }
        Some(Upscaled {
            texture: target.id,
            on_top: composite.on_top(),
        })
    }

    pub(crate) fn read_later(&mut self, ticket: Ticket, frame: &[u8], screen_y: usize) {
        if self.reads.len() >= READS {
            let _ = self.state.device.poll(wgpu::PollType::wait_indefinitely());
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };
        let size = u64::from(WIDTH as u32 * target.scale * 4 * HEIGHT as u32 * target.scale);
        self.spare.retain(|buffer| buffer.size() == size);
        let device = &self.state.device;
        let buffer = self.spare.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ds 3d video"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ds 3d video"),
        });
        encoder.copy_texture_to_buffer(
            target.view.texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(WIDTH as u32 * target.scale * 4),
                    rows_per_image: Some(HEIGHT as u32 * target.scale),
                },
            },
            extent(target.scale),
        );
        self.state.queue.submit([encoder.finish()]);
        let state = Arc::new(AtomicU8::new(MAPPING));
        let mapped = Arc::clone(&state);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                mapped.store(if result.is_ok() { MAPPED } else { FAILED }, Relaxed);
            });
        self.reads.push_back(Readback {
            buffer,
            scale: target.scale,
            state,
            ticket,
            frame: frame.to_vec(),
            screen_y,
        });
    }

    pub(crate) fn take_read(&mut self) -> Option<(Ticket, Sharp)> {
        loop {
            let front = self.reads.front()?;
            if front.state.load(Relaxed) == MAPPING {
                let _ = self.state.device.poll(wgpu::PollType::Poll);
            }
            let state = front.state.load(Relaxed);
            if state == MAPPING {
                return None;
            }
            let read = self.reads.pop_front()?;
            if state == FAILED {
                continue;
            }
            let screen = read.buffer.slice(..).get_mapped_range().to_vec();
            read.buffer.unmap();
            self.spare.push(read.buffer);
            let sharp = Sharp {
                frame: read.frame,
                width: WIDTH,
                screen,
                screen_y: read.screen_y,
                scale: read.scale as usize,
            };
            return Some((read.ticket, sharp));
        }
    }

    fn wait_reads(&mut self) {
        if !self.reads.is_empty() {
            let _ = self.state.device.poll(wgpu::PollType::wait_indefinitely());
        }
    }

    #[cfg(test)]
    pub(crate) fn read(&self) -> Option<(u32, Vec<u8>)> {
        let target = self.target.as_ref()?;
        let (width, height) = (WIDTH as u32 * target.scale, HEIGHT as u32 * target.scale);
        let row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let device = &self.state.device;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ds 3d readback"),
            size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ds 3d readback"),
        });
        encoder.copy_texture_to_buffer(
            target.view.texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(height),
                },
            },
            extent(target.scale),
        );
        self.state.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for line in mapped.chunks(row as usize) {
            pixels.extend_from_slice(&line[..(width * 4) as usize]);
        }
        Some((target.scale, pixels))
    }
}

impl Drop for Upscaler {
    fn drop(&mut self) {
        if let Some(target) = self.target.take() {
            self.state.renderer.write().free_texture(&target.id);
        }
    }
}

fn stop_upscaling(upscaler: &mut Option<Upscaler>, recorder: &mut ScreenRecorder, frame: &[u8]) {
    if let Some(mut upscaler) = upscaler.take() {
        upscaler.wait_reads();
        while let Some((ticket, sharp)) = upscaler.take_read() {
            recorder.sharp(ticket, sharp);
        }
    }
    recorder.fill_held(frame);
}

fn first_route(probe: impl Fn(Route) -> Result<(), String>) -> Result<Route, String> {
    let mut failed = Vec::new();
    for route in Route::ALL {
        match probe(route) {
            Ok(()) if failed.is_empty() => return Ok(route),
            Ok(()) => {
                log::warn!(
                    "DS 3D draws on the {route:?} GPU route: {}",
                    failed.join("; ")
                );
                return Ok(route);
            }
            Err(error) => failed.push(format!("{route:?} failed: {error}")),
        }
    }
    let failed = failed.join("; ");
    log::warn!("DS 3D draws on the CPU: {failed}");
    Err(failed)
}

fn target<'a>(slot: &'a mut Option<Target>, state: &RenderState, scale: u32) -> &'a Target {
    if slot.as_ref().is_some_and(|target| target.scale == scale) {
        return slot.as_ref().expect("target");
    }
    let texture = state.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ds 3d screen"),
        size: extent(scale),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut renderer = state.renderer.write();
    let id = match slot.take() {
        Some(old) => {
            renderer.update_egui_texture_from_wgpu_texture(
                &state.device,
                &view,
                wgpu::FilterMode::Linear,
                old.id,
            );
            old.id
        }
        None => renderer.register_native_texture(&state.device, &view, wgpu::FilterMode::Linear),
    };
    slot.insert(Target { scale, view, id })
}

fn textures(filter: TextureFilter) -> Textures {
    match filter {
        TextureFilter::Nearest => Textures::Nearest,
        TextureFilter::Bilinear => Textures::Smooth,
        TextureFilter::Bicubic => Textures::Bicubic,
        TextureFilter::Xbr => Textures::Xbr,
    }
}

fn mipmaps(distant: DistantTextures) -> Mipmaps {
    match distant {
        DistantTextures::Off => Mipmaps::Off,
        DistantTextures::Trilinear => Mipmaps::Trilinear,
        DistantTextures::Anisotropic2x => Mipmaps::Anisotropic2x,
        DistantTextures::Anisotropic4x => Mipmaps::Anisotropic4x,
        DistantTextures::Anisotropic16x => Mipmaps::Anisotropic16x,
    }
}

fn antialiasing(edges: SmoothEdges) -> Antialiasing {
    match edges {
        SmoothEdges::Off => Antialiasing::Off,
        SmoothEdges::On => Antialiasing::Four,
        SmoothEdges::More => Antialiasing::Eight,
    }
}

fn pack(composite: &Composite, into: &mut [u8]) {
    let pixels = composite.pixels().iter().zip(into.as_chunks_mut::<16>().0);
    for (index, (pixel, out)) in pixels.enumerate() {
        let y = index / WIDTH;
        let mix = match pixel.mix {
            Mix::Native => 0,
            Mix::Over => 1,
            Mix::Blend => 2,
        } | u32::from(composite.scroll(y)) << 8
            | pixel.shade.code() << 17
            | composite.master(y).code() << 24;
        for (bytes, word) in
            out.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip([pixel.native, pixel.under, pixel.below, mix])
        {
            bytes.copy_from_slice(&word.to_le_bytes());
        }
    }
}

fn extent(scale: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: WIDTH as u32 * scale,
        height: HEIGHT as u32 * scale,
        depth_or_array_layers: 1,
    }
}

impl super::Emulator {
    pub fn set_gpu(&mut self, ctx: &egui::Context, state: &RenderState) {
        if self.gpu.is_none() {
            self.gpu = Some((ctx.clone(), state.clone()));
        }
    }

    pub(super) fn draw_3d(&mut self, linked: Option<&mut citrine_gb::nds::machine::Nds>) {
        let state = self
            .gpu
            .as_ref()
            .filter(|_| self.ds_3d.upscaled() && !self.gpu_3d_failed)
            .map(|(_, state)| state.clone());
        self.load.reading.upscaling = false;
        let nds = match linked {
            Some(nds) => nds,
            None => match &mut self.seat.core_mut().machine {
                citrine_gb::machine::Machine::Nds(nds) => nds.as_mut(),
                _ => {
                    self.upscaled = None;
                    return;
                }
            },
        };
        if let Some(state) = state.as_ref().filter(|_| self.upscaler.is_none()) {
            self.upscaler = Upscaler::new(state.clone());
            self.gpu_3d_failed = self.upscaler.is_none();
        }
        let state = state.filter(|_| self.upscaler.is_some());
        nds.record_composite(state.is_some());
        let Some(state) = state else {
            stop_upscaling(&mut self.upscaler, &mut self.screen_recorder, nds.frame());
            self.upscaled = None;
            return;
        };
        let Some(upscaler) = self.upscaler.as_mut() else {
            return;
        };
        let ds_3d = self.ds_3d;
        match citrine_gb_nds_gpu::guarded(&state.device, || upscaler.draw(nds, ds_3d)) {
            Ok(shown) => self.upscaled = shown,
            Err(error) => {
                log::warn!("DS 3D moved to the CPU after a GPU error: {error}");
                self.gpu_3d_failed = true;
                stop_upscaling(&mut self.upscaler, &mut self.screen_recorder, nds.frame());
                self.upscaled = None;
                return;
            }
        }
        match self.upscaled {
            Some(shown) => {
                if let Some(ticket) = self.screen_recorder.take_held() {
                    upscaler.read_later(ticket, nds.frame(), shown.screen_y());
                }
            }
            None => upscaler.wait_reads(),
        }
        while let Some((ticket, sharp)) = upscaler.take_read() {
            self.screen_recorder.sharp(ticket, sharp);
        }
        self.screen_recorder.fill_held(nds.frame());
        if upscaler.burst {
            self.clock.excuse();
        }
        self.load.reading.upscaling = true;
        upscaler.timed(&mut self.load.reading.gpu);
    }

    pub(super) fn records_3d(&self) -> bool {
        self.upscaled.is_some() && self.ds_3d.upscaled() && self.gpu.is_some()
    }

    pub(super) fn defers_3d(&self) -> bool {
        self.upscaled.is_some() && self.ds_3d.upscaled() && !self.enable_ghosting
    }

    #[cfg(test)]
    pub(crate) fn is_upscaled(&self) -> bool {
        self.upscaled.is_some()
    }

    pub(super) fn upscaled_screen(&self, ctx: &egui::Context) -> Option<(egui::TextureId, usize)> {
        let (main, _) = self.gpu.as_ref()?;
        let upscaled = self
            .upscaled
            .filter(|_| main == ctx && self.ds_3d.upscaled())?;
        Some((upscaled.texture, upscaled.screen_y()))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::app::ui_state::ds_3d::Resolution;
    use crate::emulator::load::GpuClockKind;
    use citrine_gb::console::Console;
    use citrine_gb::gba::wait::Width;
    use citrine_gb::nds::machine::NdsFirmware;
    use std::sync::Arc;
    use std::time::Duration;
    use strum::IntoEnumIterator;

    const ROM: &[u8] = include_bytes!("../../fixtures/nds/blocksds-lighting_cube.nds");
    const FRAMES: usize = 90;
    const BLDCNT: u32 = 0x0400_0050;
    const BLDY: u32 = 0x0400_0054;
    const MASTER_BRIGHT: u32 = 0x0400_006C;

    pub(crate) fn gpu() -> Option<RenderState> {
        gpu_with(wgpu::Features::TIMESTAMP_QUERY)
    }

    fn gpu_with(features: wgpu::Features) -> Option<RenderState> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        let opened = adapter.ok().and_then(|adapter| {
            let descriptor = wgpu::DeviceDescriptor {
                required_features: adapter.features() & features,
                ..Default::default()
            };
            pollster::block_on(adapter.request_device(&descriptor))
                .ok()
                .map(|(device, queue)| (adapter, device, queue))
        });
        let Some((adapter, device, queue)) = opened else {
            assert!(
                std::env::var_os("CITRINE_GPU_REQUIRED").is_none(),
                "no GPU adapter, and CITRINE_GPU_REQUIRED is set"
            );
            eprintln!("no GPU adapter, skipped");
            return None;
        };
        let renderer = eframe::egui_wgpu::Renderer::new(
            &device,
            TARGET_FORMAT,
            eframe::egui_wgpu::RendererOptions::default(),
        );
        Some(RenderState {
            adapter,
            available_adapters: Vec::new(),
            device,
            queue,
            target_format: TARGET_FORMAT,
            renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
        })
    }

    fn off_screen(upscaler: &Upscaler, nds: &Nds, upscaled: Upscaled, expected: u32) -> usize {
        let (scale, pixels) = upscaler.read().expect("readback");
        assert_eq!(scale, expected);
        let scale = scale as usize;
        let start = upscaled.screen_y() * WIDTH * 4;
        let screen = &nds.frame()[start..start + WIDTH * HEIGHT * 4];
        let centre = scale / 2;
        screen
            .chunks(4)
            .enumerate()
            .filter(|(index, cpu)| {
                let (x, y) = (
                    index % WIDTH * scale + centre,
                    index / WIDTH * scale + centre,
                );
                let at = (y * WIDTH * scale + x) * 4;
                let gpu = &pixels[at..at + 4];
                gpu.iter().zip(cpu.iter()).any(|(a, b)| a.abs_diff(*b) > 6)
            })
            .count()
    }

    fn console() -> Nds {
        Nds::new(ROM.into(), NdsFirmware::default()).expect("the cube boots")
    }

    fn choices() -> Vec<Ds3d> {
        Resolution::iter()
            .flat_map(|resolution| {
                SmoothEdges::iter().map(move |smooth_edges| Ds3d {
                    resolution,
                    smooth_edges,
                    ..Ds3d::default()
                })
            })
            .collect()
    }

    fn clocked(state: &RenderState, bursts: bool) -> Upscaler {
        let mut upscaler = Upscaler::new(state.clone()).expect("an upscaler");
        upscaler.clock.as_mut().expect("timing").burst(bursts);
        upscaler
    }

    fn gpu_times_per_frame(
        state: &RenderState,
        resolutions: [Resolution; 2],
        bursts: bool,
    ) -> [(GpuTotals, Duration); 2] {
        let mut nds = console();
        nds.record_composite(true);
        let mut sides = resolutions.map(|resolution| {
            let upscaler = clocked(state, bursts);
            let choice = Ds3d {
                resolution,
                smooth_edges: SmoothEdges::default(),
                ..Ds3d::default()
            };
            (upscaler, choice, GpuTotals::default())
        });
        for _ in 0..FRAMES {
            nds.run_frame();
            for (upscaler, choice, totals) in &mut sides {
                upscaler.draw(&nds, *choice);
                for _ in 0..2 {
                    let _ = state.device.poll(wgpu::PollType::wait_indefinitely());
                    upscaler.timed(totals);
                }
            }
        }
        sides.map(|(_, _, totals)| {
            let frames = u32::try_from(totals.frames.max(1)).expect("few frames");
            (totals, totals.time / frames)
        })
    }

    fn clock_times_frames(state: &RenderState, bursts: bool) {
        let stamps = state
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY);
        let [(double, at_double), (seven, at_seven)] =
            gpu_times_per_frame(state, [Resolution::Double, Resolution::Seven], bursts);
        eprintln!(
            "{:?}: {at_double:?} a frame at 2x over {} frames, {at_seven:?} at 7x over {}",
            double.kind, double.frames, seven.frames
        );
        let kind = if stamps {
            GpuClockKind::Stamps
        } else {
            GpuClockKind::SubmitToDone
        };
        for (totals, at) in [(double, at_double), (seven, at_seven)] {
            assert_eq!(totals.kind, Some(kind));
            assert!(totals.frames as usize > FRAMES / 2, "{totals:?}");
            if at < Duration::from_millis(2) {
                assert_eq!(totals.work.is_some(), stamps && bursts, "{totals:?}");
            }
            if !bursts || at > Duration::from_millis(4) {
                assert_eq!(totals.work, None, "{totals:?}");
            }
            assert!(totals.work.is_none_or(|work| work > Duration::ZERO));
            let split: Duration = totals.passes.iter().sum();
            assert_eq!(split > Duration::ZERO, stamps, "{totals:?}");
            assert!(split <= totals.time, "{totals:?}");
        }
        assert!(at_double > Duration::ZERO);
        assert!(at_seven > Duration::ZERO);
    }

    #[test]
    fn the_default_3d_filters_textures_with_anisotropic_mipmaps() {
        let choice = Ds3d::default();
        assert_eq!(textures(choice.texture_filter), Textures::Bicubic);
        assert_eq!(mipmaps(choice.distant_textures), Mipmaps::Anisotropic4x);
        assert_eq!(textures(TextureFilter::Nearest), Textures::Nearest);
        assert_eq!(mipmaps(DistantTextures::Off), Mipmaps::Off);
    }

    #[test]
    fn the_gpu_clock_times_upscaled_frames_at_every_resolution() {
        if let Some(state) = gpu() {
            clock_times_frames(&state, false);
        }
    }

    #[test]
    fn a_developer_burst_times_the_3d_at_full_speed() {
        if let Some(state) = gpu() {
            clock_times_frames(&state, true);
        }
    }

    #[test]
    fn without_timestamps_the_gpu_clock_times_frames_from_submit_to_done() {
        if let Some(state) = gpu_with(wgpu::Features::empty()) {
            clock_times_frames(&state, true);
        }
    }

    #[test]
    fn a_full_clock_burst_draws_the_same_picture() {
        let Some(state) = gpu() else {
            return;
        };
        if !state
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return;
        }
        let mut nds = console();
        nds.record_composite(true);
        for _ in 0..FRAMES {
            nds.run_frame();
        }
        let choice = Ds3d {
            resolution: Resolution::Double,
            smooth_edges: SmoothEdges::On,
            ..Ds3d::default()
        };
        let mut plain = Upscaler::new(state.clone()).expect("an upscaler");
        plain.draw(&nds, choice).expect("the cube is upscalable");
        let mut timed = clocked(&state, true);
        timed.draw(&nds, choice).expect("the cube is upscalable");
        let mut first = GpuTotals::default();
        for _ in 0..4 {
            let _ = state.device.poll(wgpu::PollType::wait_indefinitely());
            timed.timed(&mut first);
        }
        assert_eq!(first.frames, 1, "{first:?}");
        if first.time > Duration::from_millis(3) {
            return;
        }
        timed.draw(&nds, choice).expect("the cube is upscalable");
        assert!(timed.burst);
        assert_eq!(plain.read(), timed.read());
    }

    fn luma(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut yuv = vec![0; width * height * 3 / 2];
        crate::media::recording::h264::rgba_to_i420(rgba, width, height, &mut yuv);
        yuv.truncate(width * height);
        yuv
    }

    #[test]
    fn a_video_records_the_gpu_picture_in_step_with_the_sound() {
        use crate::media::recording::headless::{decode, level_fits, psnr, spawner, temp_file};
        use crate::media::recording::{Outcome, ScreenRecorder, Source};
        let Some(state) = gpu() else {
            return;
        };
        let mut nds = console();
        nds.record_composite(true);
        let mut upscaler = Upscaler::new(state).expect("an upscaler");
        let choice = Ds3d {
            resolution: Resolution::Quadruple,
            smooth_edges: SmoothEdges::Off,
            ..Ds3d::default()
        };
        let path = temp_file("ds-gpu.mp4");
        let mut recorder = ScreenRecorder::default();
        let rate = nds.sample_rate();
        let new = crate::media::NewMedia {
            kind: crate::media::MediaKind::Recording,
            game: "cube".into(),
            rom_key: None,
            model: None,
            created_at: 0,
            width: WIDTH as u32,
            height: HEIGHT as u32 * 2,
            length_ms: None,
            frame_times: false,
            video: false,
            replay: None,
            frame: crate::media::Frame {
                width: WIDTH as u32,
                height: HEIGHT as u32 * 2,
                rgba: nds.frame().to_vec(),
            },
        };
        recorder
            .start_with(
                &spawner(),
                crate::media::recording::video_api().unwrap(),
                path.clone(),
                new,
                Source::of(&nds),
                rate,
            )
            .unwrap();
        let mut audio = Vec::new();
        let mut heard = 0;
        let mut last = None;
        for _ in 0..FRAMES {
            nds.run_frame();
            audio.clear();
            nds.take_audio(&mut audio);
            heard += audio.len() / 2;
            recorder.hold(&audio, rate);
            match upscaler.draw(&nds, choice) {
                Some(shown) => {
                    if let Some(ticket) = recorder.take_held() {
                        upscaler.read_later(ticket, nds.frame(), shown.screen_y());
                    }
                    last = Some((nds.frame().to_vec(), shown.screen_y()));
                }
                None => recorder.fill_held(nds.frame()),
            }
            while let Some((ticket, sharp)) = upscaler.take_read() {
                recorder.sharp(ticket, sharp);
            }
        }
        let (frame, screen_y) = last.expect("the cube is upscalable");
        let (scale, screen) = upscaler.read().expect("readback");
        assert_eq!(scale, 4);
        upscaler.wait_reads();
        while let Some((ticket, sharp)) = upscaler.take_read() {
            recorder.sharp(ticket, sharp);
        }
        recorder.stop();
        let Outcome::Saved(done) = recorder.wait().remove(0) else {
            panic!("not saved");
        };
        let video = decode(&done.output.path);
        assert_eq!((video.width, video.height), (WIDTH * 4, HEIGHT * 8));
        let fps = Source::of(&nds).fps();
        assert!(
            level_fits(video.level, video.width, video.height, fps),
            "level {} is too low for {}x{} at {fps:.1} fps",
            video.level,
            video.width,
            video.height
        );
        let expected_frames = (heard as f64 * Source::of(&nds).fps() / f64::from(rate)).round();
        let shown_frames = video.seconds * fps;
        assert!(
            (shown_frames - expected_frames).abs() <= 1.0,
            "{shown_frames:.1} frames of video for {expected_frames} frames of sound"
        );
        let sharp = Sharp {
            frame,
            width: WIDTH,
            screen,
            screen_y,
            scale: 4,
        };
        let mut gpu_picture = Vec::new();
        sharp.draw(4, &mut gpu_picture);
        let cpu_picture = Sharp {
            screen: Vec::new(),
            ..sharp
        };
        let mut cpu_upscaled = Vec::new();
        cpu_picture.draw(4, &mut cpu_upscaled);
        let (width, height) = (video.width, video.height);
        let to_gpu = psnr(&video.last_luma, &luma(&gpu_picture, width, height));
        let to_cpu = psnr(&video.last_luma, &luma(&cpu_upscaled, width, height));
        assert!(
            to_gpu > 35.0,
            "the video is {to_gpu:.1} dB off the GPU picture"
        );
        assert!(
            to_gpu > to_cpu + 1.0,
            "{to_gpu:.1} dB to the GPU picture, {to_cpu:.1} dB to the console's own"
        );
        assert_eq!(done.thumbnail.len(), WIDTH * HEIGHT * 2 * 4);
        std::fs::remove_file(&done.output.path).unwrap();
    }

    #[test]
    fn upscaling_never_changes_the_emulation() {
        let Some(state) = gpu() else {
            return;
        };
        let mut plain = console();
        let mut upscaler = Upscaler::new(state).expect("an upscaler");
        let mut watched = console();
        watched.record_composite(true);
        let mut choices = choices().into_iter().cycle();
        let mut drawn = 0;
        for frame in 0..FRAMES {
            plain.run_frame();
            watched.run_frame();
            assert_eq!(plain.frame(), watched.frame(), "frame {frame}");
            let choice = choices.next().expect("choices repeat");
            drawn += usize::from(upscaler.draw(&watched, choice).is_some());
        }
        assert!(drawn > FRAMES / 2, "only {drawn} frames were upscaled");
        assert_eq!(plain.save_state(0).unwrap(), watched.save_state(0).unwrap());
    }

    #[test]
    fn at_native_scale_the_composite_matches_the_screen_off_the_edges() {
        let Some(state) = gpu() else {
            return;
        };
        let mut nds = console();
        nds.record_composite(true);
        for _ in 0..FRAMES {
            nds.run_frame();
        }
        let mut upscaler = Upscaler::new(state).expect("an upscaler");
        let native = Ds3d {
            resolution: Resolution::Native,
            smooth_edges: SmoothEdges::Off,
            ..Ds3d::default()
        };
        let upscaled = upscaler.draw(&nds, native).expect("the cube is upscalable");
        let off = off_screen(&upscaler, &nds, upscaled, 1);
        assert!(
            off * 100 <= WIDTH * HEIGHT,
            "{off} pixels differ, more than the DS's own edge smoothing explains"
        );
    }

    #[test]
    fn a_gpu_route_that_loses_its_device_hands_the_3d_to_the_next_route() {
        let Some(state) = gpu() else {
            return;
        };
        let route = first_route(|route| {
            let (device, _queue) = pollster::block_on(
                state
                    .adapter
                    .request_device(&wgpu::DeviceDescriptor::default()),
            )
            .map_err(|error| error.to_string())?;
            if route == Route::Compute {
                device.destroy();
            }
            citrine_gb_nds_gpu::probe_on(&device, route)
        });
        assert_eq!(route, Ok(Route::Fragment));
        let mut upscaler = Upscaler::on_route(state, Route::Fragment).expect("the fragment route");
        assert_eq!(upscaler.renderer.route(), Route::Fragment);
        let mut nds = console();
        nds.record_composite(true);
        for _ in 0..FRAMES {
            nds.run_frame();
        }
        let native = Ds3d {
            resolution: Resolution::Native,
            smooth_edges: SmoothEdges::Off,
            ..Ds3d::default()
        };
        let upscaled = upscaler.draw(&nds, native).expect("the cube is drawn");
        let off = off_screen(&upscaler, &nds, upscaled, 1);
        assert!(
            off * 100 <= WIDTH * HEIGHT,
            "{off} pixels differ from the CPU picture"
        );
    }

    #[test]
    fn a_lost_device_gives_no_upscaler_instead_of_a_crash() {
        let Some(state) = gpu() else {
            return;
        };
        state.device.destroy();
        assert!(Upscaler::on_route(state.clone(), Route::Compute).is_none());
        assert!(Upscaler::on_route(state, Route::Fragment).is_none());
        assert!(first_route(|_| Err("lost".into())).is_err());
    }

    #[test]
    fn fades_keep_the_3d_upscaled() {
        let Some(state) = gpu() else {
            return;
        };
        let mut upscaler = Upscaler::new(state).expect("an upscaler");
        let fades: [&[(u32, u32)]; 5] = [
            &[(MASTER_BRIGHT, 1 << 14 | 9)],
            &[(MASTER_BRIGHT, 2 << 14 | 9)],
            &[(BLDCNT, 0x3F | 2 << 6), (BLDY, 10)],
            &[(BLDCNT, 0x3F | 3 << 6), (BLDY, 10)],
            &[
                (BLDCNT, 0x3F | 3 << 6),
                (BLDY, 6),
                (MASTER_BRIGHT, 1 << 14 | 12),
            ],
        ];
        for writes in fades {
            let mut nds = console();
            nds.record_composite(true);
            for _ in 0..FRAMES {
                nds.run_frame();
            }
            for &(register, value) in writes {
                nds.buses_mut().arm9().write(register, Width::Half, value);
            }
            nds.run_frame();
            for resolution in [Resolution::Native, Resolution::Quadruple] {
                let choice = Ds3d {
                    resolution,
                    smooth_edges: SmoothEdges::Off,
                    ..Ds3d::default()
                };
                let upscaled = upscaler
                    .draw(&nds, choice)
                    .unwrap_or_else(|| panic!("{writes:x?} drops to native at {resolution:?}"));
                let off = off_screen(&upscaler, &nds, upscaled, resolution.scale());
                assert!(
                    off * 100 <= WIDTH * HEIGHT,
                    "{writes:x?} at {resolution:?}: {off} pixels differ from the screen"
                );
            }
        }
    }
}
