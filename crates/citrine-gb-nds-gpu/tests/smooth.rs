mod common;

use citrine_gb_nds_gpu::{Antialiasing, Picture, Textures, Upscale};
use common::{Gpu, Scene, scenes};

fn smooth(gpu: &mut Gpu, scene: &Scene, scale: u32) -> Picture {
    gpu.render_with(
        scene,
        Upscale {
            scale,
            antialiasing: Antialiasing::Off,
            textures: Textures::Smooth,
        },
    )
}

#[test]
fn smooth_cut_out_edges_have_no_dark_fringe() {
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
        assert!(partial > 0, "{scale}x: the cut-out edge was not smoothed");
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
