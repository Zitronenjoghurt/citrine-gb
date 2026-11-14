use super::Cart;
use super::textures::{canvas, load, zoom_label};
use crate::utils::screenshot::encode_png;
use citrine_gb_nds_files::{Depth, FileType, Ncgr, Nclr, NodeId, Nscr, Picture};
use egui::{Align, ComboBox, Layout, Rect, RichText, Sense, Stroke, TextureHandle, Ui, vec2};
use std::sync::Arc;

const SIDE_WIDTH: f32 = 190.0;
const NARROW: f32 = 420.0;
const SWATCH: f32 = 10.0;
const MAX_WIDTH_TILES: usize = 64;

struct Choice {
    nodes: Vec<NodeId>,
    selected: usize,
    bytes: Option<Arc<[u8]>>,
}

impl Choice {
    fn new(cart: &Cart, node: NodeId, kind: FileType) -> Self {
        let mut choice = Self {
            nodes: cart.tree().nearest(node, kind),
            selected: 0,
            bytes: None,
        };
        choice.load(cart);
        choice
    }

    fn load(&mut self, cart: &Cart) {
        self.bytes = self
            .nodes
            .get(self.selected)
            .and_then(|&node| cart.file(node));
    }

    fn pick(&mut self, ui: &mut Ui, cart: &Cart, id: (&str, NodeId), empty: &str) -> bool {
        let tree = cart.tree();
        let label = |node: NodeId| tree.node(node).name().to_owned();
        let shown = self
            .nodes
            .get(self.selected)
            .map_or_else(|| empty.to_owned(), |&node| label(node));
        let mut selected = self.selected;
        ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(120.0)
            .show_ui(ui, |ui| {
                for (index, &node) in self.nodes.iter().enumerate() {
                    ui.selectable_value(&mut selected, index, label(node));
                }
            });
        if selected != self.selected {
            self.selected = selected;
            self.load(cart);
            return true;
        }
        false
    }
}

pub struct TileView {
    screen: bool,
    palettes: Choice,
    graphics: Option<Choice>,
    row: usize,
    width: usize,
    zoom: Option<f32>,
    image: Option<TextureHandle>,
    facts: String,
    below: f32,
}

impl TileView {
    pub fn new(cart: &Cart, node: NodeId, kind: Option<FileType>, bytes: &[u8]) -> Option<Self> {
        let screen = match kind {
            Some(FileType::Ncgr) => {
                Ncgr::parse(bytes).ok()?;
                false
            }
            Some(FileType::Nscr) => {
                Nscr::parse(bytes).ok()?;
                true
            }
            _ => return None,
        };
        let width = if screen {
            1
        } else {
            Ncgr::parse(bytes).ok()?.default_width()
        };
        Some(Self {
            screen,
            palettes: Choice::new(cart, node, FileType::Nclr),
            graphics: screen.then(|| Choice::new(cart, node, FileType::Ncgr)),
            row: 0,
            width,
            zoom: None,
            image: None,
            facts: String::new(),
            below: 0.0,
        })
    }

    pub fn picture(&self, bytes: &[u8]) -> Option<Picture> {
        let greys = grey_ramp();
        let palette = match &self.palettes.bytes {
            Some(file) => Nclr::parse(file).ok()?,
            None => Nclr {
                depth: Depth::Bpp8,
                colors: &greys,
            },
        };
        if self.screen {
            let graphic = self.graphics.as_ref()?.bytes.as_ref()?;
            Some(
                Nscr::parse(bytes)
                    .ok()?
                    .decode(&Ncgr::parse(graphic).ok()?, &palette),
            )
        } else {
            let tiles = Ncgr::parse(bytes).ok()?;
            let row = if self.palettes.bytes.is_none() && tiles.depth == Depth::Bpp4 {
                0
            } else {
                self.row
            };
            Some(tiles.decode(&palette, row, self.width))
        }
    }

    pub fn png(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let picture = self.picture(bytes)?;
        encode_png(picture.width, picture.height, &picture.rgba)
    }

    fn refresh(&mut self, ui: &Ui, bytes: &[u8], node: NodeId) {
        self.image = self.picture(bytes).map(|picture| {
            let depth = match &self.palettes.bytes {
                Some(file) => Nclr::parse(file).map_or(16, |p| p.depth.colors()),
                None => 16,
            };
            self.facts = format!("{} × {} · {depth} colours", picture.width, picture.height);
            load(
                ui,
                format!("ds_tiles_{node}"),
                picture.width,
                picture.height,
                &picture.rgba,
            )
        });
        if self.image.is_none() {
            self.facts.clear();
        }
    }
}

fn grey_ramp() -> Vec<u8> {
    (0..256u16)
        .flat_map(|i| {
            let level = (i % 16) * 31 / 15;
            (level | (level << 5) | (level << 10)).to_le_bytes()
        })
        .collect()
}

pub fn show(view: &mut TileView, cart: &Cart, node: NodeId, bytes: &[u8], ui: &mut Ui) {
    if view.image.is_none() && view.facts.is_empty() {
        view.refresh(ui, bytes, node);
    }
    let narrow = ui.available_width() < NARROW;
    let line = ui.spacing().interact_size.y + 8.0;
    let below = if narrow {
        view.below.max(line * 3.0)
    } else {
        line
    };
    let height = (ui.available_height() - below).max(0.0);
    let mut changed = false;
    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
        ui.horizontal_top(|ui| {
            let side = if narrow { 0.0 } else { SIDE_WIDTH };
            ui.allocate_ui(
                vec2(ui.available_width() - side, height),
                |ui| match &view.image {
                    Some(image) => canvas(ui, image, &mut view.zoom),
                    None => {
                        ui.centered_and_justified(|ui| {
                            ui.weak(if view.screen {
                                "No tile file in this folder"
                            } else {
                                "These tiles can't be read"
                            });
                        });
                    }
                },
            );
            if !narrow {
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    changed |= controls(view, cart, node, ui, false);
                });
            }
        });
    });
    let top = ui.cursor().top();
    if narrow {
        changed |= controls(view, cart, node, ui, true);
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            zoom_label(ui, view.zoom);
            ui.add(egui::Label::new(RichText::new(&view.facts).weak()).truncate());
        });
    });
    if narrow {
        let used = ui.min_rect().bottom() - top + ui.spacing().item_spacing.y;
        if (used - view.below).abs() > 0.5 {
            view.below = used;
            ui.ctx().request_repaint();
        }
    }
    if changed {
        view.refresh(ui, bytes, node);
    }
}

fn controls(view: &mut TileView, cart: &Cart, node: NodeId, ui: &mut Ui, narrow: bool) -> bool {
    let mut changed = false;
    if let Some(graphics) = &mut view.graphics {
        ui.label(RichText::new("Tile file").weak());
        changed |= graphics.pick(ui, cart, ("ds_tiles_graphic", node), "None");
        ui.add_space(8.0);
    }
    ui.label(RichText::new("Palette file").weak());
    changed |= view
        .palettes
        .pick(ui, cart, ("ds_tiles_palette", node), "Greys");
    ui.add_space(8.0);
    if let Some(file) = view.palettes.bytes.clone()
        && let Ok(palette) = Nclr::parse(&file)
    {
        ui.label(RichText::new("Palette").weak());
        let pick_row = !view.screen && palette.depth == Depth::Bpp4;
        if let Some(row) = swatches(ui, &palette, pick_row.then_some(view.row), narrow)
            && row != view.row
        {
            view.row = row;
            changed = true;
        }
        ui.add_space(8.0);
    }
    if !view.screen {
        ui.label(RichText::new("Width in tiles").weak());
        ui.horizontal(|ui| {
            if ui.small_button("−").clicked() && view.width > 1 {
                view.width -= 1;
                changed = true;
            }
            ui.label(view.width.to_string());
            if ui.small_button("+").clicked() && view.width < MAX_WIDTH_TILES {
                view.width += 1;
                changed = true;
            }
        });
    }
    changed
}

fn swatches(ui: &mut Ui, palette: &Nclr, selected: Option<usize>, narrow: bool) -> Option<usize> {
    let mut clicked = None;
    let rows = palette.rows().min(16);
    let label_width = 18.0;
    let row_width = (label_width + SWATCH * 16.0 + 4.0).min(ui.available_width());
    let swatch = ((row_width - label_width - 4.0) / 16.0).min(SWATCH);
    egui::ScrollArea::vertical()
        .id_salt("ds_tiles_swatches")
        .max_height((SWATCH + 6.0) * if narrow { 4.0 } else { 16.0 })
        .show(ui, |ui| {
            for row in 0..rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(row_width, SWATCH + 6.0), Sense::click());
                let visuals = ui.visuals();
                if selected == Some(row) {
                    ui.painter().rect_stroke(
                        rect,
                        3.0,
                        Stroke::new(1.5_f32, visuals.selection.stroke.color),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.painter().text(
                    rect.left_center() + vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    row.to_string(),
                    egui::FontId::monospace(10.0),
                    visuals.weak_text_color(),
                );
                for column in 0..16 {
                    let [r, g, b, _] = palette.rgba(row * 16 + column);
                    let min = rect.min + vec2(label_width + column as f32 * swatch, 3.0);
                    ui.painter().rect_filled(
                        Rect::from_min_size(min, vec2(swatch - 1.0, SWATCH)),
                        1.0,
                        egui::Color32::from_rgb(r, g, b),
                    );
                }
                if selected.is_some() && response.clicked() {
                    clicked = Some(row);
                }
            }
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::g2d::{build_ncgr, build_nclr, build_nscr};
    use citrine_gb_nds_files::fixtures::{narc, nds};

    fn cart() -> Cart {
        let mut colors = vec![0u16; 32];
        colors[1] = 0x001F;
        colors[17] = 0x03E0;
        let mut tiles = vec![0u8; 64];
        tiles[0] = 0x01;
        let files = narc::build(&[
            build_nclr(true, &colors),
            build_ncgr(true, 2, &tiles),
            build_nscr(8, 8, &[0x1000]),
        ]);
        let bytes = nds::build(b"CPUE", &[("poketool/pokegra.narc", files)]);
        Cart::read("Test".into(), Arc::from(bytes), false).unwrap()
    }

    fn node(cart: &Cart, name: &str) -> NodeId {
        cart.tree()
            .find(&["poketool", "pokegra.narc", name])
            .unwrap()
    }

    #[test]
    fn tiles_pick_the_nearest_palette_and_its_row() {
        let cart = cart();
        let id = node(&cart, "1");
        let bytes = cart.file(id).unwrap();
        let mut view = TileView::new(&cart, id, Some(FileType::Ncgr), &bytes).unwrap();
        assert_eq!(view.width, 2);
        let picture = view.picture(&bytes).unwrap();
        assert_eq!((picture.width, picture.height), (16, 8));
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255]);
        view.row = 1;
        assert_eq!(&view.picture(&bytes).unwrap().rgba[..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn a_screen_uses_its_tiles_and_palette_bank() {
        let cart = cart();
        let id = node(&cart, "2");
        let bytes = cart.file(id).unwrap();
        let view = TileView::new(&cart, id, Some(FileType::Nscr), &bytes).unwrap();
        let picture = view.picture(&bytes).unwrap();
        assert_eq!((picture.width, picture.height), (8, 8));
        assert_eq!(&picture.rgba[..4], &[0, 255, 0, 255]);
        assert!(view.png(&bytes).is_some());
    }

    #[test]
    fn tiles_without_a_palette_show_in_greys() {
        let mut tiles = vec![0u8; 32];
        tiles[0] = 0x0F;
        let files = narc::build(&[build_ncgr(true, 1, &tiles)]);
        let bytes = nds::build(b"CPUE", &[("a.narc", files)]);
        let cart = Cart::read("Test".into(), Arc::from(bytes), false).unwrap();
        let id = cart.tree().find(&["a.narc", "0"]).unwrap();
        let file = cart.file(id).unwrap();
        let view = TileView::new(&cart, id, Some(FileType::Ncgr), &file).unwrap();
        assert_eq!(
            &view.picture(&file).unwrap().rgba[..4],
            &[255, 255, 255, 255]
        );
    }
}
