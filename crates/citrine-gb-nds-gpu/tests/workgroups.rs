mod common;

use citrine_gb_nds_gpu::{Antialiasing, Mipmaps, Textures, Upscale};
use common::{Gpu, scenes};

const UPSCALES: [Upscale; 3] = [
    Upscale {
        scale: 1,
        antialiasing: Antialiasing::Off,
        textures: Textures::Nearest,
        mipmaps: Mipmaps::Off,
    },
    Upscale {
        scale: 4,
        antialiasing: Antialiasing::Four,
        textures: Textures::Smooth,
        mipmaps: Mipmaps::Anisotropic4x,
    },
    Upscale {
        scale: 3,
        antialiasing: Antialiasing::Eight,
        textures: Textures::Nearest,
        mipmaps: Mipmaps::Anisotropic4x,
    },
];

#[test]
fn compute_passes_write_workgroup_memory_before_reading_it() {
    let (Some(mut skipped), Some(zeroed)) = (Gpu::open(), Gpu::open()) else {
        return;
    };
    let mut zeroed = zeroed.zeroing_workgroups();
    let mut all = scenes::all();
    all.extend(scenes::meshes());
    all.push(scenes::halo(0));
    all.push(scenes::cut_out());
    for scene in all {
        for upscale in UPSCALES {
            let fast = skipped.render_with(&scene, upscale);
            let cleared = zeroed.render_with(&scene, upscale);
            assert!(fast == cleared, "{} at {upscale:?}", scene.name);
        }
    }
}
