use crate::app::ui_state::hotkeys::Hotkeys;
use crate::app::ui_state::keybinds::KeyBindings;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::tab_bar::tab_bar;
use egui::{Response, Ui, Widget};
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

mod changelog;
mod features;
#[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
mod licenses;
mod welcome;

#[derive(
    Debug, Default, Copy, Clone, PartialEq, serde::Deserialize, serde::Serialize, EnumIter,
)]
pub enum InfoTab {
    #[default]
    Welcome,
    Features,
    Changelog,
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    Licenses,
}

pub enum Controls<'a> {
    Touch,
    Desktop {
        bindings: &'a KeyBindings,
        hotkeys: &'a Hotkeys,
    },
}

impl InfoTab {
    pub fn build_md(&self, controls: &Controls) -> String {
        let markdown = match self {
            Self::Welcome => welcome::build(controls),
            Self::Features => features::build(),
            Self::Changelog => changelog::build(),
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
            Self::Licenses => licenses::build(),
        };
        markdown.replace('→', crate::icons::ARROW_RIGHT)
    }
}

impl Display for InfoTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Welcome => write!(f, "Welcome"),
            Self::Features => write!(f, "Features & plans"),
            Self::Changelog => write!(f, "Changelog"),
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
            Self::Licenses => write!(f, "Licences"),
        }
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct InfoState {
    pub tab: InfoTab,
}

pub struct InfoPanel<'a> {
    state: &'a mut InfoState,
    commonmark: &'a mut egui_commonmark::CommonMarkCache,
    controls: Controls<'a>,
}

impl<'a> InfoPanel<'a> {
    pub fn new(
        state: &'a mut InfoState,
        commonmark: &'a mut egui_commonmark::CommonMarkCache,
        controls: Controls<'a>,
    ) -> Self {
        Self {
            state,
            commonmark,
            controls,
        }
    }
}

impl Widget for InfoPanel<'_> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        ui.style_mut().url_in_tooltip = true;
        ui.vertical_centered(|ui| {
            self.tab_bar(ui);

            ui.separator();

            let markdown = self.state.tab.build_md(&self.controls);
            for (index, block) in blocks(&markdown).into_iter().enumerate() {
                match block {
                    Block::Text(text) => {
                        egui_commonmark::CommonMarkViewer::new().show(ui, self.commonmark, &text);
                    }
                    Block::Table(table) => {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            Form::show(ui, ("info_table", index), |form| {
                                form.striped();
                                form.text_only();
                                let mut rows = table_rows(&table).into_iter();
                                if let Some((label, value)) = rows.next() {
                                    form.header(&label, &value);
                                }
                                for (label, value) in rows {
                                    form.row(&label, |ui| {
                                        ui.add(egui::Label::new(value.as_str()).wrap());
                                    });
                                }
                            });
                        });
                        ui.add_space(ui.text_style_height(&egui::TextStyle::Body));
                    }
                    Block::List(items) => {
                        let gap = ui.text_style_height(&egui::TextStyle::Body);
                        if index > 0 {
                            ui.add_space(gap);
                        }
                        list(ui, self.commonmark, &items);
                        ui.add_space(gap);
                    }
                }
            }
        })
        .response
    }
}

impl InfoPanel<'_> {
    fn tab_bar(&mut self, ui: &mut Ui) {
        let tabs: Vec<(InfoTab, String)> =
            InfoTab::iter().map(|tab| (tab, tab.to_string())).collect();
        tab_bar(ui, &mut self.state.tab, &tabs);
    }
}

#[derive(Debug, PartialEq)]
enum Block {
    Text(String),
    Table(String),
    List(Vec<Item>),
}

#[derive(Debug, PartialEq)]
struct Item {
    depth: usize,
    marker: Marker,
    text: String,
}

#[derive(Debug, PartialEq)]
enum Marker {
    Bullet,
    Number(String),
}

fn list_item(line: &str) -> Option<(usize, Marker, &str)> {
    let rest = line.trim_start();
    let indent = line.len() - rest.len();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(text) = rest.strip_prefix(bullet) {
            return Some((indent, Marker::Bullet, text));
        }
    }
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let text = rest[digits..].strip_prefix(". ")?;
    (digits > 0).then(|| (indent, Marker::Number(rest[..digits].to_string()), text))
}

#[derive(Default)]
struct BlockSplitter {
    blocks: Vec<Block>,
    text: String,
    table: String,
    items: Vec<Item>,
    indents: Vec<usize>,
}

impl BlockSplitter {
    fn flush_text(&mut self) {
        let text = std::mem::take(&mut self.text);
        if !text.trim().is_empty() {
            self.blocks.push(Block::Text(text));
        }
    }

    fn flush_table(&mut self) {
        let table = std::mem::take(&mut self.table);
        if !table.trim().is_empty() {
            self.blocks.push(Block::Table(table));
        }
    }

    fn flush_list(&mut self) {
        self.indents.clear();
        if !self.items.is_empty() {
            self.blocks
                .push(Block::List(std::mem::take(&mut self.items)));
        }
    }

    fn line(&mut self, line: &str) {
        if let Some((indent, marker, text)) = list_item(line) {
            self.flush_text();
            self.flush_table();
            while self.indents.last().is_some_and(|&last| last > indent) {
                self.indents.pop();
            }
            if self.indents.last().is_none_or(|&last| last < indent) {
                self.indents.push(indent);
            }
            self.items.push(Item {
                depth: self.indents.len() - 1,
                marker,
                text: text.trim().to_string(),
            });
            return;
        }
        let continues_item = line.starts_with(char::is_whitespace) && !line.trim().is_empty();
        if let Some(item) = self.items.last_mut().filter(|_| continues_item) {
            item.text.push(' ');
            item.text.push_str(line.trim());
            return;
        }
        self.flush_list();
        if line.trim_start().starts_with('|') {
            self.flush_text();
            self.table.push_str(line);
            self.table.push('\n');
        } else {
            self.flush_table();
            self.text.push_str(line);
            self.text.push('\n');
        }
    }
}

fn blocks(markdown: &str) -> Vec<Block> {
    let mut splitter = BlockSplitter::default();
    for line in markdown.lines() {
        splitter.line(line);
    }
    splitter.flush_text();
    splitter.flush_table();
    splitter.flush_list();
    splitter.blocks
}

fn list(ui: &mut Ui, commonmark: &mut egui_commonmark::CommonMarkCache, items: &[Item]) {
    let row = ui.text_style_height(&egui::TextStyle::Body);
    let space = ui.fonts_mut(|f| f.glyph_width(&egui::TextStyle::Body.resolve(ui.style()), ' '));
    let marker_width = row * 1.5;
    for item in items {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(item.depth as f32 * marker_width);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(marker_width, row), egui::Sense::hover());
            let colour = ui.visuals().strong_text_color();
            match &item.marker {
                Marker::Bullet if item.depth == 0 => {
                    ui.painter().circle_filled(rect.center(), row / 6.0, colour);
                }
                Marker::Bullet => {
                    ui.painter().circle_stroke(
                        rect.center(),
                        row / 6.0,
                        egui::Stroke::new(0.6_f32, colour),
                    );
                }
                Marker::Number(number) => {
                    ui.painter().text(
                        rect.right_center() - egui::vec2(space, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        format!("{number}."),
                        egui::TextStyle::Body.resolve(ui.style()),
                        colour,
                    );
                }
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                egui_commonmark::CommonMarkViewer::new().show(ui, commonmark, &item.text);
            });
        });
    }
}

fn table_rows(table: &str) -> Vec<(String, String)> {
    table
        .lines()
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>()
        })
        .filter(|cells| {
            !cells
                .iter()
                .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'))
        })
        .map(|cells| (cells[0].to_string(), cells[1..].join(" ")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_use_the_icon_font() {
        let (bindings, hotkeys) = (KeyBindings::default(), Hotkeys::default());
        for controls in [
            Controls::Touch,
            Controls::Desktop {
                bindings: &bindings,
                hotkeys: &hotkeys,
            },
        ] {
            for tab in InfoTab::iter() {
                assert!(!tab.build_md(&controls).contains('→'), "{tab}");
            }
        }
    }

    #[test]
    fn table_rows_skip_the_divider() {
        assert_eq!(
            table_rows("| Action | Keys |\n|--------|:----:|\n| Start | Enter, Space |\n"),
            vec![
                ("Action".to_string(), "Keys".to_string()),
                ("Start".to_string(), "Enter, Space".to_string()),
            ]
        );
    }

    #[test]
    fn tables_split_from_the_text_around_them() {
        let markdown = "# Title\nText\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nAfter\n";
        assert_eq!(
            blocks(markdown),
            vec![
                Block::Text("# Title\nText\n\n".into()),
                Block::Table("| A | B |\n|---|---|\n| 1 | 2 |\n".into()),
                Block::Text("\nAfter\n".into()),
            ]
        );
    }

    #[test]
    fn lists_split_into_items() {
        let markdown =
            "Intro\n\n- One\n- Two that\n  wraps\n    - Nested\n- Three\n1. First\n\nAfter\n";
        let item = |depth, marker, text: &str| Item {
            depth,
            marker,
            text: text.to_string(),
        };
        assert_eq!(
            blocks(markdown),
            vec![
                Block::Text("Intro\n\n".into()),
                Block::List(vec![
                    item(0, Marker::Bullet, "One"),
                    item(0, Marker::Bullet, "Two that wraps"),
                    item(1, Marker::Bullet, "Nested"),
                    item(0, Marker::Bullet, "Three"),
                    item(0, Marker::Number("1".into()), "First"),
                ]),
                Block::Text("\nAfter\n".into()),
            ]
        );
    }

    #[test]
    fn rules_and_bold_lines_are_not_list_items() {
        assert!(list_item("---").is_none());
        assert!(list_item("**Settings** opens").is_none());
        assert!(list_item("2026 was a year").is_none());
    }

    fn items_beside_bullets(tab: InfoTab, width: f32) -> Vec<(f32, std::sync::Arc<egui::Galley>)> {
        let ctx = egui::Context::default();
        let mut state = InfoState { tab };
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 60000.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                InfoPanel::new(&mut state, &mut cache, Controls::Touch).ui(ui);
            });
        });
        let mut bullets = Vec::new();
        let mut texts = Vec::new();
        for clipped in output.shapes {
            match clipped.shape {
                egui::Shape::Circle(circle) => bullets.push(circle.center),
                egui::Shape::Text(text) if !text.galley.text().trim().is_empty() => {
                    texts.push((text.pos, text.galley))
                }
                _ => {}
            }
        }
        assert!(!bullets.is_empty(), "{tab} has no bullets");
        bullets
            .into_iter()
            .map(|bullet| {
                texts
                    .iter()
                    .filter(|(pos, galley)| {
                        let first = galley.rows[0].rect().translate(pos.to_vec2());
                        first.y_range().contains(bullet.y) && pos.x > bullet.x
                    })
                    .min_by(|a, b| a.0.x.total_cmp(&b.0.x))
                    .map(|(_, galley)| (bullet.x, galley.clone()))
                    .unwrap_or_else(|| {
                        panic!("{tab} at {width}: no text starts beside the bullet at {bullet:?}")
                    })
            })
            .collect()
    }

    #[test]
    fn list_items_sit_beside_their_bullet_in_every_about_page() {
        for tab in [InfoTab::Welcome, InfoTab::Features, InfoTab::Changelog] {
            for width in [412.0, 1280.0] {
                items_beside_bullets(tab, width);
            }
        }
    }

    #[test]
    fn a_long_item_wraps_at_phone_width() {
        let (_, item) = items_beside_bullets(InfoTab::Changelog, 412.0)
            .into_iter()
            .find(|(_, galley)| galley.text().starts_with("Remappable shortcuts"))
            .expect("shortcuts item");
        assert!(item.rows.len() > 1, "{:?} did not wrap", item.text());
    }

    fn welcome(controls: &Controls) -> String {
        InfoTab::Welcome.build_md(controls)
    }

    #[test]
    fn the_touch_layout_leaves_out_desktop_help() {
        let about = welcome(&Controls::Touch);
        for desktop in [
            "Tabs, panels",
            "drag",
            "Keyboard",
            "Shortcuts",
            "F8",
            "(default)",
        ] {
            assert!(!about.contains(desktop), "touch About mentions {desktop:?}");
        }
        assert!(about.contains("on-screen d-pad"));
        assert!(
            !about.contains("\n---\n\n---"),
            "no separator over an empty section"
        );
        assert!(
            !about.trim_end().ends_with("---"),
            "no separator at the end"
        );
    }

    #[test]
    fn about_shows_the_keys_the_player_bound() {
        use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction};
        use crate::app::ui_state::keybinds::GbButton;
        use egui::Key;

        let mut bindings = KeyBindings::default();
        bindings.unbind_key(Key::Enter);
        bindings.unbind_key(Key::Space);
        bindings.bind_key(Key::K, GbButton::Start);
        let mut hotkeys = Hotkeys::default();
        hotkeys.unbind(Hotkey::plain(Key::F5));
        hotkeys.bind(Hotkey::ctrl(Key::P), HotkeyAction::TogglePause);

        let about = welcome(&Controls::Desktop {
            bindings: &bindings,
            hotkeys: &hotkeys,
        });
        assert!(about.contains("| Start | K |"), "{about}");
        assert!(about.contains("| Pause / resume | Ctrl+P |"), "{about}");
        assert!(about.contains("| Quick load | Hold F9 |"), "{about}");
        assert!(!about.contains("F5"));
        assert!(!about.contains("(default)"));
        assert!(about.contains("Tabs, panels and windows"));
    }

    #[test]
    fn an_unbound_button_says_so() {
        let mut bindings = KeyBindings::default();
        bindings.unbind_key(egui::Key::Backspace);
        let about = welcome(&Controls::Desktop {
            bindings: &bindings,
            hotkeys: &Hotkeys::default(),
        });
        assert!(about.contains("| Select | Nothing bound |"), "{about}");
    }
}
