use crate::debug::tiles::{Depth, Flip, Picture, Rect};

mod game_boy;
#[cfg(feature = "gba")]
mod gba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetColors {
    Grey,
    Palette { group: usize, row: usize },
}

impl Default for SheetColors {
    fn default() -> Self {
        SheetColors::Palette { group: 0, row: 0 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSheet {
    pub name: String,
    pub start: u32,
    pub bank: u16,
    pub depth: Depth,
    pub columns: usize,
    pub palette_group: usize,
    pub picture: Picture,
}

impl TileSheet {
    pub fn tiles(&self) -> usize {
        (self.picture.width / 8) * (self.picture.height / 8)
    }

    pub fn tile_at(&self, x: usize, y: usize) -> Option<usize> {
        let tile = (y / 8) * self.columns + x / 8;
        (x < self.picture.width && y < self.picture.height).then_some(tile)
    }

    pub fn tile_rect(&self, tile: usize) -> Rect {
        Rect {
            x: (tile % self.columns) * 8,
            y: (tile / self.columns) * 8,
            width: 8,
            height: 8,
        }
    }

    pub fn tile_address(&self, tile: usize) -> u32 {
        self.start + (tile * self.depth.tile_bytes()) as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRef {
    pub sheet: usize,
    pub tile: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapCell {
    pub address: u32,
    pub tile: u16,
    pub source: TileRef,
    pub flip: Flip,
    pub palette: Option<u8>,
    pub bank: Option<u8>,
    pub priority: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub name: &'static str,
    pub rects: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileMap {
    pub name: String,
    pub columns: usize,
    pub cells: Vec<MapCell>,
    pub frames: Vec<Frame>,
    pub picture: Picture,
}

impl TileMap {
    pub fn cell_at(&self, x: usize, y: usize) -> Option<&MapCell> {
        if x >= self.picture.width || y >= self.picture.height {
            return None;
        }
        self.cells.get((y / 8) * self.columns + x / 8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub value: String,
}

impl Field {
    pub fn new(name: &'static str, value: impl Into<String>) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}

pub fn address(value: u32) -> String {
    if value > 0xFFFF {
        format!("${value:08X}")
    } else {
        format!("${value:04X}")
    }
}

pub fn yes_no(on: bool) -> &'static str {
    if on { "Yes" } else { "No" }
}

impl MapCell {
    pub fn fields(&self) -> Vec<Field> {
        let mut out = vec![
            Field::new("Address", address(self.address)),
            Field::new("Tile index", format!("${:02X}", self.tile)),
        ];
        if let Some(bank) = self.bank {
            out.push(Field::new("Bank", bank.to_string()));
        }
        if let Some(palette) = self.palette {
            out.push(Field::new("Palette", format!("Background {palette}")));
        }
        if self.bank.is_some() || self.palette.is_some() {
            out.push(Field::new("X flip", yes_no(self.flip.x)));
            out.push(Field::new("Y flip", yes_no(self.flip.y)));
        }
        if let Some(priority) = self.priority {
            out.push(Field::new("Above sprites", yes_no(priority)));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub index: usize,
    pub x: i32,
    pub y: i32,
    pub source: TileRef,
    pub on_screen: bool,
    pub bytes: Vec<u8>,
    pub fields: Vec<Field>,
    pub picture: Picture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorValue {
    Shade,
    Rgb555,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub name: &'static str,
    pub row: usize,
    pub kind: ColorValue,
    pub values: Vec<u16>,
    pub colors: Vec<[u8; 4]>,
}

impl Palette {
    pub fn rows(&self) -> usize {
        self.colors.len().div_ceil(self.row)
    }

    pub fn row_colors(&self, row: usize, depth: Depth) -> &[[u8; 4]] {
        if depth.colors() > self.row {
            return &self.colors;
        }
        let start = (row * self.row).min(self.colors.len());
        &self.colors[start..(start + self.row).min(self.colors.len())]
    }

    pub fn describe(&self, index: usize) -> String {
        let value = self.values.get(index).copied().unwrap_or_default();
        match self.kind {
            ColorValue::Shade => format!("Shade {value}"),
            ColorValue::Rgb555 => format!(
                "${value:04X}, red {}, green {}, blue {}",
                value & 0x1F,
                (value >> 5) & 0x1F,
                (value >> 10) & 0x1F
            ),
        }
    }
}

pub fn row_name(palette: &Palette, row: usize) -> String {
    if palette.rows() > 1 {
        format!("{} {row}", palette.name)
    } else {
        palette.name.to_string()
    }
}

pub(crate) fn sheet_colors(palettes: &[Palette], shown: SheetColors, depth: Depth) -> Vec<[u8; 4]> {
    match shown {
        SheetColors::Palette { group, row } => match palettes.get(group) {
            Some(palette) => palette.row_colors(row, depth).to_vec(),
            None => crate::debug::tiles::grey(depth),
        },
        SheetColors::Grey => crate::debug::tiles::grey(depth),
    }
}
