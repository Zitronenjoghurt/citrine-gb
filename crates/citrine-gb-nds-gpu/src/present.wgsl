@group(0) @binding(0) var source: texture_2d<u32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_present(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let color = textureLoad(source, vec2<u32>(position.xy), 0).x;
    let rgb = vec3<f32>(f32(color & 0xFFu), f32((color >> 8u) & 0xFFu), f32((color >> 16u) & 0xFFu)) / 63.0;
    return vec4<f32>(rgb, f32(color >> 24u) / 31.0);
}
