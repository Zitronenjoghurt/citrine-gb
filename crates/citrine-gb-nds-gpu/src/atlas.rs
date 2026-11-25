use std::collections::HashMap;

pub const WIDTH: u32 = 2048;
pub const LEVELS: u32 = 11;
pub const ENTRY_WORDS: usize = 4;
pub const PLACED: u32 = 1 << 31;

const KEY: u32 = 0x3FF0_FFFF;
const FORMAT_SHIFT: u32 = 26;
const DIRECT: u32 = 7;
const MIN_HEIGHT: u32 = 64;
const UNIT: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Texture {
    param: u32,
    palette: u32,
}

impl Texture {
    fn new(param: u32, palette: u32) -> Option<Self> {
        let format = (param >> FORMAT_SHIFT) & 7;
        if format == 0 {
            return None;
        }
        let palette = if format == DIRECT { 0 } else { palette };
        Some(Self {
            param: param & KEY,
            palette,
        })
    }

    fn width(self) -> u32 {
        8 << ((self.param >> 20) & 7)
    }

    fn height(self) -> u32 {
        8 << ((self.param >> 23) & 7)
    }

    fn levels(self) -> u32 {
        self.width().min(self.height()).ilog2() + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    texture: Texture,
    x: u32,
    y: u32,
}

impl Place {
    fn word(self) -> u32 {
        PLACED | (self.x / UNIT) | ((self.y / UNIT) << 9) | ((self.texture.levels() - 1) << 18)
    }

    fn entry(self) -> [u32; ENTRY_WORDS] {
        [
            self.x | (self.y << 16),
            self.texture.param,
            self.texture.palette,
            self.texture.levels(),
        ]
    }
}

#[derive(Debug, Default)]
pub struct Atlas {
    indices: HashMap<Texture, usize>,
    textures: Vec<Texture>,
    places: Vec<Option<Place>>,
    placed: Vec<Place>,
    last: Vec<Place>,
    height: u32,
}

impl Atlas {
    pub fn clear(&mut self) {
        self.indices.clear();
        self.textures.clear();
        std::mem::swap(&mut self.last, &mut self.placed);
        self.placed.clear();
    }

    pub fn add(&mut self, param: u32, palette: u32) -> Option<usize> {
        let texture = Texture::new(param, palette)?;
        Some(*self.indices.entry(texture).or_insert_with(|| {
            self.textures.push(texture);
            self.textures.len() - 1
        }))
    }

    pub fn place(&mut self, limit: u32) {
        let mut order: Vec<usize> = (0..self.textures.len()).collect();
        order.sort_by_key(|&index| {
            let texture = self.textures[index];
            (
                std::cmp::Reverse(texture.height()),
                std::cmp::Reverse(texture.width()),
                texture,
            )
        });
        self.places.clear();
        self.places.resize(self.textures.len(), None);
        let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
        for index in order {
            let texture = self.textures[index];
            let (width, height) = (texture.width(), texture.height());
            let align = width.min(height);
            let mut at = x.next_multiple_of(align);
            if shelf == 0 || at + width > WIDTH {
                y += shelf;
                shelf = height;
                at = 0;
            }
            if y + height > limit {
                continue;
            }
            x = at + width;
            let place = Place { texture, x: at, y };
            self.places[index] = Some(place);
            self.placed.push(place);
        }
        self.height = (y + shelf)
            .next_power_of_two()
            .clamp(MIN_HEIGHT, limit.max(MIN_HEIGHT));
    }

    pub fn word(&self, index: usize) -> u32 {
        self.places[index].map_or(0, Place::word)
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn len(&self) -> u32 {
        self.placed.len() as u32
    }

    pub fn levels(&self) -> u32 {
        self.placed
            .iter()
            .map(|place| place.texture.levels())
            .max()
            .unwrap_or(0)
    }

    pub fn moved(&self) -> bool {
        self.placed != self.last
    }

    pub fn entries(&self) -> impl Iterator<Item = [u32; ENTRY_WORDS]> + '_ {
        self.placed.iter().map(|place| place.entry())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(format: u32, offset: u32, width_code: u32, height_code: u32) -> u32 {
        offset >> 3 | width_code << 20 | height_code << 23 | format << FORMAT_SHIFT
    }

    fn placed(atlas: &Atlas) -> Vec<(u32, u32, u32, u32)> {
        atlas
            .placed
            .iter()
            .map(|place| {
                (
                    place.x,
                    place.y,
                    place.texture.width(),
                    place.texture.height(),
                )
            })
            .collect()
    }

    #[test]
    fn one_texture_with_two_wrap_modes_takes_one_place() {
        let mut atlas = Atlas::default();
        let plain = atlas.add(param(3, 0, 2, 2), 1);
        let repeated = atlas.add(param(3, 0, 2, 2) | 0xF << 16, 1);
        assert_eq!(plain, repeated);
        assert_eq!(atlas.add(param(3, 0, 2, 2), 2), Some(1));
        assert_eq!(
            atlas.add(param(7, 0, 2, 2), 1),
            atlas.add(param(7, 0, 2, 2), 2)
        );
        assert_eq!(atlas.add(param(0, 0, 2, 2), 1), None);
    }

    #[test]
    fn every_place_is_aligned_to_its_smaller_side() {
        let mut atlas = Atlas::default();
        for (index, (width, height)) in [(0, 3), (5, 5), (2, 0), (7, 1), (1, 4), (3, 3), (0, 0)]
            .into_iter()
            .cycle()
            .take(60)
            .enumerate()
        {
            atlas.add(param(4, index as u32 * 0x800, width, height), 0);
        }
        atlas.place(4096);
        assert_eq!(atlas.len(), 60);
        let places = placed(&atlas);
        for &(x, y, width, height) in &places {
            let align = width.min(height);
            assert_eq!((x % align, y % align), (0, 0), "{x},{y} {width}x{height}");
            assert!(x + width <= WIDTH && y + height <= atlas.height());
        }
        for (index, a) in places.iter().enumerate() {
            for b in &places[index + 1..] {
                let apart =
                    a.0 + a.2 <= b.0 || b.0 + b.2 <= a.0 || a.1 + a.3 <= b.1 || b.1 + b.3 <= a.1;
                assert!(apart, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn textures_past_the_limit_stay_unplaced() {
        let mut atlas = Atlas::default();
        for index in 0..5 {
            atlas.add(param(4, index * 0x1000, 7, 7), 0);
        }
        atlas.place(2048);
        assert_eq!(atlas.len(), 4);
        assert_eq!((0..5).filter(|&index| atlas.word(index) == 0).count(), 1);
        assert_eq!(atlas.height(), 2048);
    }

    #[test]
    fn the_same_textures_in_another_order_keep_their_places() {
        let textures = [
            param(4, 0, 3, 3),
            param(2, 0x400, 1, 2),
            param(7, 0x800, 4, 1),
        ];
        let mut atlas = Atlas::default();
        for texture in textures {
            atlas.add(texture, 3);
        }
        atlas.place(2048);
        atlas.clear();
        for texture in textures.into_iter().rev() {
            atlas.add(texture, 3);
        }
        atlas.place(2048);
        assert!(!atlas.moved());
        atlas.clear();
        atlas.add(textures[0], 4);
        atlas.place(2048);
        assert!(atlas.moved());
    }
}
