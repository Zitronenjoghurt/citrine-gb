mod common;

use common::{Gpu, Scene, compare_exact, scenes};

fn matches_the_cpu_renderer(scene: Scene) {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let picture = gpu.render(&scene, 1);
    let mismatch = compare_exact(&scene.cpu(), &picture);
    assert_eq!(
        mismatch.count, 0,
        "{}: {} pixels differ, first {:#?}",
        scene.name, mismatch.count, mismatch.first
    );
}

#[test]
fn gouraud_shapes_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::shapes());
}

#[test]
fn w_buffered_shapes_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::w_buffer());
}

#[test]
fn all_seven_texture_formats_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::textures());
}

#[test]
fn translucency_and_polygon_ids_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::translucency());
}

#[test]
fn shadow_polygons_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::shadows());
}

#[test]
fn wireframe_and_equal_depth_match_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::wireframe_and_equal_depth());
}

#[test]
fn the_rear_plane_matches_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::rear_plane());
}

#[test]
fn edge_marking_matches_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::edge_marked());
}

#[test]
fn fog_matches_the_cpu_renderer_exactly() {
    matches_the_cpu_renderer(scenes::fogged());
    matches_the_cpu_renderer(scenes::fogged_alpha());
}

#[test]
fn one_renderer_follows_texture_memory_changes_between_frames() {
    let Some(mut gpu) = Gpu::open() else {
        return;
    };
    let mut scene = scenes::textures();
    for round in 0..4u8 {
        if round == 2 {
            for byte in scene
                .memory
                .texture
                .iter_mut()
                .chain(scene.memory.palette.iter_mut())
            {
                *byte = byte.wrapping_mul(3).wrapping_add(0x21);
            }
        }
        let picture = gpu.render(&scene, 1);
        let mismatch = compare_exact(&scene.cpu(), &picture);
        assert_eq!(
            mismatch.count, 0,
            "round {round}: {} pixels differ, first {:#?}",
            mismatch.count, mismatch.first
        );
    }
}
