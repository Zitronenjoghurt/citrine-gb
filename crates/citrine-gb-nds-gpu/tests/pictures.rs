mod common;

use citrine_gb_nds_gpu::{Antialiasing, Textures, Upscale};
use common::{Gpu, cpu_pixels, gpu_pixels, save_png, scenes};
use std::path::Path;

const CROP: usize = 256;

fn crop(pixels: &[[u8; 4]], width: usize, scale: usize, center: [usize; 2]) -> Vec<[u8; 4]> {
    let [left, top] = center.map(|native| (native * 8).saturating_sub(CROP / 2));
    let step = 8 / scale;
    (top..top + CROP)
        .flat_map(|y| (left..left + CROP).map(move |x| (x, y)))
        .map(|(x, y)| pixels[(y / step) * width + x / step])
        .collect()
}

fn save_crop(
    folder: &Path,
    name: &str,
    pixels: &[[u8; 4]],
    width: usize,
    scale: usize,
    center: [usize; 2],
) {
    save_png(
        &folder.join(name),
        CROP,
        CROP,
        &crop(pixels, width, scale, center),
    );
}

#[test]
fn pictures_for_review() {
    let Some(folder) = std::env::var_os("CITRINE_GPU_PICTURES") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).expect("create the picture folder");
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let mut all = scenes::all();
    all.extend(scenes::meshes());
    for scene in &all {
        let name = scene.name.replace(' ', "-");
        let cpu = scene.cpu();
        save_png(
            &folder.join(format!("{name}-cpu-1x.png")),
            256,
            192,
            &cpu_pixels(&cpu),
        );
        for scale in [1, 4] {
            let picture = gpu.render(scene, scale);
            save_png(
                &folder.join(format!("{name}-gpu-{scale}x.png")),
                picture.width(),
                picture.height(),
                &gpu_pixels(&picture),
            );
        }
    }
    let scene = scenes::t_junctions();
    let polygon = &scene.frame.polygons[1];
    let corner = scene.frame.vertex(polygon, 1).screen.map(usize::from);
    let picture = gpu.render(&scene, 8);
    save_crop(
        &folder,
        "seam-gpu-8x-crop.png",
        &gpu_pixels(&picture),
        picture.width(),
        8,
        corner,
    );
    save_crop(
        &folder,
        "seam-cpu-1x-crop.png",
        &cpu_pixels(&scene.cpu()),
        256,
        1,
        corner,
    );
    for (scene, edge) in [
        (scenes::shapes(), [187, 68]),
        (scenes::translucency(), [135, 140]),
    ] {
        let name = scene.name;
        for (antialiasing, label) in [(Antialiasing::Off, "off"), (Antialiasing::Eight, "aa8")] {
            let picture = gpu.render_with(
                &scene,
                Upscale {
                    scale: 4,
                    antialiasing,
                    ..Upscale::default()
                },
            );
            save_png(
                &folder.join(format!("{name}-gpu-4x-{label}.png")),
                picture.width(),
                picture.height(),
                &gpu_pixels(&picture),
            );
            let picture = gpu.render_with(
                &scene,
                Upscale {
                    scale: 8,
                    antialiasing,
                    ..Upscale::default()
                },
            );
            let file = format!("{name}-gpu-8x-{label}-crop.png");
            save_crop(
                &folder,
                &file,
                &gpu_pixels(&picture),
                picture.width(),
                8,
                edge,
            );
        }
        let picture = gpu.render_with(
            &scene,
            Upscale {
                scale: 4,
                antialiasing: Antialiasing::Four,
                ..Upscale::default()
            },
        );
        save_png(
            &folder.join(format!("{name}-gpu-4x-aa4.png")),
            picture.width(),
            picture.height(),
            &gpu_pixels(&picture),
        );
    }
    for scene in [scenes::textures(), scenes::translucency()] {
        let name = scene.name;
        for (textures, label) in [(Textures::Nearest, "nearest"), (Textures::Smooth, "smooth")] {
            let picture = gpu.render_with(
                &scene,
                Upscale {
                    scale: 4,
                    textures,
                    ..Upscale::default()
                },
            );
            save_png(
                &folder.join(format!("{name}-gpu-4x-{label}.png")),
                picture.width(),
                picture.height(),
                &gpu_pixels(&picture),
            );
        }
    }
    let scene = scenes::textures();
    for (textures, label) in [(Textures::Nearest, "nearest"), (Textures::Smooth, "smooth")] {
        let picture = gpu.render_with(
            &scene,
            Upscale {
                scale: 8,
                textures,
                ..Upscale::default()
            },
        );
        save_crop(
            &folder,
            &format!("cut-out-gpu-8x-{label}-crop.png"),
            &gpu_pixels(&picture),
            picture.width(),
            8,
            [212, 132],
        );
    }
}
