use crate::app::widgets::scroll;
use crate::app::widgets::tab_bar::tab_bar;
use crate::app::widgets::text::faint;
use crate::icons;
use citrine_gb::debug::Debug;
use citrine_gb::debug::tiles::{Picture, Rect as PixelRect};
use citrine_gb::debug::video::{
    Field, Palette, SheetColors, Sprite, TileMap, TileRef, TileSheet, address, row_name,
};
use egui::{
    Align, Color32, ColorImage, ComboBox, Frame, Grid, Layout, Pos2, Rect, Response, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextWrapMode, TextureHandle, TextureOptions,
    TopBottomPanel, Ui, pos2, vec2,
};
use std::collections::HashMap;

const MAX_SCALE: f32 = 6.0;
const SPRITE_BOX: f32 = 56.0;
const SPRITE_GAP: f32 = 6.0;
const SWATCH_MAX: f32 = 22.0;
const ROW_LABEL: f32 = 20.0;
const DETAILS_SHARE: f32 = 0.4;
const PICKER_MAX: f32 = 220.0;
const CHECKER_SIZE: f32 = 4.0;
const DETAILS_GAP: f32 = 16.0;
const DETAILS_ROW_GAP: f32 = 2.0;
const CHECKER_DARK: Color32 = Color32::from_gray(0x90);
const CHECKER_LIGHT: Color32 = Color32::from_gray(0xB0);
const SCREEN: Color32 = Color32::from_rgb(0x4D, 0xD0, 0xFF);
const WINDOW: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x20);
const PICKED: Color32 = Color32::from_rgb(0xFF, 0x5C, 0xC8);
const HOVERED: Color32 = Color32::WHITE;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VideoTab {
    #[default]
    Tiles,
    Maps,
    Sprites,
    Palettes,
}

impl VideoTab {
    pub const ALL: [VideoTab; 4] = [
        VideoTab::Tiles,
        VideoTab::Maps,
        VideoTab::Sprites,
        VideoTab::Palettes,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoTab::Tiles => "Tiles",
            VideoTab::Maps => "Maps",
            VideoTab::Sprites => "Sprites",
            VideoTab::Palettes => "Palettes",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    Sheet,
    Map,
    Sprite(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picked {
    Cell { map: usize, x: usize, y: usize },
    Sprite(usize),
    Swatch { group: usize, index: usize },
}

#[derive(Default)]
pub struct VideoViewState {
    pub tab: VideoTab,
    pub sheet: usize,
    pub map: usize,
    pub colors: SheetColors,
    pub selected: Option<TileRef>,
    pub show_hidden: bool,
    pub back: Option<VideoTab>,
    picked: Option<Picked>,
    follow: bool,
    sheets: Vec<TileSheet>,
    maps: Vec<TileMap>,
    sprites: Vec<Sprite>,
    palettes: Vec<Palette>,
    textures: HashMap<Slot, TextureHandle>,
}

impl VideoViewState {
    pub fn refresh(&mut self, console: &dyn Debug) {
        console.palettes(&mut self.palettes);
        match self.tab {
            VideoTab::Tiles => {
                console.tile_sheets(self.colors, &mut self.sheets);
                if std::mem::take(&mut self.follow) {
                    let before = self.colors;
                    self.follow_sheet_palette();
                    if self.colors != before {
                        console.tile_sheets(self.colors, &mut self.sheets);
                    }
                }
            }
            VideoTab::Maps => console.tile_maps(&mut self.maps),
            VideoTab::Sprites => console.sprites(&mut self.sprites),
            VideoTab::Palettes => {}
        }
        self.sheet = self.sheet.min(self.sheets.len().saturating_sub(1));
        self.map = self.map.min(self.maps.len().saturating_sub(1));
    }

    pub fn select(&mut self, tile: TileRef) {
        self.back = (self.tab != VideoTab::Tiles).then_some(self.tab);
        self.selected = Some(tile);
        self.sheet = tile.sheet;
        self.tab = VideoTab::Tiles;
        self.follow = true;
    }

    fn follow_sheet_palette(&mut self) {
        let Some(sheet) = self.sheets.get(self.sheet) else {
            return;
        };
        if let SheetColors::Palette { group, row } = self.colors
            && group != sheet.palette_group
        {
            self.colors = SheetColors::Palette {
                group: sheet.palette_group,
                row,
            };
        }
    }

    fn pick(&mut self, picked: Picked) {
        self.picked = (self.picked != Some(picked)).then_some(picked);
    }

    fn details(&self) -> Option<(Vec<Field>, Option<TileRef>)> {
        match (self.tab, self.picked) {
            (VideoTab::Tiles, _) => {
                let selected = self.selected.filter(|tile| tile.sheet == self.sheet)?;
                let sheet = self.sheets.get(selected.sheet)?;
                Some((
                    tile_fields(sheet, selected.tile, self.sheets.len() > 1),
                    None,
                ))
            }
            (VideoTab::Maps, Some(Picked::Cell { map, x, y })) if map == self.map => {
                let cell = self.maps.get(map)?.cell_at(x, y)?;
                Some((cell.fields(), Some(cell.source)))
            }
            (VideoTab::Sprites, Some(Picked::Sprite(index))) => {
                let sprite = self.sprites.get(index)?;
                Some((sprite.fields.clone(), Some(sprite.source)))
            }
            (VideoTab::Palettes, Some(Picked::Swatch { group, index })) => {
                let palette = self.palettes.get(group)?;
                Some((swatch_fields(palette, index), None))
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn sprites(&self) -> &[Sprite] {
        &self.sprites
    }

    #[cfg(test)]
    pub fn maps(&self) -> &[TileMap] {
        &self.maps
    }

    #[cfg(test)]
    pub fn palettes(&self) -> &[Palette] {
        &self.palettes
    }
}

pub struct VideoView<'a> {
    state: &'a mut VideoViewState,
}

impl<'a> VideoView<'a> {
    pub fn new(state: &'a mut VideoViewState) -> Self {
        Self { state }
    }

    pub fn show(self, ui: &mut Ui) {
        let state = self.state;
        let tabs: Vec<(VideoTab, String)> = VideoTab::ALL
            .iter()
            .map(|tab| (*tab, tab.label().to_string()))
            .collect();
        let before = state.tab;
        tab_bar(ui, &mut state.tab, &tabs);
        if state.tab != before {
            state.back = None;
        }
        ui.separator();
        toolbar(ui, state);
        if let Some((fields, tile)) = state.details() {
            let max_height = ui.available_height() * DETAILS_SHARE;
            TopBottomPanel::bottom(ui.id().with("video_details"))
                .frame(Frame::NONE)
                .resizable(false)
                .show_separator_line(false)
                .show_inside(ui, |ui| {
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);
                    details(ui, state, &fields, tile, max_height);
                    ui.add_space(4.0);
                });
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match state.tab {
                VideoTab::Tiles => tiles(ui, state),
                VideoTab::Maps => maps(ui, state),
                VideoTab::Sprites => sprites(ui, state),
                VideoTab::Palettes => palettes(ui, state),
            });
    }
}

fn toolbar(ui: &mut Ui, state: &mut VideoViewState) {
    ui.horizontal_wrapped(|ui| match state.tab {
        VideoTab::Tiles => tiles_toolbar(ui, state),
        VideoTab::Maps => maps_toolbar(ui, state),
        VideoTab::Sprites => {
            let on_screen = state
                .sprites
                .iter()
                .filter(|sprite| sprite.on_screen)
                .count();
            ui.label(
                RichText::new(format!("{on_screen} of {} on screen", state.sprites.len())).weak(),
            );
            crate::app::widgets::check::checkbox(ui, &mut state.show_hidden, "Show hidden");
        }
        VideoTab::Palettes => {}
    });
    if state.tab != VideoTab::Palettes {
        ui.add_space(4.0);
    }
}

fn details(
    ui: &mut Ui,
    state: &mut VideoViewState,
    fields: &[Field],
    tile: Option<TileRef>,
    max_height: f32,
) {
    scroll::gutter(ui);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let widest = |text: &mut dyn Iterator<Item = &str>| {
        ui.fonts_mut(|fonts| {
            text.map(|text| {
                fonts
                    .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
        })
    };
    let label_width = widest(&mut fields.iter().map(|field| field.name));
    let value_width = widest(&mut fields.iter().map(|field| field.value.as_str()));
    let word_width = widest(&mut fields.iter().flat_map(|field| field.name.split(' ')));
    let pair = label_width + DETAILS_GAP + value_width;
    let available = ui.available_width() - ui.spacing().scroll.floating_allocated_width;
    let pairs = (((available + DETAILS_GAP) / (pair + DETAILS_GAP)) as usize).clamp(1, 3);
    let label_width = label_width.min(available - DETAILS_GAP - value_width);
    let stacked = label_width < word_width;
    let width = |text: &str| widest(&mut std::iter::once(text));
    let labels: Vec<String> = fields
        .iter()
        .map(|field| {
            let mut lines: Vec<String> = Vec::new();
            for word in field.name.split(' ') {
                match lines.last_mut() {
                    Some(line) if width(&format!("{line} {word}")) <= label_width => {
                        line.push(' ');
                        line.push_str(word);
                    }
                    _ => lines.push(word.to_owned()),
                }
            }
            lines.join("\n")
        })
        .collect();
    let button = if tile.is_some() {
        ui.spacing().interact_size.y + ui.spacing().item_spacing.y
    } else {
        0.0
    };
    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
    let bar = &mut ui.spacing_mut().scroll;
    bar.dormant_handle_opacity = bar.active_handle_opacity;
    let rows_id = ui.id().with("video_details_rows");
    let bottoms: Vec<f32> = ui.data(|data| data.get_temp(rows_id)).unwrap_or_default();
    let limit = max_height - button;
    let max_height = bottoms
        .iter()
        .copied()
        .filter(|&bottom| bottom <= limit)
        .reduce(f32::max)
        .or(bottoms.first().copied())
        .unwrap_or(limit);
    let mut bottoms = Vec::new();
    ScrollArea::vertical()
        .id_salt("video_details_scroll")
        .auto_shrink([false, true])
        .max_height(max_height)
        .show(ui, |ui| {
            let top = ui.cursor().top();
            if stacked {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
                for field in fields {
                    ui.label(RichText::new(field.name).weak());
                    let value = ui.label(&field.value);
                    bottoms.push(value.rect.bottom() - top);
                    ui.add_space(4.0);
                }
                return;
            }
            Grid::new("video_details_grid")
                .num_columns(pairs * 2)
                .min_row_height(0.0)
                .spacing(vec2(DETAILS_GAP, DETAILS_ROW_GAP))
                .show(ui, |ui| {
                    let rows = fields.len().div_ceil(pairs);
                    for row in 0..rows {
                        let mut bottom = top;
                        for index in (0..pairs)
                            .map(|pair| pair * rows + row)
                            .filter(|&index| index < fields.len())
                        {
                            let label = ui.label(RichText::new(&labels[index]).weak());
                            let lines = labels[index].lines().count();
                            let value = &fields[index].value;
                            let value = ui.label(format!("{value}{}", "\n".repeat(lines - 1)));
                            bottom = bottom.max(label.rect.bottom()).max(value.rect.bottom());
                        }
                        bottoms.push(bottom - top);
                        ui.end_row();
                    }
                });
        });
    ui.data_mut(|data| data.insert_temp(rows_id, bottoms));
    ui.add_space(4.0);
    if let Some(tile) = tile
        && ui
            .button(format!("{}  Show tile", icons::ARROW_RIGHT))
            .clicked()
    {
        state.select(tile);
    }
}

fn texture(ui: &Ui, textures: &mut HashMap<Slot, TextureHandle>, slot: Slot, picture: &Picture) {
    let image = ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
    match textures.get_mut(&slot) {
        Some(handle) => handle.set(image, TextureOptions::NEAREST),
        None => {
            let handle = ui
                .ctx()
                .load_texture("video_view", image, TextureOptions::NEAREST);
            textures.insert(slot, handle);
        }
    }
}

struct Shown {
    response: Response,
    scale: f32,
}

impl Shown {
    fn pixel_at(&self, pos: Pos2) -> (usize, usize) {
        let local = (pos - self.response.rect.min) / self.scale;
        (local.x.max(0.0) as usize, local.y.max(0.0) as usize)
    }

    fn hovered_pixel(&self) -> Option<(usize, usize)> {
        self.response.hover_pos().map(|pos| self.pixel_at(pos))
    }

    fn clicked_pixel(&self) -> Option<(usize, usize)> {
        if !self.response.clicked() {
            return None;
        }
        self.response
            .interact_pointer_pos()
            .map(|pos| self.pixel_at(pos))
    }

    fn rect(&self, rect: PixelRect) -> Rect {
        let min = self.response.rect.min + vec2(rect.x as f32, rect.y as f32) * self.scale;
        Rect::from_min_size(
            min,
            vec2(rect.width as f32, rect.height as f32) * self.scale,
        )
    }
}

fn fit_scale(ui: &Ui, width: usize) -> f32 {
    let fit = ui.available_width() / width.max(1) as f32;
    let pixels = ui.ctx().pixels_per_point();
    if fit * pixels >= 1.0 {
        ((fit * pixels).floor() / pixels).min(MAX_SCALE)
    } else {
        fit
    }
}

fn show_picture(
    ui: &mut Ui,
    textures: &mut HashMap<Slot, TextureHandle>,
    slot: Slot,
    picture: &Picture,
) -> Shown {
    let scale = fit_scale(ui, picture.width);
    texture(ui, textures, slot, picture);
    let size = vec2(picture.width as f32, picture.height as f32) * scale;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect)
        && let Some(handle) = textures.get(&slot)
    {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(handle.id(), rect, uv, Color32::WHITE);
    }
    Shown { response, scale }
}

fn mark(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter();
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(3.0_f32, Color32::from_black_alpha(200)),
        StrokeKind::Outside,
    );
    painter.rect_stroke(rect, 0.0, Stroke::new(1.5_f32, color), StrokeKind::Outside);
}

fn fields_grid(ui: &mut Ui, id: &str, fields: &[Field]) {
    Grid::new(id)
        .num_columns(2)
        .spacing(vec2(12.0, 2.0))
        .show(ui, |ui| {
            for field in fields {
                ui.label(faint(field.name));
                ui.label(&field.value);
                ui.end_row();
            }
        });
}

fn tile_fields(sheet: &TileSheet, tile: usize, banked: bool) -> Vec<Field> {
    let mut fields = vec![
        Field::new("Address", address(sheet.tile_address(tile))),
        Field::new("Tile index", format!("${tile:02X}")),
    ];
    if banked {
        fields.push(Field::new("Bank", sheet.bank.to_string()));
    }
    fields
}

fn swatch_fields(palette: &Palette, index: usize) -> Vec<Field> {
    let row = index / palette.row.max(1);
    let column = index % palette.row.max(1);
    vec![
        Field::new(
            "Colour",
            format!("{}, colour {column}", row_name(palette, row)),
        ),
        Field::new("Value", palette.describe(index)),
    ]
}

fn color_choices(palettes: &[Palette]) -> Vec<(SheetColors, String)> {
    let mut out = Vec::new();
    for (group, palette) in palettes.iter().enumerate() {
        for row in 0..palette.rows() {
            out.push((SheetColors::Palette { group, row }, row_name(palette, row)));
        }
    }
    out.push((SheetColors::Grey, "Grey".to_string()));
    out
}

fn tiles_toolbar(ui: &mut Ui, state: &mut VideoViewState) {
    if let Some(back) = state.back
        && ui
            .button(format!("{}  Back to {}", icons::ARROW_LEFT, back.label()))
            .clicked()
    {
        state.tab = back;
        state.back = None;
        return;
    }
    let before = state.sheet;
    let choices = color_choices(&state.palettes);
    let colors = choices
        .iter()
        .find(|(colors, _)| *colors == state.colors)
        .map_or("Grey", |(_, name)| name.as_str());
    let colors = format!("Colours: {colors}");
    let sheet =
        (state.sheets.len() > 1).then(|| format!("Tiles: {}", state.sheets[state.sheet].name));
    let gap = ui.spacing().item_spacing.x;
    let need = [sheet.as_deref(), Some(colors.as_str())]
        .into_iter()
        .flatten()
        .map(|text| picker_width(ui, text))
        .fold(0.0, f32::max);
    let side_by_side = (ui.available_width() - gap) / 2.0;
    let stacked = sheet.is_some() && side_by_side < need;
    let width = if sheet.is_none() || stacked {
        ui.available_width()
    } else {
        side_by_side
    }
    .min(PICKER_MAX);
    if let Some(current) = sheet {
        picker(ui, width, |ui| {
            ComboBox::from_id_salt("video_sheet")
                .width(width)
                .truncate()
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (index, sheet) in state.sheets.iter().enumerate() {
                        ui.selectable_value(&mut state.sheet, index, &sheet.name);
                    }
                });
        });
        if stacked {
            ui.end_row();
        }
    }
    state.follow |= state.sheet != before;
    picker(ui, width, |ui| {
        ComboBox::from_id_salt("video_colors")
            .width(width)
            .truncate()
            .selected_text(colors)
            .show_ui(ui, |ui| {
                for (colors, name) in &choices {
                    ui.selectable_value(&mut state.colors, *colors, name);
                }
            });
    });
}

fn picker_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui
        .fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font, Color32::WHITE))
        .size()
        .x;
    let spacing = ui.spacing();
    text + spacing.icon_width + spacing.icon_spacing + spacing.button_padding.x * 2.0
}

fn checker(painter: &egui::Painter, rect: Rect, cell: f32) {
    painter.rect_filled(rect, 0.0, CHECKER_LIGHT);
    let columns = (rect.width() / cell).ceil() as usize;
    let rows = (rect.height() / cell).ceil() as usize;
    for row in 0..rows {
        for column in (row % 2..columns).step_by(2) {
            let min = rect.min + vec2(column as f32 * cell, row as f32 * cell);
            let square = Rect::from_min_size(min, vec2(cell, cell)).intersect(rect);
            painter.rect_filled(square, 0.0, CHECKER_DARK);
        }
    }
}

fn picker(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui(vec2(width, ui.spacing().interact_size.y), add);
}

fn maps_toolbar(ui: &mut Ui, state: &mut VideoViewState) {
    if state.maps.len() > 1 {
        let current = state.maps[state.map].name.clone();
        ComboBox::from_id_salt("video_map")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (index, map) in state.maps.iter().enumerate() {
                    ui.selectable_value(&mut state.map, index, &map.name);
                }
            });
    }
    let Some(map) = state.maps.get(state.map) else {
        return;
    };
    for (frame, color) in map.frames.iter().zip([SCREEN, WINDOW]) {
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        mark(ui, rect.shrink(2.0), color);
        ui.label(frame.name);
    }
}

fn tiles(ui: &mut Ui, state: &mut VideoViewState) {
    let Some(sheet) = state.sheets.get(state.sheet) else {
        return;
    };
    let shown = show_picture(ui, &mut state.textures, Slot::Sheet, &sheet.picture);
    if let Some(selected) = state.selected.filter(|tile| tile.sheet == state.sheet) {
        mark(ui, shown.rect(sheet.tile_rect(selected.tile)), PICKED);
    }
    if let Some((x, y)) = shown.clicked_pixel()
        && let Some(tile) = sheet.tile_at(x, y)
    {
        let picked = TileRef {
            sheet: state.sheet,
            tile,
        };
        state.selected = (state.selected != Some(picked)).then_some(picked);
    }
    let Some(tile) = shown.hovered_pixel().and_then(|(x, y)| sheet.tile_at(x, y)) else {
        return;
    };
    mark(ui, shown.rect(sheet.tile_rect(tile)), HOVERED);
    let fields = tile_fields(sheet, tile, state.sheets.len() > 1);
    shown
        .response
        .on_hover_ui(|ui| fields_grid(ui, "video_tile_hover", &fields));
}

fn cell_rect(x: usize, y: usize) -> PixelRect {
    PixelRect {
        x: x / 8 * 8,
        y: y / 8 * 8,
        width: 8,
        height: 8,
    }
}

fn maps(ui: &mut Ui, state: &mut VideoViewState) {
    let Some(map) = state.maps.get(state.map) else {
        return;
    };
    let shown = show_picture(ui, &mut state.textures, Slot::Map, &map.picture);
    for (frame, color) in map.frames.iter().zip([SCREEN, WINDOW]) {
        for rect in &frame.rects {
            mark(ui, shown.rect(*rect), color);
        }
    }
    if let Some(Picked::Cell { map: index, x, y }) = state.picked
        && index == state.map
    {
        mark(ui, shown.rect(cell_rect(x, y)), PICKED);
    }
    if let Some((x, y)) = shown.clicked_pixel()
        && map.cell_at(x, y).is_some()
    {
        let here = Picked::Cell {
            map: state.map,
            x: x / 8 * 8,
            y: y / 8 * 8,
        };
        state.picked = (state.picked != Some(here)).then_some(here);
    }
    let Some((x, y)) = shown.hovered_pixel() else {
        return;
    };
    let Some(cell) = map.cell_at(x, y) else {
        return;
    };
    mark(ui, shown.rect(cell_rect(x, y)), HOVERED);
    let fields = cell.fields();
    shown
        .response
        .on_hover_ui(|ui| fields_grid(ui, "video_cell_hover", &fields));
}

fn sprites(ui: &mut Ui, state: &mut VideoViewState) {
    let width = ui.available_width();
    let columns = ((width + SPRITE_GAP) / (SPRITE_BOX + SPRITE_GAP))
        .floor()
        .max(1.0);
    let card = ((width - SPRITE_GAP * (columns - 1.0)) / columns).floor();
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPRITE_GAP, SPRITE_GAP);
        for sprite in &state.sprites {
            let is_picked = state.picked == Some(Picked::Sprite(sprite.index));
            if !sprite.on_screen && !state.show_hidden && !is_picked {
                continue;
            }
            let slot = Slot::Sprite(sprite.index);
            texture(ui, &mut state.textures, slot, &sprite.picture);
            let (rect, response) = ui.allocate_exact_size(vec2(card, card + 14.0), Sense::click());
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let painter = ui.painter();
            let visuals = ui.visuals();
            let tile_box = Rect::from_min_size(rect.min, vec2(card, card));
            let fill = if sprite.on_screen {
                visuals.extreme_bg_color
            } else {
                visuals.faint_bg_color
            };
            painter.rect_filled(tile_box, 4.0, fill);
            painter.rect_stroke(
                tile_box,
                4.0,
                visuals.widgets.noninteractive.bg_stroke,
                StrokeKind::Inside,
            );
            let largest = sprite.picture.width.max(sprite.picture.height).max(1) as f32;
            let scale = ((card - 8.0) / largest).floor().max(1.0);
            let size = vec2(sprite.picture.width as f32, sprite.picture.height as f32) * scale;
            let image = Rect::from_center_size(tile_box.center(), size);
            checker(painter, image, CHECKER_SIZE);
            if let Some(handle) = state.textures.get(&slot) {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(handle.id(), image, uv, Color32::WHITE);
            }
            if is_picked {
                mark(ui, tile_box, PICKED);
            } else if response.hovered() {
                mark(ui, tile_box, HOVERED);
            }
            let text_color = if sprite.on_screen {
                visuals.text_color()
            } else {
                visuals.weak_text_color()
            };
            painter.text(
                pos2(rect.center().x, tile_box.bottom() + 7.0),
                egui::Align2::CENTER_CENTER,
                sprite.index.to_string(),
                egui::FontId::proportional(11.0),
                text_color,
            );
            if response.clicked() {
                picked = Some(sprite.index);
            }
            response.on_hover_ui(|ui| {
                fields_grid(ui, "video_sprite_hover", &sprite.fields);
            });
        }
    });
    if let Some(index) = picked {
        state.pick(Picked::Sprite(index));
    }
}

fn palettes(ui: &mut Ui, state: &mut VideoViewState) {
    let mut picked = None;
    for (group, palette) in state.palettes.iter().enumerate() {
        ui.label(RichText::new(palette.name).weak());
        let swatch = ((ui.available_width() - ROW_LABEL) / palette.row as f32)
            .floor()
            .clamp(8.0, SWATCH_MAX);
        ui.spacing_mut().item_spacing.y = 2.0;
        for row in 0..palette.rows() {
            let row_size = vec2(ui.available_width(), swatch);
            ui.allocate_ui_with_layout(row_size, Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (rect, _) = ui.allocate_exact_size(vec2(ROW_LABEL, swatch), Sense::hover());
                if palette.rows() > 1 {
                    ui.painter().text(
                        rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        row.to_string(),
                        egui::FontId::proportional(11.0),
                        ui.visuals().weak_text_color(),
                    );
                }
                for column in 0..palette.row {
                    let index = row * palette.row + column;
                    let Some(color) = palette.colors.get(index) else {
                        break;
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(swatch, swatch), Sense::click());
                    let inner = rect.shrink(1.0);
                    let fill = Color32::from_rgb(color[0], color[1], color[2]);
                    ui.painter().rect_filled(inner, 2.0, fill);
                    ui.painter().rect_stroke(
                        inner,
                        2.0,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        StrokeKind::Inside,
                    );
                    let here = Picked::Swatch { group, index };
                    if state.picked == Some(here) {
                        mark(ui, inner, PICKED);
                    } else if response.hovered() {
                        mark(ui, inner, HOVERED);
                    }
                    if response.clicked() {
                        picked = Some(here);
                    }
                    response.on_hover_ui(|ui| {
                        fields_grid(ui, "video_swatch_hover", &swatch_fields(palette, index));
                    });
                }
            });
        }
        ui.add_space(10.0);
    }
    if let Some(here) = picked {
        state.pick(here);
    }
}
