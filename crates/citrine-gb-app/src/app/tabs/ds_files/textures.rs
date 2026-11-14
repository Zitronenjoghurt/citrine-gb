use crate::app::widgets::video::checker;
use crate::utils::screenshot::encode_png;
use citrine_gb_nds_files::Tex0;
use egui::{
    Align, ColorImage, ComboBox, Layout, Rect, RichText, Sense, TextureHandle, TextureOptions, Ui,
    vec2,
};
use std::collections::HashMap;
use std::io::Write;

const LIST_WIDTH: f32 = 180.0;
const THUMB: f32 = 28.0;
const NARROW: f32 = 420.0;
const MAX_ZOOM: f32 = 32.0;
const MIN_ZOOM: f32 = 0.25;

pub struct Picture {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub facts: String,
    pub palette: Option<usize>,
}

pub struct TextureSet {
    pictures: Vec<Picture>,
    palettes: Vec<String>,
    selected: usize,
    zoom: Option<f32>,
    images: HashMap<(usize, Option<usize>), TextureHandle>,
}

impl TextureSet {
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let tex0 = Tex0::find(bytes).ok()?;
        let pictures: Vec<_> = tex0
            .textures()
            .iter()
            .enumerate()
            .map(|(index, info)| Picture {
                name: info.name.clone(),
                width: info.width,
                height: info.height,
                facts: format!("{} × {} · {}", info.width, info.height, info.format.label()),
                palette: tex0.palette_for(index),
            })
            .collect();
        if pictures.is_empty() {
            return None;
        }
        Some(Self {
            pictures,
            palettes: tex0.palettes().iter().map(|p| p.name.clone()).collect(),
            selected: 0,
            zoom: None,
            images: HashMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.pictures.len()
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected(&self) -> &Picture {
        &self.pictures[self.selected]
    }

    pub fn select(&mut self, index: usize) {
        if index < self.pictures.len() {
            self.selected = index;
            self.zoom = None;
        }
    }

    pub fn png(&self, bytes: &[u8], index: usize) -> Option<Vec<u8>> {
        let picture = self.pictures.get(index)?;
        let rgba = Tex0::find(bytes).ok()?.decode(index, picture.palette)?;
        encode_png(picture.width, picture.height, &rgba)
    }

    pub fn zip(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut used = HashMap::<String, usize>::new();
        for (index, picture) in self.pictures.iter().enumerate() {
            let png = self.png(bytes, index)?;
            let count = used.entry(picture.name.clone()).or_default();
            *count += 1;
            let name = if *count == 1 {
                format!("{}.png", picture.name)
            } else {
                format!("{} ({count}).png", picture.name)
            };
            zip.start_file(name, options).ok()?;
            zip.write_all(&png).ok()?;
        }
        zip.finish().ok().map(std::io::Cursor::into_inner)
    }

    fn image(&mut self, ui: &Ui, bytes: &[u8], index: usize) -> Option<TextureHandle> {
        let picture = &self.pictures[index];
        let key = (index, picture.palette);
        if let Some(handle) = self.images.get(&key) {
            return Some(handle.clone());
        }
        let rgba = Tex0::find(bytes).ok()?.decode(index, picture.palette)?;
        let handle = load(
            ui,
            format!("ds_texture_{index}_{:?}", picture.palette),
            picture.width,
            picture.height,
            &rgba,
        );
        self.images.insert(key, handle.clone());
        Some(handle)
    }
}

pub fn show(set: &mut TextureSet, bytes: &[u8], ui: &mut Ui, id: impl std::hash::Hash + Copy) {
    let narrow = ui.available_width() < NARROW;
    let footer = ui.spacing().interact_size.y + 8.0;
    let height = (ui.available_height() - footer).max(0.0);
    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
        ui.horizontal_top(|ui| {
            if !narrow {
                list(set, bytes, ui, id, height);
                ui.separator();
            }
            picture(set, bytes, ui);
        });
    });
    ui.horizontal(|ui| footer_row(set, ui, id, narrow));
}

pub(super) fn list(
    set: &mut TextureSet,
    bytes: &[u8],
    ui: &mut Ui,
    id: impl std::hash::Hash,
    height: f32,
) {
    let row = THUMB + 6.0;
    let width = LIST_WIDTH.min(ui.available_width());
    ui.allocate_ui(vec2(width, height), |ui| {
        ui.set_width(width);
        egui::ScrollArea::vertical()
            .id_salt(("ds_textures_list", id))
            .auto_shrink(false)
            .show_rows(ui, row, set.len(), |ui, range| {
                for index in range {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::click());
                    let visuals = ui.visuals();
                    if index == set.selected {
                        ui.painter()
                            .rect_filled(rect, 4.0, visuals.selection.bg_fill);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
                    }
                    let thumb = Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(THUMB, THUMB));
                    if let Some(image) = set.image(ui, bytes, index) {
                        draw_fitted(ui, &image, thumb, 2.0);
                    }
                    let picture = &set.pictures[index];
                    let text_x = thumb.right() + 8.0;
                    let strong = ui.visuals().strong_text_color();
                    let weak = ui.visuals().weak_text_color();
                    ui.painter().text(
                        egui::pos2(text_x, rect.top() + 3.0),
                        egui::Align2::LEFT_TOP,
                        &picture.name,
                        egui::FontId::proportional(13.0),
                        strong,
                    );
                    ui.painter().text(
                        egui::pos2(text_x, rect.bottom() - 3.0),
                        egui::Align2::LEFT_BOTTOM,
                        &picture.facts,
                        egui::FontId::proportional(10.5),
                        weak,
                    );
                    if response.on_hover_text(&picture.name).clicked() {
                        set.select(index);
                    }
                }
            });
    });
}

fn picture(set: &mut TextureSet, bytes: &[u8], ui: &mut Ui) {
    let index = set.selected;
    if let Some(image) = set.image(ui, bytes, index) {
        canvas(ui, &image, &mut set.zoom);
    } else {
        ui.allocate_space(ui.available_size());
    }
}

pub(super) fn canvas(ui: &mut Ui, image: &TextureHandle, zoom: &mut Option<f32>) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
    let size = image.size_vec2();
    let room = ((rect.width() - 24.0) / size.x).min((rect.height() - 24.0) / size.y);
    let fit = if room >= 1.0 { room.floor() } else { room }.clamp(MIN_ZOOM, MAX_ZOOM);
    let mut shown_zoom = zoom.unwrap_or(fit);
    if response.hovered() {
        let delta = ui.input(|i| i.zoom_delta());
        if delta != 1.0 {
            shown_zoom = (shown_zoom * delta).clamp(MIN_ZOOM, MAX_ZOOM);
            *zoom = Some(shown_zoom);
        }
    }
    if response.double_clicked() {
        *zoom = None;
        shown_zoom = fit;
    }
    let shown = Rect::from_center_size(rect.center(), size * shown_zoom);
    let painter = ui.painter_at(rect);
    checker(&painter, shown.intersect(rect), 8.0);
    painter.image(
        image.id(),
        shown,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

pub(super) fn zoom_label(ui: &mut Ui, zoom: Option<f32>) {
    if let Some(zoom) = zoom {
        ui.label(RichText::new(format!("{:.0}%", zoom * 100.0)).weak())
            .on_hover_text("Double-click the picture to fit it");
    }
}

pub(super) fn load(
    ui: &Ui,
    name: String,
    width: usize,
    height: usize,
    rgba: &[u8],
) -> TextureHandle {
    let image = ColorImage::from_rgba_unmultiplied([width, height], rgba);
    ui.ctx().load_texture(name, image, TextureOptions::NEAREST)
}

fn draw_fitted(ui: &Ui, image: &TextureHandle, area: Rect, cell: f32) {
    let size = image.size_vec2();
    let scale = (area.width() / size.x).min(area.height() / size.y);
    let shown = Rect::from_center_size(area.center(), size * scale);
    checker(ui.painter(), shown, cell);
    ui.painter().image(
        image.id(),
        shown,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn footer_row(set: &mut TextureSet, ui: &mut Ui, id: impl std::hash::Hash + Copy, narrow: bool) {
    if narrow {
        let mut selected = set.selected;
        ui.label(RichText::new("Texture").weak());
        ComboBox::from_id_salt(("ds_texture_pick", id))
            .selected_text(&set.selected().name)
            .width(110.0)
            .show_ui(ui, |ui| {
                for (index, picture) in set.pictures.iter().enumerate() {
                    ui.selectable_value(&mut selected, index, &picture.name);
                }
            });
        if selected != set.selected {
            set.select(selected);
        }
    }
    let index = set.selected;
    if let Some(current) = set.pictures[index].palette {
        ui.label(RichText::new("Palette").weak());
        let mut chosen = current;
        ComboBox::from_id_salt(("ds_texture_palette", id))
            .selected_text(&set.palettes[current])
            .width(110.0)
            .show_ui(ui, |ui| {
                for (palette, name) in set.palettes.iter().enumerate() {
                    ui.selectable_value(&mut chosen, palette, name);
                }
            });
        set.pictures[index].palette = Some(chosen);
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        zoom_label(ui, set.zoom);
        if !narrow {
            ui.label(RichText::new(&set.selected().facts).weak());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::tex0::{FixtureTexture, build};

    fn set_bytes() -> Vec<u8> {
        build(
            &[
                FixtureTexture {
                    name: "roof",
                    format: 3,
                    width_shift: 0,
                    height_shift: 0,
                    data: &[0x10; 32],
                },
                FixtureTexture {
                    name: "roof",
                    format: 7,
                    width_shift: 0,
                    height_shift: 0,
                    data: &[0x00, 0xFC].repeat(64),
                },
            ],
            &[("roof_pl", &[0x1F, 0x00, 0xE0, 0x03])],
        )
    }

    #[test]
    fn reads_pictures_with_their_facts_and_paired_palette() {
        let set = TextureSet::read(&set_bytes()).unwrap();
        assert_eq!(set.len(), 2);
        assert_eq!(set.selected().facts, "8 × 8 · 16 colours");
        assert_eq!(set.selected().palette, Some(0));
        assert_eq!(set.pictures[1].palette, None);
    }

    #[test]
    fn a_file_without_textures_has_no_set() {
        assert!(TextureSet::read(b"BTX0\xFF\xFE").is_none());
    }

    #[test]
    fn exports_one_png_and_a_zip_with_unique_names() {
        let bytes = set_bytes();
        let set = TextureSet::read(&bytes).unwrap();
        let png = set.png(&bytes, 0).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let zip = set.zip(&bytes).unwrap();
        let archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|n| n.unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["roof (2).png", "roof.png"]);
    }
}
