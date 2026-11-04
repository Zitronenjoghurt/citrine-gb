use crate::game_info::Game;
use citrine_gb_gamedata::{Dvs, Generation, Mon, Pokemon, Side};
use egui::TextureHandle;
use std::collections::HashMap;

pub(super) fn sprite_key(generation: Generation, mon: &Mon) -> (u16, bool, char) {
    let shiny = generation != Generation::One && mon.shiny();
    (mon.species, shiny, unown_letter(mon))
}

pub(super) fn pokemon_sprite(pokemon: &Pokemon) -> Option<(u16, bool, char)> {
    match pokemon {
        Pokemon::Hatched(mon) => Some((mon.species, mon.shiny(), unown_letter(mon))),
        Pokemon::Egg => None,
    }
}

fn unown_letter(mon: &Mon) -> char {
    mon.dvs().map_or('A', Dvs::unown_letter)
}

pub(super) type SpriteCache = HashMap<(String, u16, bool, char), Option<TextureHandle>>;

pub(super) fn sprite_texture(
    ctx: &egui::Context,
    key: &str,
    game: &Game,
    species: u16,
    shiny: bool,
    letter: char,
) -> Option<TextureHandle> {
    let shiny = shiny && game.tables.generation() != Generation::One;
    let id = egui::Id::new("game_info_sprites");
    let cache_key = (key.to_owned(), species, shiny, letter);
    if let Some(found) = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SpriteCache>(id)
            .get(&cache_key)
            .cloned()
    }) {
        return found;
    }
    let texture = sprite_image(game, species, shiny, letter).map(|image| {
        ctx.load_texture(
            format!("game_info_{key}_{species}_{shiny}_{letter}"),
            image,
            egui::TextureOptions::NEAREST,
        )
    });
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SpriteCache>(id)
            .insert(cache_key, texture.clone())
    });
    texture
}

pub fn sprite_image(
    game: &Game,
    species: u16,
    shiny: bool,
    letter: char,
) -> Option<egui::ColorImage> {
    let Some(tables) = game.tables.gb() else {
        let picture = game.tables.front_picture(&game.rom, species, shiny)?;
        let side = citrine_gb_gamedata::gen3::PICTURE_SIDE;
        return Some(egui::ColorImage::from_rgba_unmultiplied(
            [side, side],
            &picture.rgba(),
        ));
    };
    let sprite = tables
        .sprite(&game.rom, species, Some(letter), Side::Front)
        .ok()?;
    let palette = tables.palette(species, shiny);
    let rgb = |value: u8| match &palette {
        Some(palette) => palette.rgb(value),
        None => {
            let grey = [0xFF, 0xAA, 0x55, 0x00][usize::from(value & 3)];
            [grey, grey, grey]
        }
    };
    Some(cut_out(
        sprite.width_px(),
        sprite.height_px(),
        |x, y| sprite.pixel(x, y),
        rgb,
    ))
}

pub(super) fn cut_out(
    w: usize,
    h: usize,
    shade: impl Fn(usize, usize) -> u8,
    rgb: impl Fn(u8) -> [u8; 3],
) -> egui::ColorImage {
    let mut outside = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        if outside[y * w + x] || shade(x, y) != 0 {
            continue;
        }
        outside[y * w + x] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = rgb(shade(x, y));
            let alpha = if outside[y * w + x] { 0 } else { 0xFF };
            rgba.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}
