mod common;

use citrine_gb::nds::render::Pixel;
use citrine_gb_nds_gpu::{Antialiasing, Mipmaps, Picture, Textures, Upscale};
use common::scenes::{FLOOR_DIRECT, FLOOR_FOUR_COLORS, FLOOR_PALETTE_256, FLOOR_SIZE_8, floor};
use common::{Gpu, Memory, Scene, rgb, scenes, texture_param};

const BLACK: Pixel = Pixel {
    r: 0,
    g: 0,
    b: 0,
    alpha: 0,
};

fn upscale(scale: u32, textures: Textures, mipmaps: bool) -> Upscale {
    Upscale {
        scale,
        antialiasing: Antialiasing::Off,
        textures,
        mipmaps: if mipmaps {
            Mipmaps::Anisotropic4x
        } else {
            Mipmaps::Off
        },
    }
}

fn direct_texture(at: u32, texel: impl Fn(u32, u32) -> u16) -> Memory {
    let mut memory = Memory::default();
    fill_direct(&mut memory, at, texel);
    memory
}

fn fill_direct(memory: &mut Memory, at: u32, texel: impl Fn(u32, u32) -> u16) {
    for t in 0..8 {
        for s in 0..8 {
            memory.set_texture_u16(at + (t * 8 + s) * 2, texel(s, t));
        }
    }
}

fn direct(at: u32) -> u32 {
    texture_param(FLOOR_DIRECT, at, FLOOR_SIZE_8, true, false)
}

fn checker_floor() -> Scene {
    let memory = direct_texture(0, |s, t| {
        if (s + t) % 2 == 0 {
            rgb(31, 0, 0) | 0x8000
        } else {
            rgb(0, 0, 31) | 0x8000
        }
    });
    floor("checker floor", memory, &[(direct(0), 0)])
}

fn floor_rows(picture: &Picture) -> Vec<usize> {
    (0..picture.height())
        .filter(|&y| (0..picture.width()).any(|x| picture.pixel(x, y) != BLACK))
        .collect()
}

fn far_rows(picture: &Picture, scale: u32) -> Vec<usize> {
    let rows = floor_rows(picture);
    let far = rows.len() / (3 * scale as usize);
    rows[2..far].to_vec()
}

fn row_pixels(picture: &Picture, rows: &[usize]) -> Vec<Pixel> {
    let width = picture.width();
    let margin = width / 8;
    rows.iter()
        .flat_map(|&y| (margin..width - margin).map(move |x| (x, y)))
        .map(|(x, y)| picture.pixel(x, y))
        .filter(|&pixel| pixel != BLACK)
        .collect()
}

#[test]
fn native_output_ignores_mipmaps() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let mut all = scenes::all();
    all.push(checker_floor());
    for scene in all {
        for textures in [Textures::Nearest, Textures::Smooth] {
            let plain = gpu.render_with(&scene, upscale(1, textures, false));
            let mipmapped = gpu.render_with(&scene, upscale(1, textures, true));
            for y in 0..plain.height() {
                for x in 0..plain.width() {
                    assert_eq!(
                        (plain.pixel(x, y), plain.depth(x, y), plain.attribute(x, y)),
                        (
                            mipmapped.pixel(x, y),
                            mipmapped.depth(x, y),
                            mipmapped.attribute(x, y)
                        ),
                        "{} {textures:?} at ({x}, {y})",
                        scene.name
                    );
                }
            }
        }
    }
}

#[test]
fn far_checkered_floor_blends_to_one_colour() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = checker_floor();
    for textures in [Textures::Nearest, Textures::Smooth] {
        for scale in [2, 4, 8] {
            let plain = gpu.render_with(&scene, upscale(scale, textures, false));
            let rows = far_rows(&plain, scale);
            let speckled = row_pixels(&plain, &rows)
                .into_iter()
                .filter(|pixel| pixel.r.abs_diff(pixel.b) > 40)
                .count();
            assert!(
                speckled > 100,
                "{textures:?} {scale}x: today's far floor did not alias"
            );
            let mipmapped = gpu.render_with(&scene, upscale(scale, textures, true));
            let far = row_pixels(&mipmapped, &rows);
            assert!(far.len() > 1000, "{textures:?} {scale}x: no far floor");
            let apart: Vec<Pixel> = far
                .into_iter()
                .filter(|pixel| pixel.r.abs_diff(pixel.b) > 20 || pixel.r < 12 || pixel.b < 12)
                .take(5)
                .collect();
            assert!(
                apart.is_empty(),
                "{textures:?} {scale}x: unblended {apart:?}"
            );
        }
    }
}

#[test]
fn near_floor_stays_as_today() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = checker_floor();
    for textures in [Textures::Nearest, Textures::Smooth] {
        let plain = gpu.render_with(&scene, upscale(4, textures, false));
        let mipmapped = gpu.render_with(&scene, upscale(4, textures, true));
        let rows = floor_rows(&plain);
        let near = &rows[rows.len() * 3 / 4..];
        let width = plain.width();
        let changed = near
            .iter()
            .flat_map(|&y| (0..width).map(move |x| (x, y)))
            .filter(|&(x, y)| plain.pixel(x, y) != mipmapped.pixel(x, y))
            .count();
        assert_eq!(changed, 0, "{textures:?}: near floor pixels changed");
    }
}

fn far_colour(gpu: &mut Gpu, scene: &Scene) -> (u32, u32) {
    let picture = gpu.render_with(scene, upscale(4, Textures::Nearest, true));
    let far = row_pixels(&picture, &far_rows(&picture, 4));
    assert!(!far.is_empty(), "no far floor");
    let red = far.iter().map(|pixel| u32::from(pixel.r)).sum::<u32>() / far.len() as u32;
    let blue = far.iter().map(|pixel| u32::from(pixel.b)).sum::<u32>() / far.len() as u32;
    (red, blue)
}

#[test]
fn mipmaps_follow_texture_writes() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let red = |_, _| rgb(31, 0, 0) | 0x8000;
    let blue = |_, _| rgb(0, 0, 31) | 0x8000;
    let mut scene = floor("direct floor", direct_texture(0, red), &[(direct(0), 0)]);
    assert_eq!(far_colour(&mut gpu, &scene), (63, 0));
    fill_direct(&mut scene.memory, 0, blue);
    assert_eq!(far_colour(&mut gpu, &scene), (0, 63));
}

#[test]
fn mipmaps_follow_palette_writes() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let mut memory = Memory::default();
    memory.texture[..64].fill(1);
    memory.set_color(2, rgb(31, 0, 0));
    let param = texture_param(FLOOR_PALETTE_256, 0, FLOOR_SIZE_8, true, false);
    let mut scene = floor("palette floor", memory, &[(param, 0)]);
    assert_eq!(far_colour(&mut gpu, &scene), (63, 0));
    scene.memory.set_color(2, rgb(0, 0, 31));
    assert_eq!(far_colour(&mut gpu, &scene), (0, 63));
}

#[test]
fn neighbouring_textures_never_bleed_into_each_other() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let mut memory = direct_texture(0, |_, _| rgb(31, 0, 0) | 0x8000);
    fill_direct(&mut memory, 0x80, |_, _| rgb(0, 0, 31) | 0x8000);
    let flipped = texture_param(FLOOR_DIRECT, 0x80, FLOOR_SIZE_8, true, true);
    let clamped = texture_param(FLOOR_DIRECT, 0, FLOOR_SIZE_8, false, false);
    let scene = floor(
        "two floors",
        memory,
        &[
            (direct(0), 0),
            (flipped, 0),
            (clamped, 0),
            (direct(0x80), 0),
        ],
    );
    for textures in [Textures::Nearest, Textures::Smooth] {
        let picture = gpu.render_with(&scene, upscale(4, textures, true));
        let mixed: Vec<Pixel> = row_pixels(&picture, &far_rows(&picture, 4))
            .into_iter()
            .filter(|pixel| pixel.r != 0 && pixel.b != 0 || pixel.g != 0)
            .take(5)
            .collect();
        assert!(mixed.is_empty(), "{textures:?}: mixed colours {mixed:?}");
    }
}

fn striped_floors() -> Vec<Scene> {
    let striped = direct_texture(0, |s, _| {
        if s % 2 == 0 {
            rgb(31, 31, 31) | 0x8000
        } else {
            0
        }
    });
    let mut paletted = Memory::default();
    for t in 0..8usize {
        paletted.texture[t * 2] = 0b0001_0001;
        paletted.texture[t * 2 + 1] = 0b0001_0001;
    }
    paletted.set_color(2, rgb(31, 31, 31));
    let four = texture_param(FLOOR_FOUR_COLORS, 0, FLOOR_SIZE_8, true, false) | 1 << 29;
    vec![
        floor("striped direct floor", striped, &[(direct(0), 0)]),
        floor("striped four-colour floor", paletted, &[(four, 0)]),
    ]
}

#[test]
fn cut_out_floors_keep_their_holes_and_colour() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    for scene in striped_floors() {
        for scale in [2, 4] {
            let plain = gpu.render_with(&scene, upscale(scale, Textures::Nearest, false));
            let mipmapped = gpu.render_with(&scene, upscale(scale, Textures::Nearest, true));
            let (mut drawn, mut moved, mut tinted) = (0, Vec::new(), Vec::new());
            for y in 0..plain.height() {
                for x in 0..plain.width() {
                    let (before, after) = (plain.pixel(x, y), mipmapped.pixel(x, y));
                    if (before == BLACK) != (after == BLACK) && moved.len() < 5 {
                        moved.push((x, y));
                    }
                    if after != BLACK {
                        drawn += 1;
                        if [after.r, after.g, after.b] != [63; 3] && tinted.len() < 5 {
                            tinted.push(((x, y), after));
                        }
                    }
                }
            }
            assert!(drawn > 1000, "{} {scale}x: nothing drawn", scene.name);
            assert!(
                moved.is_empty(),
                "{} {scale}x: holes moved at {moved:?}",
                scene.name
            );
            assert!(
                tinted.is_empty(),
                "{} {scale}x: tinted {tinted:?}",
                scene.name
            );
        }
    }
}
