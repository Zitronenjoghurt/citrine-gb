@group(0) @binding(0) var three_d: texture_2d<f32>;
@group(0) @binding(1) var layers: texture_2d<u32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn rgb(color: u32) -> vec3<f32> {
    return wide(vec3<u32>(color & 63u, (color >> 6u) & 63u, (color >> 12u) & 63u));
}

fn wide(channels: vec3<u32>) -> vec3<f32> {
    return vec3<f32>((channels << vec3<u32>(2u)) | (channels >> vec3<u32>(4u))) / 255.0;
}

fn six(color: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(round(color * 63.0));
}

fn shade(color: vec3<f32>, code: u32, up: u32, down: u32) -> vec3<f32> {
    let kind = code & 3u;
    if kind == 0u {
        return color;
    }
    let channels = six(color);
    let factor = vec3<u32>(code >> 2u);
    let four = vec3<u32>(4u);
    if kind == 1u {
        return wide(channels + (((vec3<u32>(63u) - channels) * factor + vec3<u32>(up)) >> four));
    }
    return wide(channels - ((channels * factor + vec3<u32>(down)) >> four));
}

fn finish(color: vec3<f32>, mix: u32) -> vec4<f32> {
    return vec4<f32>(shade(color, (mix >> 24u) & 127u, 0u, 15u), 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<u32>(position.xy);
    let scale = textureDimensions(three_d).x / 256u;
    let layer = textureLoad(layers, at / scale, 0);
    let mix = layer.w & 255u;
    if mix == 0u {
        return vec4<f32>(rgb(layer.x), 1.0);
    }
    let x = (at.x + ((layer.w >> 8u) & 511u) * scale) % (512u * scale);
    if x >= 256u * scale {
        return finish(rgb(layer.y), layer.w);
    }
    let up = textureLoad(three_d, vec2<u32>(x, at.y), 0);
    if up.a <= 0.0 {
        return finish(rgb(layer.y), layer.w);
    }
    if mix == 1u {
        return finish(shade(up.rgb, (layer.w >> 17u) & 127u, 8u, 7u), layer.w);
    }
    let alpha = round(up.a * 31.0);
    let mixed = (up.rgb * (alpha + 1.0) + rgb(layer.z) * (31.0 - alpha)) / 32.0;
    return finish(mixed, layer.w);
}
