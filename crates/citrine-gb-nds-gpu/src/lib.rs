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
const UNIFORM_WORDS: usize = 32;
const EDGE_GROUP: u32 = 8;
const REFERENCE_GROUP: u32 = 8;
pub const FIRST_PASS_STAMP: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Reference,
    Upscaled,
    Centers,
    Edges,
    Finish,
    Present,
}

impl Pass {
    pub const ALL: [Pass; 6] = [
        Pass::Reference,
        Pass::Upscaled,
        Pass::Centers,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upscale {
    pub scale: u32,
    pub antialiasing: Antialiasing,
    pub textures: Textures,
}

impl Default for Upscale {
    fn default() -> Self {
        Self {
            scale: 1,
            antialiasing: Antialiasing::Off,
            textures: Textures::Nearest,
        }
    }
}

pub struct Renderer {
    raster: wgpu::RenderPipeline,
    raster_reference: Option<wgpu::ComputePipeline>,
    raster_centers: wgpu::RenderPipeline,
    raster_edges: Edges,
    finish: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    raster_layout: wgpu::BindGroupLayout,
    edges_layout: wgpu::BindGroupLayout,
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
        let uint_texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Uint,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let raster_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            uint_texture(1),
            uint_texture(2),
            uint_texture(3),
        ];
        let raster_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d raster"),
            entries: &raster_entries,
        });
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
        let edges_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d edges"),
            entries: &edges_entries,
        });
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nds 3d present"),
            entries: &[uint_texture(0)],
        });
        let raster_source = include_str!("raster.wgsl");
        let raster_pipeline = |fragment| {
            pipeline(
                device,
                &raster_layout,
                raster_source,
                fragment,
                RASTER_FORMAT,
            )
        };
        let present = pipeline(
            device,
            &present_layout,
            include_str!("present.wgsl"),
            "fs_present",
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
        Self {
            raster: raster_pipeline("fs_raster"),
            raster_reference: computes(device)
                .then(|| compute_pipeline(device, &edges_layout, raster_source, "cs_raster")),
            raster_centers: raster_pipeline("fs_centers"),
            raster_edges: if computes(device) {
                Edges::Compute(compute_pipeline(
                    device,
                    &edges_layout,
                    raster_source,
                    "cs_edges",
                ))
            } else {
                Edges::Fragment(raster_pipeline("fs_edges"))
            },
            finish: raster_pipeline("fs_finish"),
            present,
            raster_layout,
            edges_layout,
            present_layout,
            reference_uniforms: uniforms(),
            upscaled_uniforms: uniforms(),
            placeholder,
            reference: Target::with_storage(device, 1, RASTER_FORMAT, computes(device)),
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
        }
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
        let offsets = self.scene.pack(frame, memory);
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
        let uniforms = |scale: u32,
                        samples: u32,
                        textures: Textures,
                        keeps_under: bool|
         -> [u32; UNIFORM_WORDS] {
            let mut words = [0; UNIFORM_WORDS];
            words[..16].copy_from_slice(&[
                scale,
                registers.read(DISP3DCNT),
                registers.read(ALPHA_TEST_REF),
                samples,
                scene::pack_pixel(registers.rear_color()),
                registers.rear_depth(),
                scene::pack_attribute(registers.rear_attribute()),
                0,
                offsets.bins,
                offsets.vram,
                offsets.palette,
                TILES_X as u32,
                u32::from(textures == Textures::Smooth),
                u32::from(keeps_under),
                0,
                0,
            ]);
            for (word, pair) in words[16..20].iter_mut().zip(edge.as_chunks::<2>().0) {
                *word = u32::from(pair[0]) | u32::from(pair[1]) << 16;
            }
            words[20] = registers.fog_color();
            words[21] = registers.fog_offset();
            for (word, bytes) in words[24..].iter_mut().zip(fog.as_chunks::<4>().0) {
                *word = u32::from_le_bytes(*bytes);
            }
            words
        };
        let reference_textures = if scale > 1 {
            Textures::Nearest
        } else {
            upscale.textures
        };
        queue.write_buffer(
            &self.reference_uniforms,
            0,
            words_bytes(&uniforms(1, 1, reference_textures, scale == 1)),
        );
        let samples = upscale.antialiasing.samples();
        self.edges_only = scale > 1 && samples > 1;
        let stores = self.edges_only && matches!(self.raster_edges, Edges::Compute(_));
        queue.write_buffer(
            &self.upscaled_uniforms,
            0,
            words_bytes(&uniforms(scale, samples, upscale.textures, true)),
        );
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
        let raster_group = |uniforms: &wgpu::Buffer,
                            reference: &wgpu::TextureView,
                            centers: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("nds 3d raster"),
                layout: &self.raster_layout,
                entries: &group_entries(uniforms, &data_view, reference, centers),
            })
        };
        match &self.raster_reference {
            Some(raster) => {
                let mut entries = group_entries(
                    &self.reference_uniforms,
                    &data_view,
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
                    layout: &self.edges_layout,
                    entries: &entries,
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("nds 3d reference"),
                    timestamp_writes: end().map(|(query_set, index)| {
                        wgpu::ComputePassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: Some(0),
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
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(index),
                }),
            ),
        }
        if let Some(upscaled) = self.upscaled.as_ref().filter(|_| scale > 1) {
            let shaded = raster_group(
                &self.upscaled_uniforms,
                &self.reference.view,
                &self.placeholder,
            );
            match self.centers.as_ref().filter(|_| self.edges_only) {
                Some(centers) => {
                    draw(
                        &mut encoder,
                        &self.raster_centers,
                        &shaded,
                        &centers.view,
                        ended(end()),
                    );
                    match &self.raster_edges {
                        Edges::Fragment(edges) => draw(
                            &mut encoder,
                            edges,
                            &raster_group(
                                &self.upscaled_uniforms,
                                &self.reference.view,
                                &centers.view,
                            ),
                            &upscaled.view,
                            ended(end()),
                        ),
                        Edges::Compute(edges) => {
                            let mut entries = group_entries(
                                &self.upscaled_uniforms,
                                &data_view,
                                &self.reference.view,
                                &centers.view,
                            )
                            .to_vec();
                            entries.push(wgpu::BindGroupEntry {
                                binding: 4,
                                resource: wgpu::BindingResource::TextureView(&upscaled.view),
                            });
                            let edges_group =
                                device.create_bind_group(&wgpu::BindGroupDescriptor {
                                    label: Some("nds 3d edges"),
                                    layout: &self.edges_layout,
                                    entries: &entries,
                                });
                            let mut pass =
                                encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                                    label: Some("nds 3d edges"),
                                    timestamp_writes: end().map(|(query_set, index)| {
                                        wgpu::ComputePassTimestampWrites {
                                            query_set,
                                            beginning_of_pass_write_index: None,
                                            end_of_pass_write_index: Some(index),
                                        }
                                    }),
                                });
                            pass.set_pipeline(edges);
                            pass.set_bind_group(0, &edges_group, &[]);
                            pass.dispatch_workgroups(
                                (WIDTH as u32 * scale).div_ceil(EDGE_GROUP),
                                (HEIGHT as u32 * scale).div_ceil(EDGE_GROUP),
                                1,
                            );
                        }
                    }
                }
                None => draw(
                    &mut encoder,
                    &self.raster,
                    &shaded,
                    &upscaled.view,
                    ended(end()),
                ),
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
        let mut passes = vec![Pass::Reference];
        if self.upscaled.is_some() && self.scale > 1 {
            match self.centers.as_ref().filter(|_| self.edges_only) {
                Some(_) => passes.extend([Pass::Centers, Pass::Edges]),
                None => passes.push(Pass::Upscaled),
            }
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
    fragment: &str,
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
            entry_point: Some("vs_main"),
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
) -> [wgpu::BindGroupEntry<'a>; 4] {
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
    ]
}

const EDGE_STORAGE: u32 = 12 * 1024;

enum Edges {
    Fragment(wgpu::RenderPipeline),
    Compute(wgpu::ComputePipeline),
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
        compilation_options: wgpu::PipelineCompilationOptions::default(),
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
