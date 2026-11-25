mod common;

use citrine_gb_nds_gpu::{Antialiasing, Mipmaps, Picture, Textures, Upscale};
use common::{Gpu, Scene, scenes};

fn smooth(gpu: &mut Gpu, scene: &Scene, scale: u32) -> Picture {
    smooth_with(gpu, scene, scale, Textures::Smooth)
}

fn smooth_with(gpu: &mut Gpu, scene: &Scene, scale: u32, textures: Textures) -> Picture {
    gpu.render_with(
        scene,
        Upscale {
            scale,
            antialiasing: Antialiasing::Off,
            textures,
            mipmaps: Mipmaps::Off,
        },
    )
}

#[test]
fn smooth_cut_out_edges_stay_opaque_without_a_dark_fringe() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::cut_out();
    for scale in [1, 4] {
        let picture = smooth(&mut gpu, &scene, scale);
        let (mut partial, mut darker) = (0, Vec::new());
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                let pixel = picture.pixel(x, y);
                if pixel.alpha > 0 && pixel.alpha < 31 {
                    partial += 1;
                }
                if [pixel.r, pixel.g, pixel.b] != [63; 3] && darker.len() < 5 {
                    darker.push(([x, y], pixel));
                }
            }
        }
        assert_eq!(partial, 0, "{scale}x: translucent cut-out pixels");
        assert!(darker.is_empty(), "{scale}x: darker pixels {darker:?}");
    }
}

#[test]
fn nearest_cut_out_edges_stay_hard() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let picture = gpu.render(&scenes::cut_out(), 4);
    let partial = (0..picture.height())
        .flat_map(|y| (0..picture.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| (1..31).contains(&picture.pixel(x, y).alpha))
        .count();
    assert_eq!(partial, 0);
}

#[test]
fn filtered_cut_out_edges_keep_the_edge_marks_of_nearest() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let last_dark =
        |picture: &Picture, y: usize| (0..picture.width()).rfind(|&x| picture.pixel(x, y).r < 32);
    for blending in [false, true] {
        let scene = scenes::edge_marked_cut_out(blending);
        let nearest = gpu.render(&scene, 4);
        for textures in [Textures::Smooth, Textures::Bicubic, Textures::Xbr] {
            let picture = smooth_with(&mut gpu, &scene, 4, textures);
            let (mut partial, mut marked, mut moved) = (0, 0, Vec::new());
            for y in 0..picture.height() {
                partial += (0..picture.width())
                    .filter(|&x| (1..31).contains(&picture.pixel(x, y).alpha))
                    .count();
                match (last_dark(&nearest, y), last_dark(&picture, y)) {
                    (Some(sharp), Some(filtered)) if sharp.abs_diff(filtered) <= 8 => marked += 1,
                    (None, None) => {}
                    pair if moved.len() < 5 => moved.push((y, pair)),
                    _ => {}
                }
            }
            let case = format!("{textures:?}, blending {blending}");
            assert_eq!(partial, 0, "{case}: translucent cut-out pixels");
            assert!(marked > 0, "{case}: no edge marks");
            assert!(
                moved.is_empty(),
                "{case}: rows where the edge mark moved {moved:?}"
            );
        }
    }
}

struct Row {
    mixed: usize,
    red: Vec<usize>,
    blue: Vec<usize>,
}

fn middle_row(picture: &Picture) -> Row {
    let y = picture.height() / 2;
    let mut row = Row {
        mixed: 0,
        red: Vec::new(),
        blue: Vec::new(),
    };
    for x in 0..picture.width() {
        let pixel = picture.pixel(x, y);
        if pixel.alpha == 0 {
            continue;
        }
        if pixel.r > 0 && pixel.b > 0 {
            row.mixed += 1;
        }
        if pixel.r > 0 {
            row.red.push(x);
        }
        if pixel.b > 0 {
            row.blue.push(x);
        }
    }
    row
}

fn wrap_row(repeat: bool, flip: bool) -> Option<Row> {
    let mut gpu = Gpu::open()?;
    let picture = smooth(&mut gpu, &scenes::wrap_edges(repeat, flip), 4);
    let row = middle_row(&picture);
    assert!(
        row.red.len() > 20 && row.blue.len() > 20,
        "the quad is missing from the middle row"
    );
    Some(row)
}

#[test]
fn smooth_repeat_blends_the_last_texel_with_the_first() {
    let Some(row) = wrap_row(true, false) else {
        return;
    };
    assert!(row.mixed > 0, "no pixel mixes the first and last texel");
}

#[test]
fn smooth_clamp_blends_the_edge_texel_with_itself() {
    let Some(row) = wrap_row(false, false) else {
        return;
    };
    assert_eq!(row.mixed, 0);
    let last_red = row.red.iter().max().expect("red pixels");
    let first_blue = row.blue.iter().min().expect("blue pixels");
    assert!(
        last_red < first_blue,
        "red at {last_red} after blue at {first_blue}"
    );
}

#[test]
fn smooth_flip_blends_the_edge_texel_with_its_mirror() {
    let Some(row) = wrap_row(true, true) else {
        return;
    };
    assert_eq!(row.mixed, 0);
    let last_red = row.red.iter().max().expect("red pixels");
    let first_blue = row.blue.iter().min().expect("blue pixels");
    assert!(last_red > first_blue, "the mirrored copy is missing");
}
