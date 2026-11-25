mod common;

use citrine_gb_nds_gpu::{Renderer, Route, guarded, probe_on, set_up};
use common::{Gpu, Scene, compare_exact, scenes};

fn lost() -> Option<Gpu> {
    let gpu = Gpu::open()?;
    gpu.device().destroy();
    Some(gpu)
}

#[test]
fn a_lost_device_panics_the_unguarded_set_up_like_the_phone() {
    let Some(gpu) = lost() else {
        return;
    };
    let device = gpu.device();
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Renderer::new(device)));
    let message = built.err().expect("a panic");
    let message = message
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(message.contains("is invalid"), "{message}");
}

#[test]
fn a_lost_device_fails_every_route_without_a_panic() {
    let Some(gpu) = lost() else {
        return;
    };
    for route in Route::ALL {
        assert!(probe_on(gpu.device(), route).is_err(), "{route:?}");
    }
}

#[test]
fn a_set_up_error_comes_back_and_leaves_the_device_working() {
    let Some(gpu) = Gpu::open() else {
        return;
    };
    let device = gpu.device();
    let failed = guarded(device, || {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no usages"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::empty(),
            view_formats: &[],
        })
    });
    assert!(failed.is_err());
    for route in Route::ALL {
        assert!(set_up(device, || Renderer::with_route(device, route)).is_ok());
    }
}

fn fragment_route_matches_the_cpu(scene: Scene) {
    let Some(gpu) = Gpu::open() else {
        return;
    };
    let mut gpu = gpu.on_route(Route::Fragment);
    let picture = gpu.render(&scene, 1);
    let mismatch = compare_exact(&scene.cpu(), &picture);
    assert_eq!(
        mismatch.count, 0,
        "{}: {} pixels differ, first {:#?}",
        scene.name, mismatch.count, mismatch.first
    );
}

#[test]
fn the_fragment_route_draws_shapes_like_the_cpu() {
    fragment_route_matches_the_cpu(scenes::shapes());
}

#[test]
fn the_fragment_route_marks_edges_like_the_cpu() {
    fragment_route_matches_the_cpu(scenes::edge_marked());
}
