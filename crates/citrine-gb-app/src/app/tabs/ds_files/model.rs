use super::textures::{self, TextureSet, load, zoom_label};
use crate::utils::screenshot::encode_png;
use citrine_gb::nds::geometry::Geometry;
use citrine_gb::nds::geometry::command::{
    DIF_AMB, LIGHT_COLOR, LIGHT_VECTOR, MTX_IDENTITY, MTX_LOAD_4X4, MTX_MODE, MTX_MULT_3X3,
    MTX_MULT_4X3, MTX_RESTORE, MTX_SCALE, MTX_STORE, MTX_TRANS, PLTT_BASE, POLYGON_ATTR, SPE_EMI,
    SWAP_BUFFERS, TEXIMAGE_PARAM, VIEWPORT,
};
use citrine_gb::nds::render::texture::TextureMemory;
use citrine_gb::nds::render::{CLEAR_COLOR, CLEAR_DEPTH, DISP3DCNT, Registers, Renderer};
use citrine_gb::nds::video::{HEIGHT, Line3d, WIDTH};
use citrine_gb_nds_files::{Matrix, Model, Step, Tex0, TextureFormat, Vram, models};
use egui::{Align, ComboBox, Layout, Rect, RichText, Sense, TextureHandle, Ui, vec2};

const SIDE_WIDTH: f32 = 190.0;
const NARROW: f32 = 420.0;
const DISTANCE: f32 = 2.2;
const FILL: f32 = 0.85;
const MAX_FIT: f32 = 2.0;
const STACK_SLOTS: u8 = 31;
const TEXTURES_ON: u32 = 1;
const BLENDING_ON: u32 = 1 << 3;
const ALPHA_BITS: u32 = 0x1F << 16;
const START_YAW: f32 = 35.0;
const START_PITCH: f32 = 25.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fit: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: START_YAW,
            pitch: START_PITCH,
            distance: DISTANCE,
            fit: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switches {
    pub textures: bool,
    pub wireframe: bool,
}

struct Memory<'a>(&'a Vram);

impl TextureMemory for Memory<'_> {
    fn texture(&self, address: u32) -> u8 {
        self.0.texture[address as usize % self.0.texture.len()]
    }

    fn texture_palette(&self, address: u32) -> u8 {
        self.0.palette[address as usize % self.0.palette.len()]
    }
}

struct Gx(Geometry);

impl Gx {
    fn send(&mut self, command: u8, param: u32) {
        self.0.write(usize::from(command) << 2, param, !0);
        self.0.step(u32::MAX);
    }

    fn packed(&mut self, word: u32) {
        self.0.write(0, word, !0);
        self.0.step(u32::MAX);
    }

    fn all(&mut self, command: u8, params: &[f32]) {
        for &value in params {
            self.send(command, fixed(value));
        }
    }
}

fn fixed(value: f32) -> u32 {
    ((value * 4096.0).round() as i32) as u32
}

pub struct Rendered {
    pub rgba: Vec<u8>,
    pub polygons: usize,
}

pub fn render(model: &Model, tex0: Option<&Tex0>, camera: Camera, switches: Switches) -> Rendered {
    let mut gx = Gx(Geometry::default());
    gx.send(VIEWPORT, 0xBFFF_0000);
    let (near, far) = (0.1_f32, 64.0_f32);
    let (sx, sy) = (1.3, 1.3 * WIDTH as f32 / HEIGHT as f32);
    gx.send(MTX_MODE, 0);
    gx.all(
        MTX_LOAD_4X4,
        &[
            sx,
            0.0,
            0.0,
            0.0,
            0.0,
            sy,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) / (near - far),
            -1.0,
            0.0,
            0.0,
            2.0 * far * near / (near - far),
            0.0,
        ],
    );
    gx.send(MTX_MODE, 2);
    gx.send(MTX_IDENTITY, 0);
    let pack = |v: f32| (((v * 511.0).round() as i32) as u32) & 0x3FF;
    gx.send(
        LIGHT_VECTOR,
        pack(-0.4) | pack(-0.6) << 10 | pack(-0.7) << 20,
    );
    gx.send(LIGHT_COLOR, 0x7FFF);
    let [min, max] = model.bounds;
    let center: Vec<f32> = (0..3).map(|i| (min[i] + max[i]) / 2.0).collect();
    let radius = (0..3)
        .map(|i| (max[i] - min[i]).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(0.01)
        / 2.0;
    gx.all(MTX_TRANS, &[0.0, 0.0, -camera.distance]);
    gx.all(MTX_MULT_3X3, &rotate_x(camera.pitch));
    gx.all(MTX_MULT_3X3, &rotate_y(camera.yaw));
    let fit = camera.fit / radius;
    gx.all(MTX_SCALE, &[fit, fit, fit]);
    gx.all(MTX_TRANS, &[-center[0], -center[1], -center[2]]);
    for slot in 0..STACK_SLOTS {
        gx.send(MTX_STORE, u32::from(slot));
    }
    for step in &model.steps {
        match *step {
            Step::Load(slot) => gx.send(MTX_RESTORE, u32::from(slot)),
            Step::Store(slot) => gx.send(MTX_STORE, u32::from(slot)),
            Step::Object(index) => {
                if let Some(matrix) = model.objects.get(usize::from(index)) {
                    gx.all(MTX_MULT_4X3, &four_by_three(matrix));
                }
            }
            Step::ScaleUp => gx.all(MTX_SCALE, &[model.up_scale; 3]),
            Step::ScaleDown => gx.all(MTX_SCALE, &[model.down_scale; 3]),
            Step::Material(index) => {
                if let Some(material) = model.materials.get(usize::from(index)) {
                    bind(&mut gx, material, tex0, switches);
                }
            }
            Step::Draw(index) => {
                if let Some(piece) = model.pieces.get(usize::from(index)) {
                    for word in piece.as_chunks::<4>().0 {
                        gx.packed(u32::from_le_bytes(*word));
                    }
                }
            }
        }
    }
    gx.send(SWAP_BUFFERS, 0);
    gx.0.vblank();
    let frame = gx.0.with_frame(Clone::clone);
    let mut registers = Registers::default();
    registers.write(DISP3DCNT, TEXTURES_ON | BLENDING_ON, !0);
    registers.write(CLEAR_COLOR, 0, !0);
    registers.write(CLEAR_DEPTH, 0x7FFF, !0);
    let vram = tex0.map(Tex0::vram).unwrap_or_else(|| Vram {
        texture: vec![0; 8],
        palette: vec![0; 8],
    });
    let memory = Memory(&vram);
    let mut renderer = Renderer::default();
    renderer.start_frame(registers);
    let mut line = Line3d::default();
    let mut rgba = Vec::with_capacity(WIDTH * HEIGHT * 4);
    for y in 0..HEIGHT {
        renderer.render_line(y, &frame, &memory, &mut line);
        for x in 0..WIDTH {
            let pixel = renderer.pixel(x, y);
            let wide = |c: u8| (u16::from(c.min(63)) * 255 / 63) as u8;
            rgba.extend_from_slice(&[
                wide(pixel.r),
                wide(pixel.g),
                wide(pixel.b),
                (u16::from(pixel.alpha.min(31)) * 255 / 31) as u8,
            ]);
        }
    }
    Rendered {
        rgba,
        polygons: frame.polygons.len(),
    }
}

fn bind(
    gx: &mut Gx,
    material: &citrine_gb_nds_files::Material,
    tex0: Option<&Tex0>,
    switches: Switches,
) {
    gx.send(DIF_AMB, material.diffuse_ambient);
    gx.send(SPE_EMI, material.specular_emission);
    let mut attributes = material.polygon_attr;
    if switches.wireframe {
        attributes &= !ALPHA_BITS;
    }
    gx.send(POLYGON_ATTR, attributes);
    let texture = tex0
        .zip(material.texture.as_deref())
        .and_then(|(tex0, name)| {
            let index = tex0.texture_named(name)?;
            Some((tex0, index, tex0.texture_param(index)?))
        });
    match texture {
        Some((tex0, index, param)) if switches.textures => {
            let kept = material.teximage_param & 0xC00F_0000;
            gx.send(TEXIMAGE_PARAM, param | kept);
            let format = tex0.textures()[index].format;
            let palette = material
                .palette
                .as_deref()
                .and_then(|name| tex0.palette_named(name))
                .and_then(|palette| tex0.palette_base(palette, format));
            if format != TextureFormat::Direct {
                gx.send(PLTT_BASE, palette.unwrap_or(0));
            }
        }
        _ => gx.send(TEXIMAGE_PARAM, 0),
    }
}

fn four_by_three(m: &Matrix) -> [f32; 12] {
    [
        m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2], m[0][3],
        m[1][3], m[2][3],
    ]
}

fn rotate_x(degrees: f32) -> [f32; 9] {
    let (s, c) = degrees.to_radians().sin_cos();
    [1.0, 0.0, 0.0, 0.0, c, s, 0.0, -s, c]
}

fn rotate_y(degrees: f32) -> [f32; 9] {
    let (s, c) = degrees.to_radians().sin_cos();
    [c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c]
}

pub struct ModelView {
    names: Vec<String>,
    selected: usize,
    camera: Camera,
    switches: Switches,
    zoom: Option<f32>,
    image: Option<TextureHandle>,
    polygons: usize,
    textures: Option<TextureSet>,
}

impl ModelView {
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let names: Vec<String> = models(bytes).ok()?.into_iter().map(|m| m.name).collect();
        if names.is_empty() {
            return None;
        }
        let mut view = Self {
            names,
            selected: 0,
            camera: Camera::default(),
            switches: Switches {
                textures: true,
                wireframe: false,
            },
            zoom: None,
            image: None,
            polygons: 0,
            textures: TextureSet::read(bytes),
        };
        view.fit(bytes);
        Some(view)
    }

    fn fit(&mut self, bytes: &[u8]) {
        self.camera.fit = 1.0;
        if let Some(rendered) = self.rendered(bytes) {
            self.camera.fit = fit(&rendered.rgba);
        }
    }

    pub fn textures(&self) -> Option<&TextureSet> {
        self.textures.as_ref()
    }

    pub fn rendered(&self, bytes: &[u8]) -> Option<Rendered> {
        let list = models(bytes).ok()?;
        let model = list.get(self.selected)?;
        let tex0 = Tex0::find(bytes).ok();
        Some(render(model, tex0.as_ref(), self.camera, self.switches))
    }

    pub fn png(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let rendered = self.rendered(bytes)?;
        encode_png(WIDTH, HEIGHT, &rendered.rgba)
    }

    fn refresh(&mut self, ui: &Ui, bytes: &[u8], id: impl std::fmt::Debug) {
        let rendered = self.rendered(bytes);
        self.polygons = rendered.as_ref().map_or(0, |r| r.polygons);
        self.image = rendered.map(|r| load(ui, format!("ds_model_{id:?}"), WIDTH, HEIGHT, &r.rgba));
    }
}

pub fn show(
    view: &mut ModelView,
    bytes: &[u8],
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
) {
    if view.image.is_none() {
        view.refresh(ui, bytes, id);
    }
    let narrow = ui.available_width() < NARROW;
    let line = ui.spacing().interact_size.y + 8.0;
    let height = (ui.available_height() - line).max(0.0);
    let before = (view.camera, view.switches, view.selected);
    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
        ui.horizontal_top(|ui| {
            let side = if narrow { 0.0 } else { SIDE_WIDTH };
            ui.allocate_ui(vec2(ui.available_width() - side, height), |ui| {
                stage(view, ui);
            });
            if !narrow {
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    if let Some(set) = &mut view.textures {
                        ui.label(RichText::new("Textures").weak());
                        let list_height = (height - 90.0).max(60.0);
                        textures::list(set, bytes, ui, ("ds_model_textures", id), list_height);
                        ui.add_space(8.0);
                    }
                    switches(view, ui);
                });
            }
        });
    });
    ui.horizontal(|ui| {
        if view.names.len() > 1 {
            ComboBox::from_id_salt(("ds_model_pick", id))
                .selected_text(&view.names[view.selected])
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (index, name) in view.names.iter().enumerate() {
                        ui.selectable_value(&mut view.selected, index, name);
                    }
                });
        }
        if narrow {
            switches(view, ui);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            zoom_label(ui, view.zoom);
            let textures = view.textures.as_ref().map_or(0, TextureSet::len);
            ui.add(
                egui::Label::new(RichText::new(facts(view.polygons, textures)).weak()).truncate(),
            );
        });
    });
    if before.2 != view.selected {
        view.fit(bytes);
    }
    if before != (view.camera, view.switches, view.selected) {
        view.refresh(ui, bytes, id);
    }
}

fn fit(rgba: &[u8]) -> f32 {
    let (cx, cy) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
    let (mut dx, mut dy) = (0.0_f32, 0.0_f32);
    for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = (index % WIDTH) as f32 + 0.5;
        let y = (index / WIDTH) as f32 + 0.5;
        dx = dx.max((x - cx).abs());
        dy = dy.max((y - cy).abs());
    }
    if dx == 0.0 && dy == 0.0 {
        return 1.0;
    }
    (FILL * cx / dx.max(1.0))
        .min(FILL * cy / dy.max(1.0))
        .clamp(1.0, MAX_FIT)
}

fn facts(polygons: usize, textures: usize) -> String {
    let polygons = match polygons {
        1 => "1 polygon".to_owned(),
        n => format!("{n} polygons"),
    };
    match textures {
        0 => polygons,
        1 => format!("{polygons} · 1 texture"),
        n => format!("{polygons} · {n} textures"),
    }
}

fn switches(view: &mut ModelView, ui: &mut Ui) {
    if view.textures.is_some() {
        ui.checkbox(&mut view.switches.textures, "Textures");
    }
    ui.checkbox(&mut view.switches.wireframe, "Wireframe");
}

fn stage(view: &mut ModelView, ui: &mut Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let Some(image) = &view.image else {
        return;
    };
    if response.dragged() {
        let delta = response.drag_delta();
        view.camera.yaw = (view.camera.yaw + delta.x * 0.5) % 360.0;
        view.camera.pitch = (view.camera.pitch + delta.y * 0.5).clamp(-89.0, 89.0);
    }
    if response.hovered() {
        let delta = ui.input(|i| i.zoom_delta());
        if delta != 1.0 {
            view.camera.distance = (view.camera.distance / delta).clamp(1.2, 20.0);
            view.zoom = Some(DISTANCE / view.camera.distance);
        }
    }
    if response.double_clicked() {
        view.camera = Camera {
            fit: view.camera.fit,
            ..Camera::default()
        };
        view.zoom = None;
    }
    let room = (rect.width() / WIDTH as f32).min(rect.height() / HEIGHT as f32);
    let scale = if room >= 1.0 {
        room.floor()
    } else {
        room.max(0.25)
    };
    let shown = Rect::from_center_size(rect.center(), image.size_vec2() * scale);
    ui.painter_at(rect).image(
        image.id(),
        shown,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(if response.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(x: std::ops::Range<usize>, y: std::ops::Range<usize>) -> Vec<u8> {
        let mut rgba = vec![0; WIDTH * HEIGHT * 4];
        for row in y {
            for column in x.clone() {
                rgba[(row * WIDTH + column) * 4 + 3] = 255;
            }
        }
        rgba
    }

    #[test]
    fn a_small_model_is_scaled_up_to_fill_the_frame() {
        let half = WIDTH / 8;
        let rgba = block(
            WIDTH / 2 - half..WIDTH / 2 + half,
            HEIGHT / 2 - 4..HEIGHT / 2 + 4,
        );
        assert!((fit(&rgba) - MAX_FIT).abs() < 1e-3);
        let wide = block(
            WIDTH / 2 - 80..WIDTH / 2 + 80,
            HEIGHT / 2 - 4..HEIGHT / 2 + 4,
        );
        assert!((fit(&wide) - FILL * 128.0 / 79.5).abs() < 1e-3);
    }

    #[test]
    fn an_empty_or_full_render_keeps_its_size() {
        assert_eq!(fit(&vec![0; WIDTH * HEIGHT * 4]), 1.0);
        assert_eq!(fit(&vec![255; WIDTH * HEIGHT * 4]), 1.0);
    }

    #[test]
    fn facts_count_polygons_and_textures() {
        assert_eq!(facts(1, 0), "1 polygon");
        assert_eq!(facts(186, 4), "186 polygons · 4 textures");
        assert_eq!(facts(2, 1), "2 polygons · 1 texture");
    }

    #[test]
    fn object_matrices_go_out_column_by_column() {
        let mut m = citrine_gb_nds_files::IDENTITY;
        m[0][3] = 5.0;
        m[1][0] = 2.0;
        let out = four_by_three(&m);
        assert_eq!(out[1], 2.0);
        assert_eq!(out[9], 5.0);
    }

    #[test]
    fn a_triangle_renders_through_the_ds_engine() {
        let file = citrine_gb_nds_files::fixtures::mdl0::build_triangle(31 << 16 | 0xC0);
        let view = ModelView::read(&file).unwrap();
        let rendered = view.rendered(&file).unwrap();
        assert_eq!(rendered.polygons, 1);
        let at = (HEIGHT / 2 * WIDTH + WIDTH / 2) * 4;
        assert_eq!(&rendered.rgba[at..at + 4], &[255, 255, 255, 255]);
        assert_eq!(&rendered.rgba[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn wireframe_keeps_the_polygon_but_not_its_inside() {
        let file = citrine_gb_nds_files::fixtures::mdl0::build_triangle(31 << 16 | 0xC0);
        let mut view = ModelView::read(&file).unwrap();
        view.camera.yaw = 0.0;
        view.camera.pitch = 0.0;
        view.switches.wireframe = true;
        let rendered = view.rendered(&file).unwrap();
        assert_eq!(rendered.polygons, 1);
        let at = (HEIGHT / 2 * WIDTH + WIDTH / 2) * 4;
        assert_eq!(rendered.rgba[at + 3], 0);
        assert!(rendered.rgba.chunks(4).any(|p| p[3] == 255));
    }
}
