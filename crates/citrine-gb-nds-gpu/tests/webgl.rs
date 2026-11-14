use naga::back::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

fn translates_to_webgl2(source: &str, fragment: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..glsl::Options::default()
    };
    for (stage, entry) in [
        (naga::ShaderStage::Vertex, "vs_main"),
        (naga::ShaderStage::Fragment, fragment),
    ] {
        let pipeline = glsl::PipelineOptions {
            shader_stage: stage,
            entry_point: entry.into(),
            multiview: None,
        };
        let (module, info) = naga::back::pipeline_constants::process_overrides(
            &module,
            &info,
            Some((stage, entry)),
            &naga::back::PipelineConstants::default(),
        )
        .expect("overrides at their defaults");
        let mut out = String::new();
        let mut writer = glsl::Writer::new(
            &mut out,
            &module,
            &info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .expect("a WebGL2 writer");
        writer.write().expect("GLSL ES 3.00 for WebGL2");
        assert!(out.starts_with("#version 300 es"));
    }
}

#[test]
fn the_rasteriser_translates_to_webgl2_glsl() {
    for fragment in ["fs_raster", "fs_centers", "fs_edges", "fs_finish"] {
        translates_to_webgl2(include_str!("../src/raster.wgsl"), fragment);
    }
}

#[test]
fn the_present_pass_translates_to_webgl2_glsl() {
    translates_to_webgl2(include_str!("../src/present.wgsl"), "fs_present");
}
