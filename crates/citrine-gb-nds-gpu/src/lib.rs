mod atlas;
mod scene;

use citrine_gb::nds::geometry::frame::Frame;
use citrine_gb::nds::render::texture::TextureMemory;
use citrine_gb::nds::render::{ALPHA_TEST_REF, Attribute, DISP3DCNT, Pixel, Registers};
use citrine_gb::nds::video::{HEIGHT, WIDTH};
use scene::{DATA_WIDTH, Scene, TILES_X};

pub const MAX_SCALE: u32 = 8;

const RASTER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;
const DISPLAY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const TEXEL_BYTES: u32 = 16;
const UNIFORM_WORDS: usize = 36;
const ATLAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Uint;
const ATLAS_LIMIT: u32 = 4096;
const ATLAS_VERTICES: u32 = 6;
const EDGE_GROUP: u32 = 8;
const REFERENCE_GROUP: u32 = 8;
pub const FIRST_PASS_STAMP: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Mipmaps,
    Reference,
    Opaque,
    Tail,
    Edges,
    Finish,
    Present,
}

impl Pass {
    pub const ALL: [Pass; 7] = [
        Pass::Mipmaps,
        Pass::Reference,
        Pass::Opaque,
        Pass::Tail,
        Pass::Edges,
        Pass::Finish,
        Pass::Present,
    ];
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Antialiasing {
    #[default]
    Off,
    Four,
    Eight,
}

impl Antialiasing {
    fn samples(self) -> u32 {
        match self {
            Self::Off => 1,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Textures {
    #[default]
    Nearest,
    Smooth,
    Bicubic,
    Xbr,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mipmaps {
    #[default]
    Off,
    Trilinear,
    Anisotropic2x,
    Anisotropic4x,
    Anisotropic16x,
}

impl Mipmaps {
    fn ratio(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Trilinear => 1,
            Self::Anisotropic2x => 2,
            Self::Anisotropic4x => 4,
            Self::Anisotropic16x => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upscale {
    pub scale: u32,
    pub antialiasing: Antialiasing,
    pub textures: Textures,
    pub mipmaps: Mipmaps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Compute,
    Fragment,
}

impl Route {
    pub const ALL: [Route; 2] = [Route::Compute, Route::Fragment];

    pub fn allowed(self, adapter: &wgpu::Adapter) -> bool {
        let drawn = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let raster = match self {
            Route::Compute => drawn | wgpu::TextureUsages::STORAGE_BINDING,
            Route::Fragment => drawn,
        };
        [
            (RASTER_FORMAT, raster),
            (ATLAS_FORMAT, drawn),
            (DISPLAY_FORMAT, drawn),
        ]
        .into_iter()
        .all(|(format, usages)| {
            adapter
                .get_texture_format_features(format)
                .allowed_usages
                .contains(usages)
        })
    }
}

impl Default for Upscale {
    fn default() -> Self {
        Self {
            scale: 1,
            antialiasing: Antialiasing::Off,
            textures: Textures::Nearest,
            mipmaps: Mipmaps::Off,
        }
    }
}

pub struct Renderer {
    raster: wgpu::RenderPipeline,
    raster_reference: Option<wgpu::ComputePipeline>,
    raster_edges: Edges,
    finish: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    raster_layout: wgpu::BindGroupLayout,
    edges_layout: Option<wgpu::BindGroupLayout>,
    present_layout: wgpu::BindGroupLayout,
    reference_uniforms: wgpu::Buffer,
    upscaled_uniforms: wgpu::Buffer,
    placeholder: wgpu::TextureView,
    reference: Target,
    centers: Option<Target>,
    upscaled: Option<Target>,
    finished: Option<Target>,
    finishes: bool,
    edges_only: bool,
    display: Option<Target>,
    data: Option<(u32, wgpu::Texture)>,
    scene: Scene,
    scale: u32,
    pass_stamps: Option<wgpu::QuerySet>,
    hardware: Hardware,
    opaque: Option<Target>,
    decode: wgpu::RenderPipeline,
    reduce: wgpu::RenderPipeline,
    atlas: Option<Atlas>,
    atlas_pending: bool,
    atlas_levels: u32,
}

struct Atlas {
    height: u32,
    view: wgpu::TextureView,
    levels: Vec<wgpu::TextureView>,
}

impl Atlas {
    fn new(device: &wgpu::Device, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nds 3d atlas"),
            size: extent(atlas::WIDTH, height),
            mip_level_count: atlas::LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: ATLAS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = (0..atlas::LEVELS)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..wgpu::TextureViewDescriptor::default()
                })
            })
            .collect();
        Self {
            height,
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            levels,
        }
    }
}

struct Target {
    scale: u32,
    storage: bool,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Target {
    fn new(device: &wgpu::Device, scale: u32, format: wgpu::TextureFormat) -> Self {
        Self::with_storage(device, scale, format, false)
    }

    fn with_storage(
        device: &wgpu::Device,
        scale: u32,
        format: wgpu::TextureFormat,
        storage: bool,
    ) -> Self {
        let storage_usage = if storage {
            wgpu::TextureUsages::STORAGE_BINDING
        } else {
            wgpu::TextureUsages::empty()
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("nds 3d target"),
            size: extent(WIDTH as u32 * scale, HEIGHT as u32 * scale),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | storage_usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            scale,
            storage,
            texture,
            view,
        }
    }
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Self {
        Self::zeroing_workgroups(device, false)
    }

    pub fn with_route(device: &wgpu::Device, route: Route) -> Self {
        Self::build(device, route, false)
    }

    pub fn zeroing_workgroups(device: &wgpu::Device, zero: bool) -> Self {
        Self::build(device, Route::Compute, zero)
    }

    pub fn route(&self) -> Route {
        match self.edges_layout {
            Some(_) => Route::Compute,
            None => Route::Fragment,
        }
    }

    fn build(device: &wgpu::Device, route: Route, zero: bool) -> Self {
        let compute = route == Route::Compute && computes(device);
        let uint_texture = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let stages = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let raster_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: stages | wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            uint_texture(1, stages | wgpu::ShaderStages::VERTEX),
            uint_texture(2, stages),
            uint_texture(3, stages),
            uint_texture(5, stages),
        ];
        let raster_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d raster"),
            entries: &raster_entries,
        });
        let edges_layout = compute.then(|| {
            let mut edges_entries = raster_entries.to_vec();
            edges_entries.push(wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: RASTER_FORMAT,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            });
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("nds 3d edges"),
                entries: &edges_entries,
            })
        });
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d present"),
            entries: &[uint_texture(0, stages)],
        });
        let raster_source = include_str!("raster.wgsl");
        let hardware = Hardware::new(device, &raster_layout, raster_source);
        let raster_pipeline = |fragment| {
            pipeline(
                device,
                &raster_layout,
                raster_source,
                ("vs_main", fragment),
                RASTER_FORMAT,
            )
        };
        let atlas_pipeline = |fragment| {
            pipeline(
                device,
                &raster_layout,
                raster_source,
                ("vs_atlas", fragment),
                ATLAS_FORMAT,
            )
        };
        let present = pipeline(
            device,
            &present_layout,
            include_str!("present.wgsl"),
            ("vs_main", "fs_present"),
            DISPLAY_FORMAT,
        );
        let uniforms = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("nds 3d uniforms"),
                size: (UNIFORM_WORDS * 4) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let placeholder = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("nds 3d placeholder"),
                size: extent(1, 1),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: RASTER_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let decode = atlas_pipeline("fs_decode");
        let reduce = atlas_pipeline("fs_reduce");
        Self {
            raster: raster_pipeline("fs_raster"),
            raster_reference: edges_layout
                .as_ref()
                .map(|layout| compute_pipeline(device, layout, raster_source, "cs_raster", zero)),
            raster_edges: match edges_layout.as_ref() {
                Some(layout) => Edges::Compute(compute_pipeline(
                    device,
                    layout,
                    raster_source,
                    "cs_edges",
                    zero,
                )),
                None => Edges::Fragment(raster_pipeline("fs_edges")),
            },
            finish: raster_pipeline("fs_finish"),
            present,
            raster_layout,
            edges_layout,
            present_layout,
            reference_uniforms: uniforms(),
            upscaled_uniforms: uniforms(),
            placeholder,
            reference: Target::with_storage(device, 1, RASTER_FORMAT, compute),
            centers: None,
            upscaled: None,
            finished: None,
            finishes: false,
            edges_only: false,
            display: None,
            data: None,
            scene: Scene::default(),
            scale: 1,
            pass_stamps: None,
            hardware,
            opaque: None,
            decode,
            reduce,
            atlas: None,
            atlas_pending: false,
            atlas_levels: 0,
        }
        .with_atlas_limit(device)
    }

    fn with_atlas_limit(mut self, device: &wgpu::Device) -> Self {
        self.scene.atlas_limit = device.limits().max_texture_dimension_2d.min(ATLAS_LIMIT);
        self
    }

    pub fn stamp_passes(&mut self, query_set: Option<wgpu::QuerySet>) {
        self.pass_stamps = query_set;
    }

    pub fn render<M: TextureMemory>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        registers: &Registers,
        memory: &M,
        upscale: Upscale,
    ) {
        let commands = self.encode(device, queue, frame, registers, memory, upscale);
        queue.submit([commands]);
    }

    pub fn encode<M: TextureMemory>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        registers: &Registers,
        memory: &M,
        upscale: Upscale,
    ) -> wgpu::CommandBuffer {
        let scale = upscale.scale.clamp(1, MAX_SCALE);
        self.scale = scale;
        self.scene.smooth = scale > 1 && upscale.textures != Textures::Nearest;
        self.scene.upscaled = scale > 1;
        self.scene.mipmaps = scale > 1 && upscale.mipmaps != Mipmaps::Off;
        self.scene.atlased = self.scene.mipmaps || self.scene.smooth;
        let offsets = self.scene.pack(frame, memory);
        self.prepare_atlas(device);
        let rows = self.scene.rows();
        let skip = if self
            .data
            .as_ref()
            .is_none_or(|(capacity, _)| *capacity < rows)
        {
            self.data = Some((rows, data_texture(device, rows)));
            0..0
        } else {
            self.scene.unchanged_rows()
        };
        let data = &self.data.as_ref().expect("data texture").1;
        for part in [0..skip.start, skip.end.max(skip.start)..rows] {
            if part.is_empty() {
                continue;
            }
            let words = DATA_WIDTH as usize * 4;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: data,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: part.start,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                words_bytes(
                    &self.scene.words[part.start as usize * words..part.end as usize * words],
                ),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(DATA_WIDTH * TEXEL_BYTES),
                    rows_per_image: Some(part.len() as u32),
                },
                extent(DATA_WIDTH, part.len() as u32),
            );
        }
        let edge = registers.edge_colors();
        let fog = registers.fog_table();
        let uniforms = |scale: u32, samples: u32, textures: Textures| -> [u32; UNIFORM_WORDS] {
            let mut words = [0; UNIFORM_WORDS];
            words[..16].copy_from_slice(&[
                scale,
                registers.read(DISP3DCNT),
                registers.read(ALPHA_TEST_REF),
                samples,
                scene::pack_pixel(registers.rear_color()),
                registers.rear_depth(),
                scene::pack_attribute(registers.rear_attribute()),
                offsets.tail,
                offsets.bins,
                offsets.vram,
                offsets.palette,
                TILES_X as u32,
                match textures {
                    Textures::Nearest => 0,
                    Textures::Smooth => 1,
                    Textures::Bicubic => 2,
                    Textures::Xbr => 3,
                },
                1,
                0,
                0,
            ]);
            for (word, pair) in words[16..20].iter_mut().zip(edge.as_chunks::<2>().0) {
                *word = u32::from(pair[0]) | u32::from(pair[1]) << 16;
            }
            words[20] = registers.fog_color();
            words[21] = registers.fog_offset();
            for (word, bytes) in words[24..32].iter_mut().zip(fog.as_chunks::<4>().0) {
                *word = u32::from_le_bytes(*bytes);
            }
            words[32..].copy_from_slice(&[
                offsets.atlas,
                if self.scene.mipmaps {
                    upscale.mipmaps.ratio()
                } else {
                    0
                },
                atlas::WIDTH,
                self.scene.atlas.height(),
            ]);
            words
        };
        if scale == 1 {
            queue.write_buffer(
                &self.reference_uniforms,
                0,
                words_bytes(&uniforms(1, 1, upscale.textures)),
            );
        }
        let samples = upscale.antialiasing.samples();
        self.edges_only = scale > 1 && samples > 1;
        let stores = scale > 1 && matches!(self.raster_edges, Edges::Compute(_));
        let mut upscaled_words = uniforms(scale, samples, upscale.textures);
        upscaled_words[14] = u32::from(samples == 1);
        queue.write_buffer(&self.upscaled_uniforms, 0, words_bytes(&upscaled_words));
        if scale > 1 {
            if self
                .opaque
                .as_ref()
                .is_none_or(|target| target.scale != scale)
            {
                self.opaque = Some(Target::new(device, scale, RASTER_FORMAT));
            }
            self.hardware
                .prepare(device, queue, &self.scene.hardware, self.scene.slabs, scale);
        }
        if scale > 1
            && self
                .upscaled
                .as_ref()
                .is_none_or(|target| target.scale != scale || target.storage != stores)
        {
            self.upscaled = Some(Target::with_storage(device, scale, RASTER_FORMAT, stores));
        }
        if self.edges_only
            && self
                .centers
                .as_ref()
                .is_none_or(|target| target.scale != scale)
        {
            self.centers = Some(Target::new(device, scale, RASTER_FORMAT));
        }
        self.finishes = registers.marks_edges_or_fogs();
        if self.finishes
            && self
                .finished
                .as_ref()
                .is_none_or(|target| target.scale != scale)
        {
            self.finished = Some(Target::new(device, scale, RASTER_FORMAT));
        }
        if self
            .display
            .as_ref()
            .is_none_or(|target| target.scale != scale)
        {
            self.display = Some(Target::new(device, scale, DISPLAY_FORMAT));
        }
        self.record(device, self.pass_stamps.as_ref())
            .expect("a prepared scene")
    }

    fn prepare_atlas(&mut self, device: &wgpu::Device) {
        if !self.scene.atlased {
            self.atlas_pending = false;
            self.atlas_levels = 0;
            return;
        }
        let height = self.scene.atlas.height();
        let fresh = self
            .atlas
            .as_ref()
            .is_none_or(|atlas| atlas.height != height);
        if fresh {
            self.atlas = Some(Atlas::new(device, height));
        }
        let levels = if self.scene.mipmaps {
            self.scene.atlas.levels()
        } else {
            self.scene.atlas.levels().min(1)
        };
        self.atlas_pending = (fresh || self.scene.atlas_changed() || levels > self.atlas_levels)
            && self.scene.atlas.len() > 0;
        self.atlas_levels = levels;
    }

    fn record_atlas(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        data_view: &wgpu::TextureView,
        stamps: Option<(&wgpu::QuerySet, u32)>,
    ) {
        let Some(atlas) = self.atlas.as_ref() else {
            return;
        };
        let count = self.scene.atlas.len();
        let levels = self.atlas_levels;
        for level in 0..levels {
            let (pipeline, source) = match level {
                0 => (&self.decode, &self.placeholder),
                _ => (&self.reduce, &atlas.levels[level as usize - 1]),
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nds 3d atlas"),
                layout: &self.raster_layout,
                entries: &group_entries(
                    &self.upscaled_uniforms,
                    data_view,
                    &self.placeholder,
                    &self.placeholder,
                    source,
                ),
            });
            let timestamp_writes =
                stamps
                    .filter(|_| level == 0 || level + 1 == levels)
                    .map(|(query_set, index)| wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: (level == 0).then_some(0),
                        end_of_pass_write_index: (level + 1 == levels).then_some(index),
                    });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nds 3d atlas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &atlas.levels[level as usize],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            let first = level * ATLAS_VERTICES;
            pass.draw(first..first + ATLAS_VERTICES, 0..count);
        }
    }

    fn atlas_view(&self) -> &wgpu::TextureView {
        match self.atlas.as_ref() {
            Some(atlas) if self.scene.atlased => &atlas.view,
            _ => &self.placeholder,
        }
    }

    pub fn encode_again(
        &self,
        device: &wgpu::Device,
        pass_stamps: Option<&wgpu::QuerySet>,
    ) -> Option<wgpu::CommandBuffer> {
        self.record(device, pass_stamps)
    }

    fn record(
        &self,
        device: &wgpu::Device,
        pass_stamps: Option<&wgpu::QuerySet>,
    ) -> Option<wgpu::CommandBuffer> {
        let scale = self.scale;
        let data = &self.data.as_ref()?.1;
        let data_view = data.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nds 3d"),
        });
        let mut stamp = FIRST_PASS_STAMP;
        let mut end = || {
            let index = stamp;
            stamp += 1;
            pass_stamps.map(|query_set| (query_set, index))
        };
        let atlas_view = self.atlas_view();
        if scale > 1 && self.atlas_pending {
            self.record_atlas(device, &mut encoder, &data_view, end());
        }
        let raster_group = |uniforms: &wgpu::Buffer,
                            reference: &wgpu::TextureView,
                            centers: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nds 3d raster"),
                layout: &self.raster_layout,
                entries: &group_entries(uniforms, &data_view, reference, centers, atlas_view),
            })
        };
        match self
            .raster_reference
            .as_ref()
            .zip(self.edges_layout.as_ref())
        {
            _ if scale > 1 => {}
            Some((raster, layout)) => {
                let mut entries = group_entries(
                    &self.reference_uniforms,
                    &data_view,
                    &self.placeholder,
                    &self.placeholder,
                    &self.placeholder,
                )
                .to_vec();
                entries.push(wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.reference.view),
                });
                let reference_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("nds 3d reference"),
                    layout,
                    entries: &entries,
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("nds 3d reference"),
                    timestamp_writes: end().map(|(query_set, index)| {
                        wgpu::ComputePassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: (index == FIRST_PASS_STAMP).then_some(0),
                            end_of_pass_write_index: Some(index),
                        }
                    }),
                });
                pass.set_pipeline(raster);
                pass.set_bind_group(0, &reference_group, &[]);
                pass.dispatch_workgroups(
                    (WIDTH as u32).div_ceil(REFERENCE_GROUP),
                    (HEIGHT as u32).div_ceil(REFERENCE_GROUP),
                    1,
                );
            }
            None => draw(
                &mut encoder,
                &self.raster,
                &raster_group(
                    &self.reference_uniforms,
                    &self.placeholder,
                    &self.placeholder,
                ),
                &self.reference.view,
                end().map(|(query_set, index)| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: (index == FIRST_PASS_STAMP).then_some(0),
                    end_of_pass_write_index: Some(index),
                }),
            ),
        }
        if let (Some(upscaled), Some(opaque)) = (
            self.upscaled.as_ref().filter(|_| scale > 1),
            self.opaque.as_ref(),
        ) {
            let centers = self.centers.as_ref().filter(|_| self.edges_only);
            self.hardware.record(
                device,
                &mut encoder,
                &self.upscaled_uniforms,
                (&data_view, atlas_view),
                &opaque.view,
                end().map(|(query_set, index)| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: (index == FIRST_PASS_STAMP).then_some(0),
                    end_of_pass_write_index: Some(index),
                }),
            );
            draw(
                &mut encoder,
                &self.hardware.tail,
                &raster_group(&self.upscaled_uniforms, &opaque.view, &opaque.view),
                &centers.unwrap_or(upscaled).view,
                ended(end()),
            );
            match (centers, &self.raster_edges, self.edges_layout.as_ref()) {
                (Some(centers), Edges::Fragment(edges), _) => draw(
                    &mut encoder,
                    edges,
                    &raster_group(&self.upscaled_uniforms, &opaque.view, &centers.view),
                    &upscaled.view,
                    ended(end()),
                ),
                (Some(centers), Edges::Compute(edges), Some(layout)) => {
                    let mut entries = group_entries(
                        &self.upscaled_uniforms,
                        &data_view,
                        &opaque.view,
                        &centers.view,
                        atlas_view,
                    )
                    .to_vec();
                    entries.push(wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&upscaled.view),
                    });
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("nds 3d edges"),
                        layout,
                        entries: &entries,
                    });
                    dispatch(
                        &mut encoder,
                        edges,
                        &group,
                        end(),
                        (
                            (WIDTH as u32 * scale).div_ceil(EDGE_GROUP),
                            (HEIGHT as u32 * scale).div_ceil(EDGE_GROUP),
                        ),
                    );
                }
                _ => {}
            }
        }
        if let Some(finished) = self.finished.as_ref().filter(|_| self.finishes) {
            let uniforms = if scale > 1 {
                &self.upscaled_uniforms
            } else {
                &self.reference_uniforms
            };
            draw(
                &mut encoder,
                &self.finish,
                &raster_group(uniforms, &self.placeholder, &self.raster_output().view),
                &finished.view,
                ended(end()),
            );
        }
        let present_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nds 3d present"),
            layout: &self.present_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&self.output().view),
            }],
        });
        draw(
            &mut encoder,
            &self.present,
            &present_group,
            &self.display.as_ref()?.view,
            ended(end()),
        );
        Some(encoder.finish())
    }

    pub fn passes(&self) -> Vec<Pass> {
        let mut passes = Vec::new();
        if self.scale > 1 && self.atlas_pending {
            passes.push(Pass::Mipmaps);
        }
        if self.upscaled.is_some() && self.scale > 1 {
            passes.extend([Pass::Opaque, Pass::Tail]);
            if self.centers.is_some() && self.edges_only {
                passes.push(Pass::Edges);
            }
        } else {
            passes.push(Pass::Reference);
        }
        if self.finished.is_some() && self.finishes {
            passes.push(Pass::Finish);
        }
        passes.push(Pass::Present);
        passes
    }

    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.display.as_ref().map(|target| &target.view)
    }

    pub fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Picture {
        let output = self.output();
        let (width, height) = (WIDTH as u32 * output.scale, HEIGHT as u32 * output.scale);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("nds 3d readback"),
            size: u64::from(width * height * TEXEL_BYTES),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("nds 3d readback"),
        });
        encoder.copy_texture_to_buffer(
            output.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * TEXEL_BYTES),
                    rows_per_image: Some(height),
                },
            },
            extent(width, height),
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| {
            result.expect("map the readback buffer");
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for the readback");
        let bytes = slice.get_mapped_range();
        let (chunks, _) = bytes.as_chunks::<{ TEXEL_BYTES as usize }>();
        let texels = chunks
            .iter()
            .map(|texel| {
                [0, 1, 2, 3].map(|word| {
                    let at = word * 4;
                    u32::from_le_bytes([texel[at], texel[at + 1], texel[at + 2], texel[at + 3]])
                })
            })
            .collect();
        Picture {
            scale: output.scale,
            texels,
        }
    }

    fn output(&self) -> &Target {
        match &self.finished {
            Some(target) if self.finishes => target,
            _ => self.raster_output(),
        }
    }

    fn raster_output(&self) -> &Target {
        match &self.upscaled {
            Some(target) if self.scale > 1 => target,
            _ => &self.reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    scale: u32,
    texels: Vec<[u32; 4]>,
}

impl Picture {
    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn width(&self) -> usize {
        WIDTH * self.scale as usize
    }

    pub fn height(&self) -> usize {
        HEIGHT * self.scale as usize
    }

    pub fn pixel(&self, x: usize, y: usize) -> Pixel {
        scene::unpack_pixel(self.texel(x, y)[0])
    }

    pub fn depth(&self, x: usize, y: usize) -> u32 {
        self.texel(x, y)[1]
    }

    pub fn attribute(&self, x: usize, y: usize) -> Attribute {
        scene::unpack_attribute(self.texel(x, y)[2])
    }

    fn texel(&self, x: usize, y: usize) -> [u32; 4] {
        self.texels[y * self.width() + x]
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &str,
    (vertex, fragment): (&str, &str),
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(fragment),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(fragment),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vertex),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn group_entries<'a>(
    uniforms: &'a wgpu::Buffer,
    data: &'a wgpu::TextureView,
    reference: &'a wgpu::TextureView,
    centers: &'a wgpu::TextureView,
    atlas: &'a wgpu::TextureView,
) -> [wgpu::BindGroupEntry<'a>; 5] {
    [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(data),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(reference),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(centers),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(atlas),
        },
    ]
}

const EDGE_STORAGE: u32 = 12 * 1024;

enum Edges {
    Fragment(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
}

pub fn guarded<T>(device: &wgpu::Device, build: impl FnOnce() -> T) -> Result<T, wgpu::Error> {
    const FILTERS: [wgpu::ErrorFilter; 3] = [
        wgpu::ErrorFilter::Validation,
        wgpu::ErrorFilter::OutOfMemory,
        wgpu::ErrorFilter::Internal,
    ];
    for filter in FILTERS {
        device.push_error_scope(filter);
    }
    let built = build();
    let mut failed = None;
    for _ in FILTERS {
        if let Some(Some(error)) = now(device.pop_error_scope()) {
            failed.get_or_insert(error);
        }
    }
    match failed {
        Some(error) => Err(error),
        None => Ok(built),
    }
}

pub fn set_up<T>(device: &wgpu::Device, build: impl FnOnce() -> T) -> Result<T, wgpu::Error> {
    guarded(device, || {
        let built = build();
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("nds 3d device check"),
                size: extent(1, 1),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DISPLAY_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        built
    })
}

pub fn probe(adapter: &wgpu::Adapter, like: &wgpu::Device, route: Route) -> Result<(), String> {
    if !route.allowed(adapter) {
        return Err(format!("{route:?} needs texture usages this GPU lacks"));
    }
    let descriptor = wgpu::DeviceDescriptor {
        label: Some("nds 3d probe"),
        required_features: like.features(),
        required_limits: like.limits(),
        ..wgpu::DeviceDescriptor::default()
    };
    let (device, _queue) = now(adapter.request_device(&descriptor))
        .ok_or("the probe device did not open at once")?
        .map_err(|error| error.to_string())?;
    probe_on(&device, route)
}

pub fn probe_on(device: &wgpu::Device, route: Route) -> Result<(), String> {
    set_up(device, || Renderer::with_route(device, route))
        .map(drop)
        .map_err(|error| error.to_string())
}

fn now<F: std::future::Future>(future: F) -> Option<F::Output> {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(output) => Some(output),
        std::task::Poll::Pending => None,
    }
}

fn computes(device: &wgpu::Device) -> bool {
    let limits = device.limits();
    limits.max_compute_invocations_per_workgroup >= EDGE_GROUP * EDGE_GROUP
        && limits.max_compute_workgroup_storage_size >= EDGE_STORAGE
        && limits.max_storage_textures_per_shader_stage > 0
}

fn compute_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &str,
    entry: &str,
    zero: bool,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(entry),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(entry),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(&layout),
        module: &module,
        entry_point: Some(entry),
        compilation_options: wgpu::PipelineCompilationOptions {
            zero_initialize_workgroup_memory: zero,
            ..Default::default()
        },
        cache: None,
    })
}

fn ended(stamp: Option<(&wgpu::QuerySet, u32)>) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    stamp.map(|(query_set, index)| wgpu::RenderPassTimestampWrites {
        query_set,
        beginning_of_pass_write_index: None,
        end_of_pass_write_index: Some(index),
    })
}

fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    group: &wgpu::BindGroup,
    stamp: Option<(&wgpu::QuerySet, u32)>,
    (x, y): (u32, u32),
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("nds 3d"),
        timestamp_writes: stamp.map(|(query_set, index)| wgpu::ComputePassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(index),
        }),
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.dispatch_workgroups(x, y, 1);
}

fn draw(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    target: &wgpu::TextureView,
    stamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("nds 3d"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: stamps,
        occlusion_query_set: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

fn data_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nds 3d data"),
        size: extent(DATA_WIDTH, rows),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RASTER_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn words_bytes(words: &[u32]) -> &[u8] {
    bytemuck::cast_slice(words)
}

const HARDWARE_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SLAB_VERTICES: u32 = 6;
const EDGE_VERTICES: u32 = 66;
const INSTANCE_BYTES: u64 = 8;

struct Hardware {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    tail: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    slabs: u32,
    count: u32,
    depth: Option<(u32, wgpu::TextureView)>,
}

impl Hardware {
    fn new(device: &wgpu::Device, raster_layout: &wgpu::BindGroupLayout, source: &str) -> Self {
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d hardware"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: both,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: both,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nds 3d hardware"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nds 3d hardware"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nds 3d hardware"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_hw"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_hw"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: RASTER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: HARDWARE_DEPTH,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            tail: self::pipeline(
                device,
                raster_layout,
                source,
                ("vs_main", "fs_tail"),
                RASTER_FORMAT,
            ),
            layout,
            pipeline,
            instances: instance_buffer(device, 8192),
            slabs: 0,
            count: 0,
            depth: None,
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[[u32; 2]],
        slabs: u32,
        scale: u32,
    ) {
        if self.instances.size() < instances.len() as u64 * INSTANCE_BYTES {
            self.instances = instance_buffer(device, instances.len().next_power_of_two() as u64);
        }
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, words_bytes(instances.as_flattened()));
        }
        self.slabs = slabs;
        self.count = instances.len() as u32;
        if self.depth.as_ref().is_none_or(|(held, _)| *held != scale) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("nds 3d hardware depth"),
                size: extent(WIDTH as u32 * scale, HEIGHT as u32 * scale),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HARDWARE_DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((
                scale,
                texture.create_view(&wgpu::TextureViewDescriptor::default()),
            ));
        }
    }

    fn record(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &wgpu::Buffer,
        (data, atlas): (&wgpu::TextureView, &wgpu::TextureView),
        target: &wgpu::TextureView,
        stamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let Some((_, depth)) = &self.depth else {
            return;
        };
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("nds 3d hardware"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(data),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(atlas),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nds 3d hardware"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: stamps,
            occlusion_query_set: None,
        });
        if self.count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
        }
        if self.slabs > 0 {
            pass.set_vertex_buffer(0, self.instances.slice(..));
            pass.draw(0..SLAB_VERTICES, 0..self.slabs);
        }
        if self.count > self.slabs {
            let edges = u64::from(self.slabs) * INSTANCE_BYTES;
            pass.set_vertex_buffer(0, self.instances.slice(edges..));
            pass.draw(0..EDGE_VERTICES, 0..self.count - self.slabs);
        }
    }
}

fn instance_buffer(device: &wgpu::Device, instances: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("nds 3d hardware instances"),
        size: instances * INSTANCE_BYTES,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
