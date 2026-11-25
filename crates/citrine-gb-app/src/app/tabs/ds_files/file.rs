#[cfg(feature = "nds")]
use super::model::{self, ModelView};
#[cfg(feature = "nds")]
use super::sound::{self, SoundView};
use super::textures::{self, TextureSet};
use super::tiles::{self, TileView};
use super::{Cart, OpenFile};
use crate::app::widgets::hex_view::{HexView, parse_offset};
use crate::emulator::Emulator;
use crate::icons;
use crate::utils::file_channels::FileChannels;
use crate::utils::file_saver::FileSaver;
use citrine_gb_nds_files::{FileType, NodeId};
use egui::{Align, Layout, RichText, Ui};

pub struct FileView {
    hex: bool,
    text: Option<String>,
    offset: String,
    jump: Option<usize>,
    textures: Option<TextureSet>,
    tiles: Option<TileView>,
    #[cfg(feature = "nds")]
    model: Option<ModelView>,
    #[cfg(feature = "nds")]
    sound: Option<SoundView>,
}

impl FileView {
    pub fn new(cart: &Cart, node: NodeId, kind: Option<FileType>, bytes: &[u8]) -> Self {
        let text =
            (kind == Some(FileType::Text)).then(|| String::from_utf8_lossy(bytes).into_owned());
        #[cfg(feature = "nds")]
        let model = (kind == Some(FileType::Nsbmd))
            .then(|| ModelView::read(bytes))
            .flatten();
        #[cfg(feature = "nds")]
        let sound = SoundView::read(cart, node, kind, bytes);
        #[cfg(feature = "nds")]
        let has_model = model.is_some() || sound.is_some();
        #[cfg(not(feature = "nds"))]
        let has_model = false;
        let textures = (!has_model && matches!(kind, Some(FileType::Nsbtx | FileType::Nsbmd)))
            .then(|| TextureSet::read(bytes))
            .flatten();
        let tiles = TileView::new(cart, node, kind, bytes);
        Self {
            hex: text.is_none() && textures.is_none() && tiles.is_none() && !has_model,
            text,
            offset: String::new(),
            jump: None,
            textures,
            tiles,
            #[cfg(feature = "nds")]
            model,
            #[cfg(feature = "nds")]
            sound,
        }
    }

    #[cfg(feature = "nds")]
    pub fn stop(&mut self, emulator: &mut Emulator) {
        if let Some(sound) = &mut self.sound {
            sound.stop(emulator);
        }
    }

    #[cfg(not(feature = "nds"))]
    pub fn stop(&mut self, _: &mut Emulator) {}

    fn has_preview(&self) -> bool {
        self.text.is_some() || self.textures.is_some() || self.tiles.is_some() || self.has_model()
    }

    #[cfg(feature = "nds")]
    fn has_model(&self) -> bool {
        self.model.is_some() || self.sound.is_some()
    }

    #[cfg(not(feature = "nds"))]
    fn has_model(&self) -> bool {
        false
    }

    fn picture_set(&self) -> Option<&TextureSet> {
        #[cfg(feature = "nds")]
        if let Some(model) = &self.model {
            return model.textures();
        }
        self.textures.as_ref()
    }

    fn picture(&self, title: &str, bytes: &[u8]) -> Option<(String, Option<Vec<u8>>)> {
        #[cfg(feature = "nds")]
        if let Some(model) = &self.model {
            return Some((format!("{title}.png"), model.png(bytes)));
        }
        if let Some(view) = &self.tiles {
            return Some((format!("{title}.png"), view.png(bytes)));
        }
        let set = self.textures.as_ref()?;
        Some((
            format!("{}.png", set.selected().name),
            set.png(bytes, set.selected_index()),
        ))
    }
}

pub(super) fn show(
    open: &mut OpenFile,
    ui: &mut Ui,
    phone: bool,
    files: &mut FileChannels,
    emulator: &mut Emulator,
) {
    header(open, ui, phone, files);
    ui.separator();
    if open.view.hex {
        hex(open, ui);
    } else if show_sound(&mut open.view, &open.bytes, ui, emulator, open.node)
        || show_model(&mut open.view, &open.bytes, ui, open.node)
    {
    } else if let Some(set) = &mut open.view.textures {
        textures::show(set, &open.bytes, ui, open.node);
    } else if let Some(view) = &mut open.view.tiles {
        tiles::show(view, &open.cart, open.node, &open.bytes, ui);
    } else if let Some(text) = &open.view.text {
        egui::ScrollArea::both()
            .id_salt(("ds_file_text", open.node))
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).monospace()).selectable(true));
            });
    }
}

#[cfg(feature = "nds")]
fn show_model(view: &mut FileView, bytes: &[u8], ui: &mut Ui, node: NodeId) -> bool {
    let Some(view) = &mut view.model else {
        return false;
    };
    model::show(view, bytes, ui, node);
    true
}

#[cfg(feature = "nds")]
fn show_sound(
    view: &mut FileView,
    bytes: &[u8],
    ui: &mut Ui,
    emulator: &mut Emulator,
    node: NodeId,
) -> bool {
    let Some(view) = &mut view.sound else {
        return false;
    };
    sound::show(view, bytes, ui, emulator, node);
    true
}

#[cfg(not(feature = "nds"))]
fn show_sound(_: &mut FileView, _: &[u8], _: &mut Ui, _: &mut Emulator, _: NodeId) -> bool {
    false
}

#[cfg(not(feature = "nds"))]
fn show_model(_: &mut FileView, _: &[u8], _: &mut Ui, _: NodeId) -> bool {
    false
}

fn header(open: &mut OpenFile, ui: &mut Ui, phone: bool, files: &mut FileChannels) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            export(open, ui, files);
            if open.view.has_preview() {
                if ui
                    .add(egui::Button::selectable(open.view.hex, "Hex"))
                    .clicked()
                {
                    open.view.hex = true;
                }
                if ui
                    .add(egui::Button::selectable(!open.view.hex, "Preview"))
                    .clicked()
                {
                    open.view.hex = false;
                }
            }
            if !phone {
                let path = open.cart.tree.path(open.node).join(" › ");
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(path).size(13.0).weak()).truncate());
                });
            }
        });
    });
}

fn export(open: &OpenFile, ui: &mut Ui, files: &mut FileChannels) {
    ui.menu_button(format!("{}  Export", icons::DOWNLOAD_SIMPLE), |ui| {
        let name = file_name(open);
        let extension = name
            .rsplit_once('.')
            .map_or("bin", |(_, ext)| ext)
            .to_owned();
        let label = format!("{}  File", icons::FILE);
        if ui.button(label).on_hover_text(&name).clicked() {
            FileSaver::new(&name)
                .add_filter("DS file", &[extension.as_str()])
                .dispatch(&files.spawner, open.bytes.to_vec(), files.save_tx.clone());
            ui.close();
        }
        let title = open.title().replace('/', "_");
        if let Some((png_name, png)) = open.view.picture(&title, &open.bytes) {
            let picture = format!("{}  This picture", icons::IMAGE);
            if ui.button(picture).on_hover_text(&png_name).clicked() {
                if let Some(png) = png {
                    FileSaver::new(&png_name)
                        .add_filter("PNG picture", &["png"])
                        .dispatch(&files.spawner, png, files.save_tx.clone());
                }
                ui.close();
            }
        }
        if let Some(set) = open.view.picture_set()
            && set.len() > 1
        {
            let stem = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem);
            let all = format!("{}  All {} pictures", icons::STACK, set.len());
            let zip_name = format!("{stem}.zip");
            if ui.button(all).on_hover_text(&zip_name).clicked() {
                if let Some(zip) = set.zip(&open.bytes) {
                    FileSaver::new(&zip_name)
                        .add_filter("Zip archive", &["zip"])
                        .dispatch(&files.spawner, zip, files.save_tx.clone());
                }
                ui.close();
            }
        }
    });
}

pub fn file_name(open: &OpenFile) -> String {
    let tree = &open.cart.tree;
    let node = tree.node(open.node);
    if node.cart_id().is_some() {
        return node.name().to_owned();
    }
    let extension = match node.file_type() {
        Some(FileType::Data) | None => "bin".to_owned(),
        Some(FileType::Text) => "txt".to_owned(),
        Some(kind) => kind.label().to_lowercase(),
    };
    format!("{}.{extension}", open.title().replace('/', "_"))
}

fn hex(open: &mut OpenFile, ui: &mut Ui) {
    let footer = ui.spacing().interact_size.y + 8.0;
    let jump = open.view.jump.take();
    let size = open.bytes.len();
    ui.allocate_ui(
        egui::vec2(
            ui.available_width(),
            (ui.available_height() - footer).max(0.0),
        ),
        |ui| {
            HexView::new(
                ("ds_file_hex", open.node, open.cart.bytes.len()),
                &open.bytes,
            )
            .jump(jump)
            .show(ui);
        },
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new("Offset").weak());
        let field = ui.add(
            egui::TextEdit::singleline(&mut open.view.offset)
                .hint_text("00000000")
                .font(egui::TextStyle::Monospace)
                .desired_width(90.0),
        );
        if field.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(offset) = parse_offset(&open.view.offset)
        {
            open.view.jump = Some(offset.min(size.saturating_sub(1)));
        }
        let magic: String = crate::app::widgets::hex_view::ascii(&open.bytes[..size.min(4)]);
        let facts = if magic.chars().all(|c| c.is_ascii_alphanumeric()) && size >= 4 {
            format!("Magic {magic} · {} bytes", super::explorer::thousands(size))
        } else {
            format!("{} bytes", super::explorer::thousands(size))
        };
        ui.label(RichText::new(facts).weak());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tabs::ds_files::{Cart, DsFilesUi};
    use citrine_gb_nds_files::fixtures::{narc, nds, tex0};
    use std::sync::Arc;

    fn state() -> DsFilesUi {
        let set = tex0::build(
            &[tex0::FixtureTexture {
                name: "roof",
                format: 3,
                width_shift: 0,
                height_shift: 0,
                data: &[0x10; 32],
            }],
            &[("roof_pl", &[0x1F, 0x00, 0xE0, 0x03])],
        );
        let textures = narc::build(&[b"BTX0\xFF\xFE".to_vec(), vec![1, 2, 3], set]);
        let bytes = nds::build(
            b"CPUE",
            &[
                ("fielddata/map_tex_set.narc", textures),
                ("data/intro.txt", b"Hello\n".to_vec()),
            ],
        );
        let mut state = DsFilesUi::default();
        state.add_cart(Cart::read("Test".into(), Arc::from(bytes), false).unwrap());
        state
    }

    fn open(state: &mut DsFilesUi, path: &[&str]) -> u32 {
        let node = state.cart().unwrap().tree().find(path).unwrap();
        state.open(node).unwrap()
    }

    #[test]
    fn text_opens_as_text_and_data_as_hex() {
        let mut state = state();
        let text = open(&mut state, &["data", "intro.txt"]);
        let data = open(&mut state, &["fielddata", "map_tex_set.narc", "1"]);
        let text = state.file(text).unwrap();
        assert!(!text.view.hex);
        assert_eq!(text.view.text.as_deref(), Some("Hello\n"));
        assert!(state.file(data).unwrap().view.hex);
        assert!(!state.file(data).unwrap().view.has_preview());
    }

    #[test]
    fn exported_names_say_where_the_file_came_from() {
        let mut state = state();
        let text = open(&mut state, &["data", "intro.txt"]);
        let texture = open(&mut state, &["fielddata", "map_tex_set.narc", "0"]);
        let data = open(&mut state, &["fielddata", "map_tex_set.narc", "1"]);
        assert_eq!(file_name(state.file(text).unwrap()), "intro.txt");
        assert_eq!(
            file_name(state.file(texture).unwrap()),
            "map_tex_set_0.nsbtx"
        );
        assert_eq!(file_name(state.file(data).unwrap()), "map_tex_set_1.bin");
    }

    #[test]
    fn a_texture_set_opens_on_its_pictures_and_a_broken_one_on_hex() {
        let mut state = state();
        let set = open(&mut state, &["fielddata", "map_tex_set.narc", "2"]);
        let broken = open(&mut state, &["fielddata", "map_tex_set.narc", "0"]);
        let set = state.file(set).unwrap();
        assert!(!set.view.hex);
        assert_eq!(set.view.textures.as_ref().unwrap().selected().name, "roof");
        assert!(state.file(broken).unwrap().view.hex);
    }
}
