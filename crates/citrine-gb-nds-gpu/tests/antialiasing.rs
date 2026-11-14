mod common;

use citrine_gb::nds::render::{Pixel, Renderer as CpuRenderer};
use citrine_gb::nds::video::{HEIGHT, WIDTH};
use citrine_gb_nds_gpu::{Antialiasing, Picture, Upscale};
use common::{Gpu, compare_exact, rgb, scenes};
use scenes::MESH_REAR;

const SMOOTHED: [Antialiasing; 2] = [Antialiasing::Four, Antialiasing::Eight];

fn channels(pixel: Pixel) -> [u8; 3] {
    [pixel.r, pixel.g, pixel.b]
}

fn every_pixel(picture: &Picture) -> impl Iterator<Item = Pixel> + '_ {
    (0..picture.height()).flat_map(move |y| (0..picture.width()).map(move |x| picture.pixel(x, y)))
}

fn polygon_color(cpu: &CpuRenderer) -> [u8; 3] {
    let rear = cpu.pixel(0, 0);
    let mut counts = std::collections::HashMap::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel = cpu.pixel(x, y);
            if pixel != rear {
                *counts.entry(channels(pixel)).or_insert(0) += 1;
            }
        }
    }
    let (color, count) = counts
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .expect("the halo scene draws its polygons");
    assert!(count > 2000, "the halo scene covers only {count} pixels");
    color
}

#[test]
fn antialiasing_stays_off_at_1x() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    for scene in scenes::all() {
        let picture = gpu.render_with(
            &scene,
            Upscale {
                scale: 1,
                antialiasing: Antialiasing::Eight,
                ..Upscale::default()
            },
        );
        let mismatch = compare_exact(&scene.cpu(), &picture);
        assert_eq!(mismatch.count, 0, "{}: {:#?}", scene.name, mismatch.first);
    }
}

#[test]
fn smoothed_edges_only_mix_the_polygon_and_the_rear_plane() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::halo(u32::from(rgb(20, 24, 31)) | 31 << 16);
    let cpu = scene.cpu();
    let rear = channels(cpu.pixel(0, 0));
    let polygon = polygon_color(&cpu);
    for scale in [2, 4, 8] {
        for antialiasing in SMOOTHED {
            let picture = gpu.render_with(
                &scene,
                Upscale {
                    scale,
                    antialiasing,
                    ..Upscale::default()
                },
            );
            let (mut mixed, mut outside) = (0, 0);
            for pixel in every_pixel(&picture) {
                let color = channels(pixel);
                if color != rear && color != polygon {
                    mixed += 1;
                }
                let within = (0..3).all(|channel| {
                    let (low, high) = (
                        rear[channel].min(polygon[channel]),
                        rear[channel].max(polygon[channel]),
                    );
                    (low.saturating_sub(1)..=high + 1).contains(&color[channel])
                });
                if !within {
                    outside += 1;
                }
            }
            assert_eq!(
                outside, 0,
                "{scale}x {antialiasing:?}: {outside} pixels leave the range between the two colours"
            );
            assert!(
                mixed > 200 * scale as usize,
                "{scale}x {antialiasing:?}: only {mixed} edge pixels were smoothed"
            );
        }
    }
}

#[test]
fn smoothed_edges_over_a_transparent_rear_plane_keep_the_polygon_colour() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::halo(0);
    let polygon = polygon_color(&scene.cpu());
    for scale in [2, 4, 8] {
        for antialiasing in SMOOTHED {
            let picture = gpu.render_with(
                &scene,
                Upscale {
                    scale,
                    antialiasing,
                    ..Upscale::default()
                },
            );
            let (mut partial, mut darker) = (0, 0);
            for pixel in every_pixel(&picture) {
                if pixel.alpha == 0 {
                    continue;
                }
                if pixel.alpha < 31 {
                    partial += 1;
                }
                let color = channels(pixel);
                if (0..3).any(|channel| color[channel].abs_diff(polygon[channel]) > 1) {
                    darker += 1;
                }
            }
            assert_eq!(
                darker, 0,
                "{scale}x {antialiasing:?}: {darker} pixels drift from the polygon colour"
            );
            assert!(
                partial > 200 * scale as usize,
                "{scale}x {antialiasing:?}: only {partial} pixels have partial cover"
            );
        }
    }
}

fn expanded(color: u16) -> [u8; 3] {
    [0, 5, 10].map(|shift| {
        let channel = (color >> shift & 31) as u8;
        if channel == 0 { 0 } else { channel * 2 + 1 }
    })
}

fn near_rear(picture: &Picture, x: usize, y: usize, rear: [u8; 3]) -> bool {
    let (width, height) = (picture.width(), picture.height());
    (y.saturating_sub(2)..(y + 3).min(height)).any(|ny| {
        (x.saturating_sub(2)..(x + 3).min(width)).any(|nx| channels(picture.pixel(nx, ny)) == rear)
    })
}

#[test]
fn smoothing_leaves_texels_and_seams_inside_a_mesh_alone() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::textured_fan();
    let rear = expanded(MESH_REAR);
    let [sharp, four, eight] =
        [Antialiasing::Off, Antialiasing::Four, Antialiasing::Eight].map(|antialiasing| {
            gpu.render_with(
                &scene,
                Upscale {
                    scale: 4,
                    antialiasing,
                    ..Upscale::default()
                },
            )
        });
    let (mut rim, mut inside) = (0, 0);
    for y in 0..sharp.height() {
        for x in 0..sharp.width() {
            if four.pixel(x, y) == eight.pixel(x, y) {
                continue;
            }
            if near_rear(&sharp, x, y, rear) {
                rim += 1;
            } else {
                inside += 1;
            }
        }
    }
    assert_eq!(
        inside, 0,
        "{inside} pixels inside the fan differ between Four and Eight"
    );
    assert!(
        rim > 200,
        "only {rim} rim pixels differ between Four and Eight"
    );
}

#[test]
fn smoothing_covers_a_depth_jump_within_one_polygon_id() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::stacked();
    let sharp = gpu.render_with(
        &scene,
        Upscale {
            scale: 4,
            ..Upscale::default()
        },
    );
    let rear = expanded(MESH_REAR);
    let front = channels(sharp.pixel(sharp.width() / 2, sharp.height() / 2));
    let mut counts = std::collections::HashMap::new();
    for pixel in every_pixel(&sharp) {
        *counts.entry(channels(pixel)).or_insert(0) += 1;
    }
    let (back, count) = counts
        .into_iter()
        .filter(|&(color, _)| color != rear && color != front)
        .max_by_key(|&(_, count)| count)
        .expect("the back quad shows");
    assert!(count > 10_000, "the back quad shows only {count} pixels");
    let known = [rear, back, front];
    let mut border = 0;
    for y in 1..sharp.height() - 1 {
        for x in 1..sharp.width() - 1 {
            let touches_back = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .into_iter()
                .any(|(nx, ny)| channels(sharp.pixel(nx, ny)) == back);
            if channels(sharp.pixel(x, y)) == front && touches_back {
                border += 1;
            }
        }
    }
    assert!(
        border > 200,
        "the front quad's border has only {border} pixels"
    );
    for antialiasing in SMOOTHED {
        let picture = gpu.render_with(
            &scene,
            Upscale {
                scale: 4,
                antialiasing,
                ..Upscale::default()
            },
        );
        let mut mixed = 0;
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                let color = channels(picture.pixel(x, y));
                let between_quads = (0..3).all(|channel| {
                    let (back, front) = (known[1][channel], known[2][channel]);
                    (back.min(front)..=back.max(front)).contains(&color[channel])
                });
                if between_quads && !known.contains(&color) && !near_rear(&picture, x, y, known[0])
                {
                    mixed += 1;
                }
            }
        }
        assert!(
            mixed * 3 > border,
            "{antialiasing:?}: only {mixed} pixels mix the front and back quads, border {border}"
        );
    }
}

#[test]
fn a_marked_edge_on_the_top_row_mixes_with_the_polygon_under_it() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::sign_over_floor();
    let cpu = scene.cpu();
    let sign = channels(cpu.pixel(WIDTH / 2, 4));
    let mixed = channels(cpu.pixel(WIDTH / 2, 0));
    assert_eq!(sign, expanded(scenes::SIGN_COLOR));
    assert_ne!(mixed, expanded(scenes::SIGN_EDGE));
    for (scale, antialiasing) in [
        (1, Antialiasing::Off),
        (4, Antialiasing::Off),
        (4, Antialiasing::Four),
    ] {
        let picture = gpu.render_with(
            &scene,
            Upscale {
                scale,
                antialiasing,
                ..Upscale::default()
            },
        );
        let middle = (WIDTH / 2) * scale as usize;
        for row in 0..scale as usize {
            let got = channels(picture.pixel(middle, row));
            assert!(
                (0..3).all(|channel| got[channel].abs_diff(mixed[channel]) <= 1),
                "{scale}x {antialiasing:?} row {row}: {got:?}, the DS shows {mixed:?}"
            );
        }
    }
}
