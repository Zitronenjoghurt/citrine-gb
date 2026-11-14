mod common;

use citrine_gb::nds::geometry::frame::{Frame, Polygon, SUBPIXEL_BITS};
use citrine_gb_nds_gpu::{Antialiasing, Textures, Upscale};
use common::{Gpu, Scene, scenes};

fn fine_point(frame: &Frame, polygon: &Polygon, index: usize) -> [f32; 2] {
    let unit = (1 << SUBPIXEL_BITS) as f32;
    let vertex = frame.vertex(polygon, index);
    [0, 1].map(|axis| f32::from(vertex.screen[axis]) + f32::from(vertex.subpixel[axis]) / unit)
}

fn fine_points(frame: &Frame) -> Vec<[f32; 2]> {
    frame
        .polygons
        .iter()
        .flat_map(|polygon| {
            (0..usize::from(polygon.count)).map(move |index| fine_point(frame, polygon, index))
        })
        .collect()
}

fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn hull(mut points: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    points.sort_by(|a, b| a.partial_cmp(b).expect("finite points"));
    points.dedup();
    let mut lower: Vec<[f32; 2]> = Vec::new();
    for &point in &points {
        while lower.len() >= 2
            && cross(lower[lower.len() - 2], lower[lower.len() - 1], point) <= 0.0
        {
            lower.pop();
        }
        lower.push(point);
    }
    let mut upper: Vec<[f32; 2]> = Vec::new();
    for &point in points.iter().rev() {
        while upper.len() >= 2
            && cross(upper[upper.len() - 2], upper[upper.len() - 1], point) <= 0.0
        {
            upper.pop();
        }
        upper.push(point);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

fn deep_inside(outline: &[[f32; 2]], point: [f32; 2], depth: f32) -> bool {
    (0..outline.len()).all(|index| {
        let (a, b) = (outline[index], outline[(index + 1) % outline.len()]);
        let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        cross(a, b, point) >= depth * length
    })
}

fn rear_pixels_inside(
    gpu: &mut Gpu,
    scene: &Scene,
    upscale: Upscale,
) -> (usize, usize, Vec<[usize; 2]>) {
    let scale = upscale.scale;
    let picture = gpu.render_with(scene, upscale);
    let outline = hull(fine_points(&scene.frame));
    let pixel = 1.0 / scale as f32;
    let (mut inside, mut holes, mut first) = (0, 0, Vec::new());
    for y in 0..picture.height() {
        for x in 0..picture.width() {
            let point = [x as f32 * pixel, y as f32 * pixel];
            if !deep_inside(&outline, point, 1.5 * pixel) {
                continue;
            }
            inside += 1;
            let color = picture.pixel(x, y);
            if color.r != 0 {
                holes += 1;
                if first.len() < 5 {
                    first.push([x, y]);
                }
            }
        }
    }
    (inside, holes, first)
}

fn shows_no_rear_plane_inside(scene: Scene) {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    for textures in [Textures::Nearest, Textures::Smooth] {
        for antialiasing in [Antialiasing::Off, Antialiasing::Four, Antialiasing::Eight] {
            for scale in [2, 4, 8] {
                let upscale = Upscale {
                    scale,
                    antialiasing,
                    textures,
                };
                let (inside, holes, first) = rear_pixels_inside(&mut gpu, &scene, upscale);
                assert!(
                    inside > 1000 * (scale * scale) as usize,
                    "{} at {scale}x: outline too small",
                    scene.name
                );
                assert_eq!(
                    holes, 0,
                    "{} at {scale}x, antialiasing {antialiasing:?}, textures {textures:?}: {holes} of {inside} pixels show the rear plane, first at {first:?}",
                    scene.name
                );
            }
        }
    }
}

#[test]
fn a_fan_shows_no_rear_plane_between_its_triangles_at_any_scale() {
    shows_no_rear_plane_inside(scenes::fan());
}

#[test]
fn a_strip_shows_no_rear_plane_between_its_triangles_at_any_scale() {
    shows_no_rear_plane_inside(scenes::strip());
}

#[test]
fn a_mesh_with_t_junctions_shows_no_rear_plane_at_any_scale() {
    shows_no_rear_plane_inside(scenes::t_junctions());
}

#[test]
fn a_floor_clipped_by_the_near_plane_shows_no_rear_plane_at_any_scale() {
    shows_no_rear_plane_inside(scenes::clipped_floor());
}

#[test]
fn silhouettes_follow_the_true_outline_instead_of_native_steps() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let scene = scenes::halo(0);
    let outlines: Vec<Vec<[f32; 2]>> = scene
        .frame
        .polygons
        .iter()
        .map(|polygon| {
            hull(
                (0..usize::from(polygon.count))
                    .map(|index| fine_point(&scene.frame, polygon, index))
                    .collect(),
            )
        })
        .collect();
    for scale in [2, 4, 8] {
        let picture = gpu.render(&scene, scale);
        let pixel = 1.0 / scale as f32;
        let (mut drawn, mut beyond, mut first) = (0, 0, Vec::new());
        for y in 0..picture.height() {
            for x in 0..picture.width() {
                if picture.pixel(x, y).alpha == 0 {
                    continue;
                }
                drawn += 1;
                let point = [x as f32 * pixel, y as f32 * pixel];
                if !outlines
                    .iter()
                    .any(|outline| deep_inside(outline, point, -0.25))
                {
                    beyond += 1;
                    if first.len() < 5 {
                        first.push([x, y]);
                    }
                }
            }
        }
        assert!(
            drawn > 2000 * (scale * scale) as usize,
            "halo at {scale}x draws too little"
        );
        assert_eq!(
            beyond, 0,
            "halo at {scale}x: {beyond} pixels lie over a quarter pixel outside every polygon, first at {first:?}"
        );
    }
}

#[test]
fn a_textured_fan_shows_no_rear_plane_between_its_triangles_at_any_scale() {
    shows_no_rear_plane_inside(scenes::textured_fan());
}

#[test]
fn a_textured_mesh_with_t_junctions_shows_no_rear_plane_at_any_scale() {
    shows_no_rear_plane_inside(scenes::textured_t_junctions());
}

fn band_at(frame: &Frame, point: [f32; 2]) -> Option<(usize, f32)> {
    frame
        .polygons
        .iter()
        .enumerate()
        .find_map(|(index, polygon)| {
            let points: Vec<[f32; 2]> = (0..usize::from(polygon.count))
                .map(|slot| fine_point(frame, polygon, slot))
                .collect();
            let top = points.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
            let bottom = points.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
            (point[1] >= top && point[1] < bottom)
                .then(|| (index, (point[1] - top).min(bottom - point[1])))
        })
}

#[test]
fn a_band_keeps_its_own_color_next_to_the_band_above_at_any_scale() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    for tilt in [-40.0, 40.0] {
        let scene = scenes::bands(tilt);
        let colors: Vec<u8> = scene
            .frame
            .polygons
            .iter()
            .map(|polygon| scene.frame.vertex(polygon, 0).color[1])
            .collect();
        for scale in [2, 4, 8] {
            let picture = gpu.render(&scene, scale);
            let pixel = 1.0 / scale as f32;
            let (mut checked, mut wrong, mut first) = (0, 0, Vec::new());
            for y in 0..picture.height() {
                for x in picture.width() * 2 / 5..picture.width() * 3 / 5 {
                    let point = [x as f32 * pixel, y as f32 * pixel];
                    let Some((band, inset)) = band_at(&scene.frame, point) else {
                        continue;
                    };
                    if inset < 0.001 {
                        continue;
                    }
                    checked += 1;
                    let green = picture.pixel(x, y).g > picture.pixel(x, y).r;
                    if green != (colors[band] > 0) {
                        wrong += 1;
                        if first.len() < 5 {
                            first.push([x, y]);
                        }
                    }
                }
            }
            assert!(
                checked > 1000,
                "bands tilted {tilt} at {scale}x: too few pixels"
            );
            assert_eq!(
                wrong, 0,
                "bands tilted {tilt} at {scale}x: {wrong} of {checked} pixels show the band above or below, first at {first:?}"
            );
        }
    }
}
