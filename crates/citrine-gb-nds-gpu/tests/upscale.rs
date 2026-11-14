mod common;

use citrine_gb::nds::render::Pixel;
use citrine_gb::nds::video::{HEIGHT, WIDTH};
use common::{Gpu, Scene, scenes};

struct Closeness {
    far: usize,
    total: usize,
}

fn level_difference(a: Pixel, b: Pixel) -> u8 {
    [a.r.abs_diff(b.r), a.g.abs_diff(b.g), a.b.abs_diff(b.b)]
        .into_iter()
        .max()
        .unwrap_or(0)
}

fn closeness(gpu: &mut Gpu, scene: &Scene, scale: u32) -> Closeness {
    let cpu = scene.cpu();
    let picture = gpu.render(scene, scale);
    let step = scale as usize;
    let mut far = 0;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let sample = picture.pixel(x * step + step / 2, y * step + step / 2);
            let nearest = (y.saturating_sub(1)..(y + 2).min(HEIGHT))
                .flat_map(|ny| (x.saturating_sub(1)..(x + 2).min(WIDTH)).map(move |nx| (nx, ny)))
                .map(|(nx, ny)| level_difference(cpu.pixel(nx, ny), sample))
                .min()
                .unwrap_or(u8::MAX);
            if nearest > 2 {
                far += 1;
            }
        }
    }
    Closeness {
        far,
        total: WIDTH * HEIGHT,
    }
}

fn stays_close_to_the_cpu_renderer(scene: Scene, percent: f32) {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    for scale in [2, 4, 8] {
        let result = closeness(&mut gpu, &scene, scale);
        let share = result.far as f32 * 100.0 / result.total as f32;
        eprintln!(
            "{} at {scale}x: {share:.2}% far from every CPU pixel around them",
            scene.name
        );
        assert!(
            share <= percent,
            "{} at {scale}x: {share:.2}% far from every CPU pixel around them",
            scene.name
        );
    }
}

#[test]
fn upscaled_gouraud_shapes_stay_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::shapes(), 0.2);
}

#[test]
fn upscaled_textures_stay_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::textures(), 1.5);
}

#[test]
fn upscaled_translucency_stays_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::translucency(), 0.1);
}

#[test]
fn upscaled_shadows_stay_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::shadows(), 0.1);
}

#[test]
fn upscaled_edge_marking_stays_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::edge_marked(), 1.0);
}

#[test]
fn upscaled_fog_stays_close_to_the_cpu_renderer() {
    stays_close_to_the_cpu_renderer(scenes::fogged(), 0.2);
}
