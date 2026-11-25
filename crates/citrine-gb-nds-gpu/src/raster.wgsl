struct Uniforms {
    frame: vec4<u32>,
    rear: vec4<u32>,
    offsets: vec4<u32>,
    options: vec4<u32>,
    edge: vec4<u32>,
    fog: vec4<u32>,
    fog_table: array<vec4<u32>, 2>,
    mips: vec4<u32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var data: texture_2d<u32>;
@group(0) @binding(2) var reference: texture_2d<u32>;
@group(0) @binding(3) var centers: texture_2d<u32>;
@group(0) @binding(4) var edged: texture_storage_2d<rgba32uint, write>;
@group(0) @binding(5) var atlas: texture_2d<u32>;

const WIDTH: i32 = 256;
const HEIGHT: i32 = 192;
const TILE: i32 = 8;
const DATA_WIDTH: u32 = 1024u;
const NONE: u32 = 0xFFFFFFFFu;
const NO_ID: u32 = 64u;

const TEXTURES: u32 = 1u;
const ALPHA_TEST: u32 = 4u;
const BLENDING: u32 = 8u;
const ANTIALIASING: u32 = 16u;
const EDGE_MARKING: u32 = 32u;
const FOG_ALPHA_ONLY: u32 = 64u;
const FOG_ENABLE: u32 = 128u;

const TRANSLUCENT_DEPTH: u32 = 2048u;
const DEPTH_EQUAL: u32 = 16384u;
const FOG: u32 = 32768u;
const EQUAL_Z: i32 = 0x200;
const EQUAL_W: i32 = 0xFF;
const OPAQUE: u32 = 31u;
const DECAL: u32 = 1u;
const SHADOW: u32 = 3u;

const LEFT: u32 = 1u;
const RIGHT: u32 = 2u;
const TOP: u32 = 4u;
const BOTTOM: u32 = 8u;

const HEADER: u32 = 12u;
const VERTEX: u32 = 8u;
const ENTRY: u32 = 4u;
const ENTRY_TRANSLUCENT: u32 = 0x80000000u;
const ENTRY_MASK: u32 = 0x40000000u;

const FRACTION: u32 = 18u;
const ONE: i32 = 262144;
const HALF: i32 = 131072;
const LOW_BITS: i32 = 511;

const TEXTURE_SIZE: u32 = 0x80000u;
const PALETTE_SIZE: u32 = 0x18000u;
const SLOT: u32 = 0x20000u;
const COMPRESSED_INDEX_HIGH: u32 = 0x10000u;

const MARGIN: f32 = 0.125;
const PLACE_WORD: u32 = 6u;
const PLACED: u32 = 0x80000000u;
const WIRE: f32 = 0.5;
const HOLE: f32 = 1.0;

const CONTRAST: u32 = 2u;
const EDGE_WEIGHT: u32 = 17u;
const DEPTH_STEP: u32 = 0x200u;
const JUMP: u32 = 0x400u;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn word(index: u32) -> u32 {
    return texel(index)[index & 3u];
}

fn vram(address: u32) -> u32 {
    let wrapped = address & (TEXTURE_SIZE - 1u);
    return (word(u.offsets.y + (wrapped >> 2u)) >> ((wrapped & 3u) * 8u)) & 0xFFu;
}

fn vram16(address: u32) -> u32 {
    return vram(address) | (vram(address + 1u) << 8u);
}

fn palette8(address: u32) -> u32 {
    let wrapped = address % PALETTE_SIZE;
    return (word(u.offsets.z + (wrapped >> 2u)) >> ((wrapped & 3u) * 8u)) & 0xFFu;
}

fn palette16(address: u32) -> u32 {
    return palette8(address) | (palette8(address + 1u) << 8u);
}

struct Poly {
    record: u32,
    attributes: u32,
    texture: u32,
    palette: u32,
    front: bool,
    w_buffer: bool,
    degenerate: bool,
    translucent: bool,
    count: u32,
    top: u32,
    bottom: u32,
    fine_top: u32,
    fine_bottom: u32,
}

fn texel(index: u32) -> vec4<u32> {
    let at = index >> 2u;
    return textureLoad(data, vec2<u32>(at % DATA_WIDTH, at / DATA_WIDTH), 0);
}

fn load(record: u32) -> Poly {
    return poly_of(record, texel(record));
}

fn poly_of(record: u32, head: vec4<u32>) -> Poly {
    let flags = head.z;
    let shape = head.w;
    var p: Poly;
    p.record = record;
    p.attributes = head.x;
    p.texture = head.y;
    p.palette = flags & 0xFFFFu;
    p.front = (flags & 0x10000u) != 0u;
    p.w_buffer = (flags & 0x20000u) != 0u;
    p.degenerate = (flags & 0x40000u) != 0u;
    p.translucent = (flags & 0x80000u) != 0u;
    p.count = shape & 0xFFu;
    p.top = (shape >> 8u) & 0xFFu;
    p.bottom = (shape >> 16u) & 0xFFu;
    p.fine_top = (shape >> 24u) & 0xFu;
    p.fine_bottom = shape >> 28u;
    return p;
}

fn vertex_word(p: Poly, index: u32, field: u32) -> u32 {
    return word(p.record + HEADER + index * VERTEX + field);
}

fn screen(p: Poly, index: u32) -> vec2<i32> {
    let value = vertex_word(p, index, 0u);
    return vec2<i32>(i32(value & 0xFFFFu), i32(value >> 16u));
}

fn fine_of(value: u32) -> vec2<f32> {
    return vec2<f32>(f32(value & 0xFFFFu), f32(value >> 16u)) / 16.0;
}

fn fine(p: Poly, index: u32) -> vec2<f32> {
    return fine_of(vertex_word(p, index, 1u));
}

fn vertex_depth(p: Poly, index: u32) -> i32 {
    return bitcast<i32>(vertex_word(p, index, 2u));
}

fn vertex_w(p: Poly, index: u32) -> i32 {
    return bitcast<i32>(vertex_word(p, index, 3u));
}

fn widen(channel: u32) -> i32 {
    if channel == 0u {
        return 0;
    }
    return i32((channel << 3u) | 7u);
}

fn vertex_color(p: Poly, index: u32) -> vec3<i32> {
    let value = vertex_word(p, index, 4u);
    return vec3<i32>(widen(value & 0xFFu), widen((value >> 8u) & 0xFFu), widen((value >> 16u) & 0xFFu));
}

fn vertex_texcoord(p: Poly, index: u32) -> vec2<i32> {
    let value = vertex_word(p, index, 5u);
    return vec2<i32>(bitcast<i32>(value << 16u) >> 16u, bitcast<i32>(value) >> 16u);
}

fn mode(p: Poly) -> u32 {
    return (p.attributes >> 4u) & 3u;
}

fn polygon_id(attributes: u32) -> u32 {
    return (attributes >> 24u) & 0x3Fu;
}

fn raw_alpha(p: Poly) -> u32 {
    return (p.attributes >> 16u) & 0x1Fu;
}

fn polygon_alpha(p: Poly) -> u32 {
    let alpha = raw_alpha(p);
    if alpha == 0u {
        return OPAQUE;
    }
    return alpha;
}

struct Entry {
    record: u32,
    translucent: bool,
    mask: bool,
    low: u32,
    high: u32,
}

fn entry(start: u32, index: u32) -> Entry {
    let words = texel(u.offsets.x + (start + index) * ENTRY);
    var e: Entry;
    e.record = words.x & ~(ENTRY_TRANSLUCENT | ENTRY_MASK);
    e.translucent = (words.x & ENTRY_TRANSLUCENT) != 0u;
    e.mask = (words.x & ENTRY_MASK) != 0u;
    e.low = words.y;
    e.high = words.z;
    return e;
}

fn on_rows(e: Entry, y: i32) -> bool {
    let top = i32(e.low >> 20u);
    let bottom = i32(e.high >> 20u);
    return (y >= top && y < bottom) || (top == bottom && y == top);
}

fn on_columns(e: Entry, x: i32) -> bool {
    let left = i32((e.low & 0xFFFFu) >> 4u);
    let right = i32((e.high & 0xFFFFu) >> 4u);
    return x >= left - 3 && x <= right + 3;
}

fn reaches(e: Entry, point: vec2<f32>, slack: f32) -> bool {
    let low = e.low;
    let high = e.high;
    let lowest = vec2<f32>(f32(low & 0xFFFFu), f32(low >> 16u)) / 16.0 - slack;
    let highest = vec2<f32>(f32(high & 0xFFFFu), f32(high >> 16u)) / 16.0 + slack;
    return all(point >= lowest) && all(point <= highest);
}

fn near_slack(e: Entry, pixel: f32) -> f32 {
    if (e.low >> 16u) == (e.high >> 16u) {
        return pixel + MARGIN;
    }
    return MARGIN + 1.0 / 64.0;
}

fn mask_clears(record: u32, row: u32) -> bool {
    return ((word(record + 4u + (row >> 5u)) >> (row & 31u)) & 1u) != 0u;
}

fn step_index(p: Poly, index: u32, forward: bool) -> u32 {
    if forward {
        return (index + 1u) % p.count;
    }
    return (index + p.count - 1u) % p.count;
}

fn mul64(a: u32, b: u32) -> vec2<u32> {
    let a0 = a & 0xFFFFu;
    let a1 = a >> 16u;
    let b0 = b & 0xFFFFu;
    let b1 = b >> 16u;
    let low = a0 * b0;
    let cross0 = a0 * b1;
    let cross1 = a1 * b0;
    let middle = (low >> 16u) + (cross0 & 0xFFFFu) + (cross1 & 0xFFFFu);
    let lo = (low & 0xFFFFu) | (middle << 16u);
    let hi = a1 * b1 + (cross0 >> 16u) + (cross1 >> 16u) + (middle >> 16u);
    return vec2<u32>(lo, hi);
}

fn negate64(value: vec2<u32>) -> vec2<u32> {
    let lo = ~value.x + 1u;
    var hi = ~value.y;
    if lo == 0u {
        hi = hi + 1u;
    }
    return vec2<u32>(lo, hi);
}

fn shift_low(value: vec2<u32>, shift: u32) -> u32 {
    if shift == 0u {
        return value.x;
    }
    return (value.x >> shift) | (value.y << (32u - shift));
}

struct Slope {
    x0: i32,
    y0: i32,
    increment: i32,
    negative: bool,
    x_major: bool,
}

fn slope_new(a: vec2<i32>, b: vec2<i32>) -> Slope {
    var top = a;
    var bottom = b;
    if b.y < a.y {
        top = b;
        bottom = a;
    }
    var x0 = top.x << FRACTION;
    var dx = bottom.x - top.x;
    let dy = bottom.y - top.y;
    let negative = dx < 0;
    if negative {
        x0 = x0 - 1;
        dx = -dx;
    }
    let x_major = dx > dy;
    if x_major || dx == dy {
        x0 = x0 + select(HALF, -HALF, negative);
    }
    var reciprocal = ONE;
    if dy != 0 {
        reciprocal = ONE / dy;
    }
    var s: Slope;
    s.x0 = x0;
    s.y0 = top.y;
    s.increment = dx * reciprocal;
    s.negative = negative;
    s.x_major = x_major;
    return s;
}

fn slope_vertical(s: Slope) -> bool {
    return s.increment == 0;
}

fn slope_start(s: Slope, y: i32) -> i32 {
    let displacement = (y - s.y0) * s.increment;
    if s.negative {
        return s.x0 - displacement;
    }
    return s.x0 + displacement;
}

fn slope_end(s: Slope, y: i32) -> i32 {
    let start = slope_start(s, y);
    if !s.x_major {
        return start;
    }
    if !s.negative {
        return (start & ~LOW_BITS) + s.increment - ONE;
    }
    return (start | LOW_BITS) - s.increment + ONE;
}

fn slope_span(s: Slope, y: i32) -> vec2<i32> {
    let start = slope_start(s, y) >> FRACTION;
    let end = slope_end(s, y) >> FRACTION;
    return vec2<i32>(min(start, end), max(start, end));
}

struct Interp {
    across: bool,
    start: i32,
    length: i32,
    position: i32,
    shift: u32,
    linear: bool,
    w_buffer: bool,
    reciprocal: i32,
    w0_numerator: u32,
    w0_denominator: u32,
    w1_denominator: u32,
    factor: u32,
}

fn interp_new(across: bool, span: vec2<i32>, w: vec2<i32>, w_buffer: bool, at: i32) -> Interp {
    var it: Interp;
    it.across = across;
    it.start = span.x;
    it.length = span.y - span.x;
    let w0 = u32(w.x);
    let w1 = u32(w.y);
    var low_bits = 0x7Eu;
    if across {
        it.w0_numerator = w0;
        it.w0_denominator = w0;
        it.w1_denominator = w1;
        it.shift = 8u;
        low_bits = 0x7Fu;
    } else {
        it.w0_numerator = w0 >> 1u;
        it.w0_denominator = (w0 + (w0 & ~w1 & 1u)) >> 1u;
        it.w1_denominator = w1 >> 1u;
        it.shift = 9u;
    }
    it.linear = w0 == w1 && (w0 & low_bits) == 0u;
    it.w_buffer = w_buffer;
    it.reciprocal = 0;
    if it.length != 0 {
        it.reciprocal = (1 << 22u) / it.length;
    }
    let x = at - it.start;
    it.position = x;
    it.factor = 0u;
    if it.length != 0 && (!it.linear || w_buffer) {
        let xu = u32(x);
        let numerator = (xu * it.w0_numerator) << it.shift;
        let denominator = xu * it.w0_denominator + (u32(it.length) - xu) * it.w1_denominator;
        if denominator != 0u {
            it.factor = numerator / denominator;
        }
    }
    return it;
}

fn perspective(it: Interp, a: i32, b: i32) -> i32 {
    if a < b {
        let product = mul64(u32(b) - u32(a), it.factor);
        return a + bitcast<i32>(shift_low(product, it.shift));
    }
    let one = 1u << it.shift;
    let difference = u32(a) - u32(b);
    var product: vec2<u32>;
    if it.factor <= one {
        product = mul64(difference, one - it.factor);
    } else {
        product = negate64(mul64(difference, it.factor - one));
    }
    return b + bitcast<i32>(shift_low(product, it.shift));
}

fn interpolate(it: Interp, a: i32, b: i32) -> i32 {
    if it.length == 0 || a == b {
        return a;
    }
    if it.linear {
        if a < b {
            return a + (b - a) * it.position / it.length;
        }
        return b + (a - b) * (it.length - it.position) / it.length;
    }
    return perspective(it, a, b);
}

fn depth(it: Interp, a: i32, b: i32) -> i32 {
    if it.length == 0 || a == b {
        return a;
    }
    if it.w_buffer {
        return perspective(it, a, b);
    }
    var base = b;
    var displacement = u32(a) - u32(b);
    var factor = u32(it.length - it.position);
    if a < b {
        base = a;
        displacement = u32(b) - u32(a);
        factor = u32(it.position);
    }
    if it.across {
        let product = mul64((displacement >> 9u) * factor, u32(it.reciprocal));
        return base + bitcast<i32>(shift_low(product, 13u));
    }
    var shift = 0u;
    loop {
        if displacement <= 0x3FFu {
            break;
        }
        displacement = displacement >> 1u;
        shift = shift + 1u;
    }
    let product = mul64(displacement * factor, u32(it.reciprocal));
    return base + bitcast<i32>(shift_low(product, 22u) << shift);
}

struct Values {
    w: i32,
    depth: i32,
    color: vec3<i32>,
    texcoord: vec2<i32>,
}

fn color_of(value: u32) -> vec3<i32> {
    return vec3<i32>(widen(value & 0xFFu), widen((value >> 8u) & 0xFFu), widen((value >> 16u) & 0xFFu));
}

fn texcoord_of(value: u32) -> vec2<i32> {
    return vec2<i32>(bitcast<i32>(value << 16u) >> 16u, bitcast<i32>(value) >> 16u);
}

fn values_along(it: Interp, p: Poly, start: u32, stop: u32, ta: vec4<u32>, tb: vec4<u32>) -> Values {
    var v: Values;
    v.w = interpolate(it, bitcast<i32>(ta.w), bitcast<i32>(tb.w));
    v.depth = depth(it, bitcast<i32>(ta.z), bitcast<i32>(tb.z));
    let sa = texel(p.record + HEADER + start * VERTEX + 4u);
    let sb = texel(p.record + HEADER + stop * VERTEX + 4u);
    let a = color_of(sa.x);
    let b = color_of(sb.x);
    v.color = vec3<i32>(interpolate(it, a.x, b.x), interpolate(it, a.y, b.y), interpolate(it, a.z, b.z));
    let s = texcoord_of(sa.y);
    let t = texcoord_of(sb.y);
    v.texcoord = vec2<i32>(interpolate(it, s.x, t.x), interpolate(it, s.y, t.y));
    return v;
}

struct Edge {
    slope: Slope,
    start: u32,
    stop: u32,
    offset: i32,
    flat: bool,
    ta: vec4<u32>,
    tb: vec4<u32>,
}

fn screen_of(first: vec4<u32>) -> vec2<i32> {
    return vec2<i32>(i32(first.x & 0xFFFFu), i32(first.x >> 16u));
}

fn edge_walk(p: Poly, y: i32, left: bool, top: vec4<u32>) -> Edge {
    let forward = left == p.front;
    var start = p.top;
    var stop = step_index(p, start, forward);
    var ta = top;
    var tb = vertex_first(p, stop);
    for (var n = 0u; n < p.count; n = n + 1u) {
        if y >= screen_of(tb).y && start != p.bottom {
            start = stop;
            stop = step_index(p, start, forward);
            ta = tb;
            tb = vertex_first(p, stop);
        } else {
            break;
        }
    }
    var e: Edge;
    e.ta = ta;
    e.tb = tb;
    e.slope = slope_new(screen_of(ta), screen_of(tb));
    e.offset = 0;
    if e.slope.increment >= ONE && left == e.slope.negative {
        e.offset = 1;
    }
    e.start = start;
    e.stop = stop;
    return e;
}

fn edge_values(p: Poly, e: Edge, y: i32) -> Values {
    if e.flat {
        let still = interp_new(false, vec2<i32>(0, 0), vec2<i32>(0, 0), p.w_buffer, 0);
        let t = vertex_first(p, e.stop);
        return values_along(still, p, e.stop, e.stop, t, t);
    }
    let along = interp_new(false, vec2<i32>(screen_of(e.ta).y - e.offset, screen_of(e.tb).y - e.offset), vec2<i32>(bitcast<i32>(e.ta.w), bitcast<i32>(e.tb.w)), p.w_buffer, y);
    return values_along(along, p, e.start, e.stop, e.ta, e.tb);
}

fn edge_flat(p: Poly, rightmost: bool) -> Edge {
    let last = p.count - 1u;
    var best = 0u;
    for (var k = 0u; k < 2u; k = k + 1u) {
        let index = select(1u, last, k == 1u);
        let further = select(screen(p, index).x < screen(p, best).x, screen(p, index).x > screen(p, best).x, rightmost);
        if further {
            best = index;
        }
    }
    let point = screen(p, best);
    var e: Edge;
    e.slope = slope_new(point, point);
    e.start = best;
    e.stop = best;
    e.flat = true;
    return e;
}

struct Sample {
    covered: bool,
    depth: i32,
    color: vec3<u32>,
    texcoord: vec2<i32>,
    edges: u32,
    rim: bool,
    fringe: bool,
    footprint: vec4<f32>,
}

struct RowSetup {
    live: bool,
    start: i32,
    end: i32,
    left_end: i32,
    right_start: i32,
    rows: u32,
    fill_left: bool,
    fill_middle: bool,
    fill_right: bool,
    lv: Values,
    rv: Values,
}

fn row_zone(row: RowSetup, x: i32) -> u32 {
    if x < row.left_end {
        return LEFT;
    }
    if x < row.right_start {
        return 0u;
    }
    return RIGHT;
}

fn row_covers(row: RowSetup, x: i32) -> bool {
    if x < max(row.start, 0) || x > min(row.end, WIDTH - 1) {
        return false;
    }
    let zone = row_zone(row, x);
    if zone == LEFT {
        return row.fill_left;
    }
    if zone == RIGHT {
        return row.fill_right;
    }
    return row.fill_middle;
}

fn reference_row(p: Poly, y: i32, columns: vec2<i32>) -> RowSetup {
    var row: RowSetup;
    row.live = false;
    if p.degenerate || p.count < 3u {
        return row;
    }
    let top_texel = vertex_first(p, p.top);
    let top = screen_of(top_texel).y;
    let bottom = screen(p, p.bottom).y;
    let flat = top == bottom;
    if y < top || (y >= bottom && !(flat && y == top)) {
        return row;
    }
    var left: Edge;
    var right: Edge;
    if flat {
        left = edge_flat(p, false);
        right = edge_flat(p, true);
    } else {
        left = edge_walk(p, y, true, top_texel);
        right = edge_walk(p, y, false, top_texel);
    }
    let alpha = raw_alpha(p);
    let wireframe = alpha == 0u;
    let disp = u.frame.y;
    let fill_all = (disp & (ANTIALIASING | EDGE_MARKING)) != 0u || (alpha < OPAQUE && (disp & BLENDING) != 0u) || wireframe;

    let left_span = slope_span(left.slope, y);
    let right_span = slope_span(right.slope, y);
    var start = left_span.x;
    var end = right_span.y;
    if slope_vertical(right.slope) && (!slope_vertical(left.slope) || start != end) && end != 0 {
        end = end - 1;
    }
    let last_row = y == bottom - 1;
    var apart = screen_of(left.tb).x != screen_of(right.tb).x;
    if flat {
        apart = screen(p, left.stop).x != screen(p, right.stop).x;
    }

    var left_length = 1;
    var right_length = 1;
    var fill_left: bool;
    var fill_right: bool;
    var l = left;
    var r = right;
    if start > end {
        let swap = start;
        start = end;
        end = swap;
        fill_left = right.slope.negative || !right.slope.x_major || (last_row && right.slope.x_major && apart);
        fill_right = (!left.slope.negative && left.slope.x_major) || (!(left.slope.negative && left.slope.x_major) && slope_vertical(right.slope)) || (last_row && left.slope.x_major && apart);
        l = right;
        r = left;
    } else {
        if left.slope.x_major {
            left_length = left_span.y - left_span.x + 1;
        }
        if right.slope.x_major {
            right_length = right_span.y - right_span.x + 1;
        }
        fill_left = left.slope.negative || !left.slope.x_major || (last_row && left.slope.x_major && apart) || (left.slope.increment == right.slope.increment && start + left_length == end + 1);
        fill_right = (!right.slope.negative && right.slope.x_major) || slope_vertical(right.slope) || (last_row && right.slope.x_major && apart);
    }
    row.live = true;
    row.start = start;
    row.end = end;
    row.left_end = min(start + left_length, end + 1);
    row.right_start = end - right_length + 1;
    row.rows = 0u;
    if y == top {
        row.rows = TOP;
    } else if last_row {
        row.rows = BOTTOM;
    }
    row.fill_left = fill_all || fill_left;
    row.fill_middle = !wireframe || y == top || last_row;
    row.fill_right = fill_all || fill_right;
    var needed = false;
    for (var column = columns.x; column <= columns.y; column = column + 1) {
        if row_covers(row, column) {
            needed = true;
        }
    }
    if needed {
        row.lv = edge_values(p, l, y);
        row.rv = edge_values(p, r, y);
    }
    return row;
}

fn reference_tail(p: Poly, row: RowSetup, x: i32) -> Sample {
    var out: Sample;
    out.covered = false;
    if !row.live || !row_covers(row, x) {
        return out;
    }
    let edge = row_zone(row, x);
    let lv = row.lv;
    let rv = row.rv;
    let across = interp_new(true, vec2<i32>(row.start, row.end + 1), vec2<i32>(lv.w, rv.w), p.w_buffer, x);
    out.covered = true;
    out.depth = depth(across, lv.depth, rv.depth);
    out.color = vec3<u32>(
        u32(interpolate(across, lv.color.x, rv.color.x) >> 3u),
        u32(interpolate(across, lv.color.y, rv.color.y) >> 3u),
        u32(interpolate(across, lv.color.z, rv.color.z) >> 3u),
    ) & vec3<u32>(0xFFu);
    out.texcoord = vec2<i32>(
        (interpolate(across, lv.texcoord.x, rv.texcoord.x) << 16u) >> 16u,
        (interpolate(across, lv.texcoord.y, rv.texcoord.y) << 16u) >> 16u,
    );
    out.edges = row.rows | edge;
    out.rim = out.edges != 0u;
    return out;
}

fn reference_sample(p: Poly, x: i32, y: i32) -> Sample {
    return reference_tail(p, reference_row(p, y, vec2<i32>(x, x)), x);
}

struct FineValues {
    w: f32,
    depth: f32,
    color: vec3<f32>,
    texcoord: vec2<f32>,
}

fn blend_perspective(a: f32, b: f32, wa: f32, wb: f32, t: f32) -> f32 {
    if wa == wb || wa <= 0.0 || wb <= 0.0 {
        return mix(a, b, t);
    }
    let ia = (1.0 - t) / wa;
    let ib = t / wb;
    return (a * ia + b * ib) / (ia + ib);
}

fn blend_w(wa: f32, wb: f32, t: f32) -> f32 {
    if wa == wb || wa <= 0.0 || wb <= 0.0 {
        return mix(wa, wb, t);
    }
    return 1.0 / ((1.0 - t) / wa + t / wb);
}

fn vertex_first(p: Poly, index: u32) -> vec4<u32> {
    return texel(p.record + HEADER + index * VERTEX);
}

fn vertex_second(p: Poly, index: u32) -> vec4<u32> {
    return texel(p.record + HEADER + index * VERTEX + 4u);
}

fn fine_values(p: Poly, index: u32) -> FineValues {
    let first = vertex_first(p, index);
    let second = vertex_second(p, index);
    return values_of(vec4<u32>(first.z, first.w, second.x, second.y));
}

fn values_of(packed_values: vec4<u32>) -> FineValues {
    let color = packed_values.z;
    let texcoord = packed_values.w;
    var v: FineValues;
    v.w = f32(bitcast<i32>(packed_values.y));
    v.depth = f32(bitcast<i32>(packed_values.x));
    v.color = vec3<f32>(f32(widen(color & 0xFFu)), f32(widen((color >> 8u) & 0xFFu)), f32(widen((color >> 16u) & 0xFFu)));
    v.texcoord = vec2<f32>(f32(bitcast<i32>(texcoord << 16u) >> 16u), f32(bitcast<i32>(texcoord) >> 16u));
    return v;
}

fn fine_mix(p: Poly, a: FineValues, b: FineValues, t: f32) -> FineValues {
    var v: FineValues;
    v.w = blend_w(a.w, b.w, t);
    if p.w_buffer {
        v.depth = blend_perspective(a.depth, b.depth, a.w, b.w, t);
    } else {
        v.depth = mix(a.depth, b.depth, t);
    }
    v.color = vec3<f32>(
        blend_perspective(a.color.x, b.color.x, a.w, b.w, t),
        blend_perspective(a.color.y, b.color.y, a.w, b.w, t),
        blend_perspective(a.color.z, b.color.z, a.w, b.w, t),
    );
    v.texcoord = vec2<f32>(
        blend_perspective(a.texcoord.x, b.texcoord.x, a.w, b.w, t),
        blend_perspective(a.texcoord.y, b.texcoord.y, a.w, b.w, t),
    );
    return v;
}

struct Chain {
    start: u32,
    stop: u32,
    a: vec2<f32>,
    b: vec2<f32>,
}

fn chain(p: Poly, y: f32, left: bool) -> Chain {
    let forward = left == p.front;
    var c: Chain;
    c.start = p.fine_top;
    c.stop = step_index(p, c.start, forward);
    c.a = fine(p, c.start);
    c.b = fine(p, c.stop);
    for (var n = 0u; n < p.count; n = n + 1u) {
        if y >= c.b.y && c.start != p.fine_bottom {
            c.start = c.stop;
            c.stop = step_index(p, c.start, forward);
            c.a = c.b;
            c.b = fine(p, c.stop);
        } else {
            break;
        }
    }
    return c;
}

fn edge_x(a: vec2<f32>, b: vec2<f32>, y: f32) -> f32 {
    if b.y == a.y {
        return a.x;
    }
    return a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y);
}

fn edge_t(a: vec2<f32>, b: vec2<f32>, y: f32) -> f32 {
    if b.y == a.y {
        return 0.0;
    }
    return clamp((y - a.y) / (b.y - a.y), 0.0, 1.0);
}

fn segment_distance(point: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let along = b - a;
    let length = dot(along, along);
    var t = 0.0;
    if length > 0.0 {
        t = clamp(dot(point - a, along) / length, 0.0, 1.0);
    }
    return distance(point, a + along * t);
}

fn boundary_distance(p: Poly, point: vec2<f32>) -> f32 {
    var nearest = 1.0e9;
    let first = fine(p, 0u);
    var a = first;
    for (var i = 0u; i < p.count; i = i + 1u) {
        var b = first;
        if i + 1u < p.count {
            b = fine(p, i + 1u);
        }
        nearest = min(nearest, segment_distance(point, a, b));
        a = b;
    }
    return nearest;
}

struct UpRow {
    live: bool,
    valued: bool,
    swapped: bool,
    top: f32,
    bottom: f32,
    xl: f32,
    xr: f32,
    tl: f32,
    tr: f32,
    left: vec2<u32>,
    right: vec2<u32>,
    lv: FineValues,
    rv: FineValues,
}

fn upscaled_row(p: Poly, point_y: f32) -> UpRow {
    var row: UpRow;
    row.live = false;
    row.valued = false;
    if p.degenerate || p.count < 3u {
        return row;
    }
    let top = fine(p, p.fine_top).y;
    let bottom = fine(p, p.fine_bottom).y;
    let y = clamp(point_y, top, bottom);
    let lc = chain(p, y, true);
    let rc = chain(p, y, false);
    let left = vec2<u32>(lc.start, lc.stop);
    let right = vec2<u32>(rc.start, rc.stop);
    let la = lc.a;
    let lb = lc.b;
    let ra = rc.a;
    let rb = rc.b;
    var xl = edge_x(la, lb, y);
    var xr = edge_x(ra, rb, y);
    if top == bottom {
        xl = min(fine(p, 0u).x, min(fine(p, 1u).x, fine(p, p.count - 1u).x));
        xr = max(fine(p, 0u).x, max(fine(p, 1u).x, fine(p, p.count - 1u).x));
    }
    return row_between(top, bottom, left, right, la, lb, ra, rb, xl, xr, y);
}

fn row_between(top: f32, bottom: f32, left_in: vec2<u32>, right_in: vec2<u32>, la_in: vec2<f32>, lb_in: vec2<f32>, ra_in: vec2<f32>, rb_in: vec2<f32>, xl_in: f32, xr_in: f32, y: f32) -> UpRow {
    var row: UpRow;
    row.valued = false;
    row.swapped = false;
    var left = left_in;
    var right = right_in;
    var la = la_in;
    var lb = lb_in;
    var ra = ra_in;
    var rb = rb_in;
    var xl = xl_in;
    var xr = xr_in;
    if xl > xr {
        row.swapped = true;
        let swap_x = xl;
        xl = xr;
        xr = swap_x;
        let swap_chain = left;
        left = right;
        right = swap_chain;
        let swap_a = la;
        let swap_b = lb;
        la = ra;
        lb = rb;
        ra = swap_a;
        rb = swap_b;
    }
    row.live = true;
    row.top = top;
    row.bottom = bottom;
    row.xl = xl;
    row.xr = xr;
    row.tl = edge_t(la, lb, y);
    row.tr = edge_t(ra, rb, y);
    row.left = left;
    row.right = right;
    return row;
}

fn upscaled_values(p: Poly, row: UpRow) -> UpRow {
    var out = row;
    out.lv = fine_mix(p, fine_values(p, row.left.x), fine_values(p, row.left.y), row.tl);
    out.rv = fine_mix(p, fine_values(p, row.right.x), fine_values(p, row.right.y), row.tr);
    out.valued = true;
    return out;
}

fn upscaled_tail(p: Poly, given: UpRow, point: vec2<f32>, pixel: f32, forced: bool) -> Sample {
    var out: Sample;
    out.covered = false;
    if !given.live {
        return out;
    }
    let top = given.top;
    let bottom = given.bottom;
    let xl = given.xl;
    let xr = given.xr;
    var inside = point.y >= top && point.y < bottom && point.x >= xl && point.x < xr;
    if top == bottom {
        inside = point.y >= top && point.y < top + pixel && point.x >= xl && point.x < xr;
    }
    let alpha = raw_alpha(p);
    var covered = inside;
    if alpha == 0u {
        covered = inside && boundary_distance(p, point) < max(WIRE, pixel);
    } else if !inside && alpha == OPAQUE && mode(p) != SHADOW {
        covered = boundary_distance(p, point) <= MARGIN;
    }
    if !covered && !forced {
        return out;
    }
    var row = given;
    if !row.valued {
        row = upscaled_values(p, row);
    }
    let lv = row.lv;
    let rv = row.rv;
    var tx = 0.0;
    if xr > xl {
        tx = clamp((point.x - xl) / (xr - xl), 0.0, 1.0);
    }
    let v = fine_mix(p, lv, rv, tx);
    out.covered = true;
    out.fringe = !inside;
    out.depth = i32(floor(v.depth));
    out.color = vec3<u32>(max(floor(v.color), vec3<f32>(0.0))) >> vec3<u32>(3u);
    out.texcoord = (vec2<i32>(floor(v.texcoord)) << vec2<u32>(16u)) >> vec2<u32>(16u);
    if (mipmapped(p) || u.options.x == 3u) && top != bottom {
        out.footprint = footprint(p, row, point, pixel, v.texcoord);
    }
    var edges = 0u;
    if point.x - xl < pixel {
        edges = edges | LEFT;
    }
    if xr - point.x <= pixel {
        edges = edges | RIGHT;
    }
    if point.y - top < pixel {
        edges = edges | TOP;
    }
    if bottom - point.y <= pixel {
        edges = edges | BOTTOM;
    }
    out.edges = edges;
    out.rim = point.x - xl < 1.0 || xr - point.x <= 1.0 || point.y - top < 1.0 || bottom - point.y <= 1.0;
    return out;
}

fn row_texcoord(row: UpRow, x: f32) -> vec2<f32> {
    var t = 0.0;
    if row.xr > row.xl {
        t = clamp((x - row.xl) / (row.xr - row.xl), -1.0, 2.0);
    }
    let a = row.lv;
    let b = row.rv;
    return vec2<f32>(
        blend_perspective(a.texcoord.x, b.texcoord.x, a.w, b.w, t),
        blend_perspective(a.texcoord.y, b.texcoord.y, a.w, b.w, t),
    );
}

fn footprint(p: Poly, row: UpRow, point: vec2<f32>, pixel: f32, at: vec2<f32>) -> vec4<f32> {
    var step_x = pixel;
    if point.x + pixel >= row.xr {
        step_x = -pixel;
    }
    let across = (row_texcoord(row, point.x + step_x) - at) * sign(step_x);
    var step_y = pixel;
    if point.y + pixel >= row.bottom {
        step_y = -pixel;
    }
    let below = upscaled_values(p, upscaled_row(p, point.y + step_y));
    let down = (row_texcoord(below, point.x) - at) * sign(step_y);
    return vec4<f32>(across, down) / 16.0;
}

fn upscaled_sample(p: Poly, point: vec2<f32>, pixel: f32, forced: bool) -> Sample {
    return upscaled_tail(p, upscaled_row(p, point.y), point, pixel, forced);
}

fn slab_sample(p: Poly, reach: vec2<u32>, corners: vec4<u32>, la: vec4<u32>, lb: vec4<u32>, ra: vec4<u32>, rb: vec4<u32>, point: vec2<f32>, pixel: f32) -> Sample {
    var out: Sample;
    out.covered = false;
    let rows = fine_of(reach.y);
    if point.y < rows.x || point.y >= rows.y {
        return out;
    }
    let ends = fine_of(reach.x);
    let a = fine_of(corners.x);
    let b = fine_of(corners.y);
    let c = fine_of(corners.z);
    let d = fine_of(corners.w);
    var row = row_between(ends.x, ends.y, vec2<u32>(0u), vec2<u32>(0u), a, b, c, d, edge_x(a, b, point.y), edge_x(c, d, point.y), point.y);
    if point.x < row.xl || point.x >= row.xr {
        return out;
    }
    if row.swapped {
        row.lv = fine_mix(p, values_of(ra), values_of(rb), row.tl);
        row.rv = fine_mix(p, values_of(la), values_of(lb), row.tr);
    } else {
        row.lv = fine_mix(p, values_of(la), values_of(lb), row.tl);
        row.rv = fine_mix(p, values_of(ra), values_of(rb), row.tr);
    }
    row.valued = true;
    return upscaled_tail(p, row, point, pixel, false);
}

struct Texel {
    color: u32,
    alpha: u32,
}

fn wrap(coordinate: i32, size: i32, repeat: bool, flip: bool) -> i32 {
    if !repeat {
        return clamp(coordinate, 0, size - 1);
    }
    if flip && (coordinate & size) != 0 {
        return (size - 1) - (coordinate & (size - 1));
    }
    return coordinate & (size - 1);
}

fn mix_color(a: u32, b: u32, weight_a: u32, weight_b: u32) -> u32 {
    let total = weight_a + weight_b;
    var out = 0u;
    for (var k = 0u; k < 3u; k = k + 1u) {
        let mask = 0x1Fu << (k * 5u);
        let channel = ((a & mask) * weight_a + (b & mask) * weight_b) / total;
        out = out | (channel & mask);
    }
    return out;
}

fn compressed(base: u32, width: u32, s: u32, t: u32, palette: u32) -> Texel {
    let address = (base + (t & ~3u) * (width / 4u) + (s & ~3u) + (t & 3u)) & (TEXTURE_SIZE - 1u);
    var index_address = SLOT + ((address & (SLOT - 1u) & ~3u) >> 1u);
    if address >= 2u * SLOT {
        index_address = index_address + COMPRESSED_INDEX_HIGH;
    }
    var code = 0u;
    if !(address >= SLOT && address < 2u * SLOT) {
        code = (vram(address) >> ((s & 3u) * 2u)) & 3u;
    }
    let info = vram16(index_address);
    let colors = palette + (info & 0x3FFFu) * 4u;
    let block = info >> 14u;
    var out: Texel;
    out.alpha = OPAQUE;
    if code == 0u {
        out.color = palette16(colors);
    } else if code == 1u {
        out.color = palette16(colors + 2u);
    } else if code == 2u {
        if block == 1u {
            out.color = mix_color(palette16(colors), palette16(colors + 2u), 1u, 1u);
        } else if block == 3u {
            out.color = mix_color(palette16(colors), palette16(colors + 2u), 5u, 3u);
        } else {
            out.color = palette16(colors + 4u);
        }
    } else if block == 2u {
        out.color = palette16(colors + 6u);
    } else if block == 3u {
        out.color = mix_color(palette16(colors), palette16(colors + 2u), 3u, 5u);
    } else {
        out.color = 0u;
        out.alpha = 0u;
    }
    return out;
}

fn indexed(index: u32, base: u32, color0: u32) -> Texel {
    var out: Texel;
    out.color = palette16(base + index * 2u);
    out.alpha = select(OPAQUE, color0, index == 0u);
    return out;
}

fn sample_texture(param: u32, palette_base: u32, s_in: i32, t_in: i32) -> Texel {
    let width = 8 << ((param >> 20u) & 7u);
    let height = 8 << ((param >> 23u) & 7u);
    let s = u32(wrap(s_in >> 4u, width, (param & 0x10000u) != 0u, (param & 0x40000u) != 0u));
    let t = u32(wrap(t_in >> 4u, height, (param & 0x20000u) != 0u, (param & 0x80000u) != 0u));
    let w = u32(width);
    let base = (param & 0xFFFFu) << 3u;
    let texel = t * w + s;
    var color0 = OPAQUE;
    if (param & 0x20000000u) != 0u {
        color0 = 0u;
    }
    let format = (param >> 26u) & 7u;
    var out: Texel;
    switch format {
        case 1u: {
            let value = vram(base + texel);
            let alpha = value >> 5u;
            out.color = palette16((palette_base << 4u) + (value & 0x1Fu) * 2u);
            out.alpha = alpha * 4u + alpha / 2u;
        }
        case 2u: {
            let value = vram(base + texel / 4u);
            out = indexed((value >> ((s & 3u) * 2u)) & 3u, palette_base << 3u, color0);
        }
        case 3u: {
            let value = vram(base + texel / 2u);
            out = indexed((value >> ((s & 1u) * 4u)) & 0xFu, palette_base << 4u, color0);
        }
        case 4u: {
            out = indexed(vram(base + texel), palette_base << 4u, color0);
        }
        case 5u: {
            out = compressed(base, w, s, t, palette_base << 4u);
        }
        case 6u: {
            let value = vram(base + texel);
            out.color = palette16((palette_base << 4u) + (value & 7u) * 2u);
            out.alpha = value >> 3u;
        }
        case 7u: {
            let color = vram16(base + texel * 2u);
            out.color = color;
            out.alpha = select(0u, OPAQUE, (color & 0x8000u) != 0u);
        }
        default: {
            out.color = 0u;
            out.alpha = OPAQUE;
        }
    }
    return out;
}

fn hard_alpha(format: u32) -> bool {
    return format != 1u && format != 6u;
}

fn smooth_texture(p: Poly, s_in: i32, t_in: i32) -> Texel {
    let param = p.texture;
    let place = atlas_place(p);
    let s = s_in - 8;
    let t = t_in - 8;
    let fs = u32(s & 15);
    let ft = u32(t & 15);
    let s0 = (s >> 4u) << 4u;
    let t0 = (t >> 4u) << 4u;
    var weighted = vec3<u32>(0u);
    var coverage = 0u;
    for (var k = 0u; k < 4u; k = k + 1u) {
        let dx = k & 1u;
        let dy = k >> 1u;
        let weight = select(16u - fs, fs, dx == 1u) * select(16u - ft, ft, dy == 1u);
        if (place & PLACED) != 0u {
            let tap = vec4<u32>(atlas_load(place, param, 0u, vec2<i32>((s0 >> 4u) + i32(dx), (t0 >> 4u) + i32(dy)))) >> vec4<u32>(6u);
            weighted = weighted + tap.xyz * weight;
            coverage = coverage + tap.w * weight;
            continue;
        }
        let texel = sample_texture(param, p.palette, s0 + i32(dx) * 16, t0 + i32(dy) * 16);
        let color = vec3<u32>(texel.color & 0x1Fu, (texel.color >> 5u) & 0x1Fu, (texel.color >> 10u) & 0x1Fu);
        weighted = weighted + color * texel.alpha * weight;
        coverage = coverage + texel.alpha * weight;
    }
    var out: Texel;
    out.alpha = (coverage + 128u) / 256u;
    let format = (param >> 26u) & 7u;
    if hard_alpha(format) {
        out.alpha = select(0u, OPAQUE, coverage * 2u >= OPAQUE * 256u);
    }
    if out.alpha == 0u {
        out.color = 0u;
        return out;
    }
    let color = (weighted + coverage / 2u) / coverage;
    out.color = color.x | (color.y << 5u) | (color.z << 10u);
    return out;
}

fn cubic(x: f32) -> vec4<f32> {
    let x2 = x * x;
    let x3 = x2 * x;
    return vec4<f32>(
        -0.5 * x3 + x2 - 0.5 * x,
        1.5 * x3 - 2.5 * x2 + 1.0,
        -1.5 * x3 + 2.0 * x2 + 0.5 * x,
        0.5 * x3 - 0.5 * x2,
    );
}

fn bicubic_texture(p: Poly, s_in: i32, t_in: i32) -> Texel {
    let param = p.texture;
    let place = atlas_place(p);
    let s = s_in - 8;
    let t = t_in - 8;
    let ws = cubic(f32(s & 15) / 16.0);
    let wt = cubic(f32(t & 15) / 16.0);
    let corner = vec2<i32>((s >> 4u) - 1, (t >> 4u) - 1);
    var sum = vec4<f32>(0.0);
    for (var k = 0u; k < 16u; k = k + 1u) {
        let dx = k & 3u;
        let dy = k >> 2u;
        var texel: Texel;
        if (place & PLACED) != 0u {
            texel = atlas_texel(place, param, corner + vec2<i32>(i32(dx), i32(dy)));
        } else {
            texel = sample_texture(param, p.palette, (corner.x + i32(dx)) << 4u, (corner.y + i32(dy)) << 4u);
        }
        let weight = ws[dx] * wt[dy];
        let color = vec3<f32>(f32(texel.color & 0x1Fu), f32((texel.color >> 5u) & 0x1Fu), f32((texel.color >> 10u) & 0x1Fu));
        let alpha = f32(texel.alpha);
        sum = sum + vec4<f32>(color * alpha, alpha) * weight;
    }
    var out: Texel;
    let alpha = clamp(sum.w, 0.0, f32(OPAQUE));
    out.alpha = u32(round(alpha));
    let format = (param >> 26u) & 7u;
    if hard_alpha(format) {
        out.alpha = select(0u, OPAQUE, alpha * 2.0 >= f32(OPAQUE));
    }
    if out.alpha == 0u || sum.w <= 0.0 {
        out.alpha = 0u;
        out.color = 0u;
        return out;
    }
    let color = vec3<u32>(clamp(round(sum.xyz / sum.w), vec3<f32>(0.0), vec3<f32>(31.0)));
    out.color = color.x | (color.y << 5u) | (color.z << 10u);
    return out;
}

const XBR_YUV: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.299, -0.169, 0.5),
    vec3<f32>(0.587, -0.331, -0.419),
    vec3<f32>(0.114, 0.5, -0.081),
);
const XBR_EQ: f32 = 0.4706;
const XBR_LV2: f32 = 2.0;
const XBR_CELLS: f32 = 4.0;

fn xbr_key(texel: Texel) -> vec3<f32> {
    if texel.alpha == 0u {
        return vec3<f32>(-1.0);
    }
    return XBR_YUV * (vec3<f32>(f32(texel.color & 0x1Fu), f32((texel.color >> 5u) & 0x1Fu), f32((texel.color >> 10u) & 0x1Fu)) / 31.0);
}

fn xbr_value(texel: Texel) -> vec4<f32> {
    let alpha = f32(texel.alpha);
    let color = vec3<f32>(f32(texel.color & 0x1Fu), f32((texel.color >> 5u) & 0x1Fu), f32((texel.color >> 10u) & 0x1Fu));
    return vec4<f32>(color * alpha, alpha);
}

fn xbr_dist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return dot(abs(a - b), vec3<f32>(1.0));
}

fn xbr_dist4(a: mat4x3<f32>, b: mat4x3<f32>) -> vec4<f32> {
    return vec4<f32>(xbr_dist(a[0], b[0]), xbr_dist(a[1], b[1]), xbr_dist(a[2], b[2]), xbr_dist(a[3], b[3]));
}

fn xbr_eq(a: mat4x3<f32>, b: mat4x3<f32>) -> vec4<f32> {
    return step(xbr_dist4(a, b), vec4<f32>(XBR_EQ));
}

fn xbr_neq(a: mat4x3<f32>, b: mat4x3<f32>) -> vec4<f32> {
    return vec4<f32>(1.0) - xbr_eq(a, b);
}

fn xbr_diff(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return select(vec4<f32>(0.0), vec4<f32>(1.0), a != b);
}

fn xbr_lt(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return select(vec4<f32>(0.0), vec4<f32>(1.0), a < b);
}

fn xbr_weighted(a: mat4x3<f32>, b: mat4x3<f32>, c: mat4x3<f32>, d: mat4x3<f32>, e: mat4x3<f32>, f: mat4x3<f32>, g: mat4x3<f32>, h: mat4x3<f32>) -> vec4<f32> {
    return xbr_dist4(a, b) + xbr_dist4(a, c) + xbr_dist4(d, e) + xbr_dist4(d, f) + 4.0 * xbr_dist4(g, h);
}

struct XbrPick {
    key: vec3<f32>,
    value: vec4<f32>,
}

fn xbr_mix(a: XbrPick, b: XbrPick, t: f32) -> XbrPick {
    var out: XbrPick;
    out.key = mix(a.key, b.key, t);
    out.value = mix(a.value, b.value, t);
    return out;
}

fn xbr_closer(e: vec3<f32>, a: XbrPick, b: XbrPick) -> XbrPick {
    return xbr_mix(a, b, step(xbr_dist(e, a.key), xbr_dist(e, b.key)));
}

fn atlas_texel(place: u32, param: u32, texel: vec2<i32>) -> Texel {
    let tap = atlas_load(place, param, 0u, texel);
    var out: Texel;
    out.alpha = u32(tap.w) >> 6u;
    out.color = 0u;
    if tap.w != 0.0 {
        let color = vec3<u32>(round(tap.xyz / tap.w));
        out.color = color.x | (color.y << 5u) | (color.z << 10u);
    }
    return out;
}

fn xbr_texture(p: Poly, s_in: i32, t_in: i32, span: f32) -> Texel {
    let param = p.texture;
    let place = atlas_place(p);
    let tx = s_in >> 4u;
    let ty = t_in >> 4u;
    let fp = (floor((vec2<f32>(f32(s_in & 15), f32(t_in & 15)) + 0.5) / 16.0 * XBR_CELLS) + 0.5) / XBR_CELLS;
    var at: array<Texel, 25>;
    for (var k = 0u; k < 25u; k = k + 1u) {
        let dx = i32(k % 5u) - 2;
        let dy = i32(k / 5u) - 2;
        if (place & PLACED) != 0u {
            at[k] = atlas_texel(place, param, vec2<i32>(tx + dx, ty + dy));
        } else {
            at[k] = sample_texture(param, p.palette, (tx + dx) * 16, (ty + dy) * 16);
        }
    }
    let A1 = xbr_key(at[1]);
    let B1 = xbr_key(at[2]);
    let C1 = xbr_key(at[3]);
    let A0 = xbr_key(at[5]);
    let A = xbr_key(at[6]);
    let B = xbr_key(at[7]);
    let C = xbr_key(at[8]);
    let C4 = xbr_key(at[9]);
    let D0 = xbr_key(at[10]);
    let D = xbr_key(at[11]);
    let E = xbr_key(at[12]);
    let F = xbr_key(at[13]);
    let F4 = xbr_key(at[14]);
    let G0 = xbr_key(at[15]);
    let G = xbr_key(at[16]);
    let H = xbr_key(at[17]);
    let I = xbr_key(at[18]);
    let I4 = xbr_key(at[19]);
    let G5 = xbr_key(at[21]);
    let H5 = xbr_key(at[22]);
    let I5 = xbr_key(at[23]);

    let b = mat4x3<f32>(B, D, H, F);
    let c = mat4x3<f32>(C, A, G, I);
    let d = mat4x3<f32>(D, H, F, B);
    let e = mat4x3<f32>(E, E, E, E);
    let f = mat4x3<f32>(F, B, D, H);
    let g = mat4x3<f32>(G, I, C, A);
    let h = mat4x3<f32>(H, F, B, D);
    let i = mat4x3<f32>(I, C, A, G);
    let i4 = mat4x3<f32>(I4, C1, A0, G5);
    let i5 = mat4x3<f32>(I5, C4, A1, G0);
    let h5 = mat4x3<f32>(H5, F4, B1, D0);
    let f4 = mat4x3<f32>(F4, B1, D0, H5);

    let v2f = vec3<f32>(65536.0, 256.0, 1.0);
    let b_ = v2f * b;
    let c_ = v2f * c;
    let d_ = b_.yzwx;
    let e_ = v2f * e;
    let f_ = b_.wxyz;
    let g_ = c_.zwxy;
    let h_ = b_.zwxy;

    let Ao = vec4<f32>(1.0, -1.0, -1.0, 1.0);
    let Bo = vec4<f32>(1.0, 1.0, -1.0, -1.0);
    let Co = vec4<f32>(1.5, 0.5, -0.5, 0.5);
    let Ax = vec4<f32>(1.0, -1.0, -1.0, 1.0);
    let Bx = vec4<f32>(0.5, 2.0, -0.5, -2.0);
    let Cx = vec4<f32>(1.0, 1.0, -0.5, 0.0);
    let Ay = vec4<f32>(1.0, -1.0, -1.0, 1.0);
    let By = vec4<f32>(2.0, 0.5, -2.0, -0.5);
    let Cy = vec4<f32>(2.0, 0.0, -1.0, 0.5);
    let Ci = vec4<f32>(0.25);

    let fx = Ao * fp.y + Bo * fp.x;
    let fx_l = Ax * fp.y + Bx * fp.x;
    let fx_u = Ay * fp.y + By * fp.x;

    let irlv0 = xbr_diff(e_, f_) * xbr_diff(e_, h_);
    let irlv1 = saturate(irlv0 * (xbr_neq(f, b) * xbr_neq(h, d) + xbr_eq(e, i) * xbr_neq(f, i4) * xbr_neq(h, i5) + xbr_eq(e, g) + xbr_eq(e, c)));
    let irlv2l = xbr_diff(e_, g_) * xbr_diff(d_, g_);
    let irlv2u = xbr_diff(e_, c_) * xbr_diff(b_, c_);

    let aa = max(2.0 * span, 2.0 / XBR_CELLS);
    let delta = vec4<f32>(aa);
    let delta_l = vec4<f32>(0.5, 1.0, 0.5, 1.0) * aa;
    let delta_u = delta_l.yxwz;
    var fx45i = saturate(0.5 + (fx - Co - Ci) / delta);
    var fx45 = saturate(0.5 + (fx - Co) / delta);
    var fx30 = saturate(0.5 + (fx_l - Cx) / delta_l);
    var fx60 = saturate(0.5 + (fx_u - Cy) / delta_u);

    let wd1 = xbr_weighted(e, c, g, i, h5, f4, h, f);
    let wd2 = xbr_weighted(h, d, i5, f, i4, b, e, i);
    let d_fg = xbr_dist4(f, g);
    let d_hc = xbr_dist4(h, c);

    let edri = step(wd1, wd2) * irlv0;
    let edr = xbr_lt(wd1, wd2) * irlv1 * (vec4<f32>(1.0) - edri.yzwx * edri.wxyz);
    let edr_l = step(XBR_LV2 * d_fg, d_hc) * irlv2l * edr;
    let edr_u = step(XBR_LV2 * d_hc, d_fg) * irlv2u * edr;

    fx45i = edri * fx45i;
    fx45 = edr * fx45;
    fx30 = edr_l * fx30;
    fx60 = edr_u * fx60;

    let px = step(xbr_dist4(e, f), xbr_dist4(e, h));
    let most = max(max(fx30, fx60), max(fx45, fx45i));

    let pe = XbrPick(E, xbr_value(at[12]));
    let ph = XbrPick(H, xbr_value(at[17]));
    let pf = XbrPick(F, xbr_value(at[13]));
    let pb = XbrPick(B, xbr_value(at[7]));
    let pd = XbrPick(D, xbr_value(at[11]));

    var r1 = xbr_mix(pe, xbr_mix(ph, pf, px.x), most.x);
    var r2 = xbr_mix(pe, xbr_mix(pb, pd, px.z), most.z);
    let r1a = xbr_closer(E, r1, r2);
    r1 = xbr_mix(pe, xbr_mix(pf, pb, px.y), most.y);
    r2 = xbr_mix(pe, xbr_mix(pd, ph, px.w), most.w);
    let r1b = xbr_closer(E, r1, r2);
    let picked = xbr_closer(E, r1a, r1b).value;

    var out: Texel;
    let alpha = clamp(picked.w, 0.0, f32(OPAQUE));
    out.alpha = u32(round(alpha));
    let format = (param >> 26u) & 7u;
    if hard_alpha(format) {
        out.alpha = select(0u, OPAQUE, alpha * 2.0 >= f32(OPAQUE));
    }
    if out.alpha == 0u || picked.w <= 0.0 {
        out.alpha = 0u;
        out.color = 0u;
        return out;
    }
    let color = vec3<u32>(clamp(round(picked.xyz / picked.w), vec3<f32>(0.0), vec3<f32>(31.0)));
    out.color = color.x | (color.y << 5u) | (color.z << 10u);
    return out;
}

fn texture_of(p: Poly, sample: Sample) -> Texel {
    var texel: Texel;
    if u.options.x == 3u {
        let span = max(length(sample.footprint.xy), length(sample.footprint.zw));
        texel = xbr_texture(p, sample.texcoord.x, sample.texcoord.y, span);
    } else if u.options.x == 2u {
        texel = bicubic_texture(p, sample.texcoord.x, sample.texcoord.y);
    } else if u.options.x != 0u {
        texel = smooth_texture(p, sample.texcoord.x, sample.texcoord.y);
    } else {
        texel = sample_texture(p.texture, p.palette, sample.texcoord.x, sample.texcoord.y);
    }
    if mipmapped(p) {
        return mip_texture(p, sample, texel);
    }
    return texel;
}

fn atlas_place(p: Poly) -> u32 {
    return vertex_word(p, 0u, PLACE_WORD);
}

fn mipmapped(p: Poly) -> bool {
    return u.mips.y != 0u && ((p.texture >> 26u) & 7u) != 0u && (atlas_place(p) & PLACED) != 0u;
}

fn atlas_load(place: u32, param: u32, level: u32, texel: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(8 << ((param >> 20u) & 7u), 8 << ((param >> 23u) & 7u)) >> vec2<u32>(level);
    let s = wrap(texel.x, size.x, (param & 0x10000u) != 0u, (param & 0x40000u) != 0u);
    let t = wrap(texel.y, size.y, (param & 0x20000u) != 0u, (param & 0x80000u) != 0u);
    let origin = (vec2<i32>(i32(place & 0x1FFu), i32((place >> 9u) & 0x1FFu)) * 8) >> vec2<u32>(level);
    return vec4<f32>(textureLoad(atlas, origin + vec2<i32>(s, t), i32(level)));
}

fn atlas_level(place: u32, param: u32, level: u32, uv: vec2<f32>) -> vec4<f32> {
    let at = uv / f32(1u << level);
    let base = at - 0.5;
    let corner = vec2<i32>(floor(base));
    let f = base - floor(base);
    let a = atlas_load(place, param, level, corner);
    let b = atlas_load(place, param, level, corner + vec2<i32>(1, 0));
    let c = atlas_load(place, param, level, corner + vec2<i32>(0, 1));
    let d = atlas_load(place, param, level, corner + vec2<i32>(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn mip_texture(p: Poly, sample: Sample, base: Texel) -> Texel {
    let across = sample.footprint.xy;
    let down = sample.footprint.zw;
    let lx = length(across);
    let ly = length(down);
    let major = max(lx, ly);
    if major <= 1.0 || base.alpha == 0u {
        return base;
    }
    let place = atlas_place(p);
    let minor = max(min(lx, ly), major / f32(u.mips.y));
    let probes = ceil(major / minor - 0.001);
    let taps = u32(probes) * 2u;
    let last = (place >> 18u) & 0xFu;
    let lod = clamp(log2(major / probes), 0.0, f32(last));
    let fine = u32(floor(lod));
    let blend = lod - floor(lod);
    let coarse = min(fine + 1u, last);
    let axis = select(down, across, lx >= ly);
    let uv = vec2<f32>(sample.texcoord) / 16.0;
    var sum = vec4<f32>(0.0);
    for (var i = 0u; i < taps; i = i + 1u) {
        let at = uv + axis * ((f32(i) + 0.5) / f32(taps) - 0.5);
        var tap = atlas_level(place, p.texture, fine, at);
        if blend > 0.0 && coarse > fine {
            tap = mix(tap, atlas_level(place, p.texture, coarse, at), blend);
        }
        sum = sum + tap;
    }
    if sum.w <= 0.0 {
        return base;
    }
    let color = vec3<u32>(clamp(round(sum.xyz / sum.w), vec3<f32>(0.0), vec3<f32>(31.0)));
    var out = base;
    out.color = color.x | (color.y << 5u) | (color.z << 10u);
    return out;
}

fn expand(channel: u32) -> u32 {
    let level = channel & 0x1Fu;
    if level == 0u {
        return 0u;
    }
    return level * 2u + 1u;
}

fn shade(p: Poly, vertex: vec3<u32>, alpha: u32, textured: bool, texel: Texel) -> vec4<u32> {
    if !textured {
        return vec4<u32>(vertex, alpha);
    }
    let texture = vec3<u32>(expand(texel.color), expand(texel.color >> 5u), expand(texel.color >> 10u));
    let m = mode(p);
    if m == DECAL || m == SHADOW {
        if texel.alpha == 0u {
            return vec4<u32>(vertex, alpha);
        }
        if texel.alpha == OPAQUE {
            return vec4<u32>(texture, alpha);
        }
        return vec4<u32>((texture * texel.alpha + vertex * (OPAQUE - texel.alpha)) >> vec3<u32>(5u), alpha);
    }
    let modulated = ((texture + 1u) * (vertex + 1u) - 1u) >> vec3<u32>(6u);
    return vec4<u32>(modulated, ((texel.alpha + 1u) * (alpha + 1u) - 1u) >> 5u);
}

struct State {
    color: vec4<u32>,
    depth: i32,
    opaque_id: u32,
    translucent_id: u32,
    fog: bool,
    back: bool,
    edges: u32,
    stencil: bool,
    winner: u32,
    fringe: bool,
    rim: bool,
    under: u32,
    under_depth: i32,
}

fn alpha_threshold() -> u32 {
    if (u.frame.y & ALPHA_TEST) != 0u {
        return u.frame.z;
    }
    return 0u;
}

fn depth_test(p: Poly, d: i32, state: State) -> bool {
    if (p.attributes & DEPTH_EQUAL) != 0u {
        let tolerance = select(EQUAL_Z, EQUAL_W, p.w_buffer);
        return abs(state.depth - d) <= tolerance;
    }
    if p.front && state.back && state.translucent_id == NO_ID {
        return d <= state.depth;
    }
    return d < state.depth;
}

fn over(source: vec4<u32>, below: vec4<u32>) -> vec4<u32> {
    if below.w == 0u {
        return source;
    }
    var out = source;
    if (u.frame.y & BLENDING) != 0u {
        let weight = source.w + 1u;
        out = vec4<u32>((source.xyz * weight + below.xyz * (32u - weight)) >> vec3<u32>(5u), source.w);
    }
    out.w = max(source.w, below.w);
    return out;
}

fn apply(p: Poly, sample: Sample, row: u32, state: ptr<function, State>) {
    let id = polygon_id(p.attributes);
    let m = mode(p);
    if m == SHADOW && id == 0u {
        if mask_clears(p.record, row) {
            (*state).stencil = false;
        }
        if !sample.covered || polygon_alpha(p) <= alpha_threshold() {
            return;
        }
        if !depth_test(p, sample.depth, *state) {
            (*state).stencil = true;
        }
        return;
    }
    if !sample.covered {
        return;
    }
    let shadow = m == SHADOW;
    if shadow && !(*state).stencil {
        return;
    }
    if sample.fringe && (*state).winner != NONE && !(*state).fringe {
        return;
    }
    let over_fringe = (*state).fringe && !sample.fringe;
    var beneath = false;
    if !over_fringe && !depth_test(p, sample.depth, *state) {
        if !(*state).rim || sample.depth >= (*state).under_depth {
            return;
        }
        beneath = true;
    }
    let alpha = raw_alpha(p);
    let textured = (u.frame.y & TEXTURES) != 0u && ((p.texture >> 26u) & 7u) != 0u;
    var texel: Texel;
    if textured {
        texel = texture_of(p, sample);
    }
    var pixel = shade(p, sample.color, alpha, textured, texel);
    if alpha == 0u {
        pixel.w = OPAQUE;
    }
    if pixel.w <= alpha_threshold() {
        return;
    }
    if pixel.w != OPAQUE {
        var same = false;
        if (*state).translucent_id != NO_ID {
            same = (*state).translucent_id == id;
        } else {
            same = shadow && (*state).opaque_id == id;
        }
        if same {
            return;
        }
        if beneath || (*state).rim {
            (*state).under = packed(over(pixel, channels((*state).under)));
        }
        if beneath {
            return;
        }
        (*state).color = over(pixel, (*state).color);
        if (p.attributes & TRANSLUCENT_DEPTH) != 0u {
            (*state).depth = sample.depth;
        }
        (*state).translucent_id = id;
        (*state).fog = (*state).fog && (p.attributes & FOG) != 0u;
        (*state).back = !p.front;
        return;
    }
    if beneath {
        (*state).under = packed(pixel);
        (*state).under_depth = sample.depth;
        return;
    }
    (*state).under = packed((*state).color);
    (*state).under_depth = (*state).depth;
    (*state).depth = sample.depth;
    (*state).color = pixel;
    (*state).opaque_id = id;
    (*state).translucent_id = NO_ID;
    (*state).fog = (p.attributes & FOG) != 0u;
    (*state).back = !p.front;
    (*state).edges = sample.edges;
    (*state).rim = sample.rim;
    (*state).winner = p.record;
    (*state).fringe = sample.fringe;
}

fn rear_state() -> State {
    var state: State;
    let pixel = u.rear.x;
    state.color = vec4<u32>(pixel & 0xFFu, (pixel >> 8u) & 0xFFu, (pixel >> 16u) & 0xFFu, pixel >> 24u);
    state.depth = bitcast<i32>(u.rear.y);
    let bits = u.rear.z;
    state.opaque_id = bits & 0x3Fu;
    state.translucent_id = NO_ID;
    state.fog = (bits & 0x2000u) != 0u;
    state.back = false;
    state.edges = 0u;
    state.stencil = false;
    state.winner = NONE;
    state.fringe = false;
    state.rim = false;
    state.under = pixel;
    state.under_depth = state.depth;
    return state;
}

const HW_RIM: u32 = 0x80000u;
const HW_FRINGE: u32 = 0x100000u;
const HW_COVERED: u32 = 0x200000u;

fn opaque_at(at: vec2<i32>) -> u32 {
    let t = textureLoad(reference, at, 0);
    if (t.z & HW_COVERED) == 0u || (t.z & HW_FRINGE) != 0u {
        return NONE;
    }
    return t.w;
}

fn opaque_winner(x: i32, y: i32) -> u32 {
    let scale = i32(u.frame.x);
    let at = vec2<i32>(clamp(x, 0, WIDTH - 1), clamp(y, 0, HEIGHT - 1)) * scale;
    let corner = opaque_at(at);
    if corner != NONE {
        return corner;
    }
    return opaque_at(at + vec2<i32>(scale / 2));
}

fn covered_reference(x: i32, y: i32) -> bool {
    return opaque_winner(x, y) != NONE;
}

fn segment_nearest(point: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let along = b - a;
    let length = dot(along, along);
    var t = 0.0;
    if length > 0.0 {
        t = clamp(dot(point - a, along) / length, 0.0, 1.0);
    }
    return a + along * t;
}

fn boundary_nearest(p: Poly, point: vec2<f32>) -> vec2<f32> {
    var nearest = fine(p, 0u);
    for (var i = 0u; i < p.count; i = i + 1u) {
        let candidate = segment_nearest(point, fine(p, i), fine(p, step_index(p, i, true)));
        if distance(point, candidate) < distance(point, nearest) {
            nearest = candidate;
        }
    }
    return nearest;
}

fn between_polygons(point: vec2<f32>, start: u32, count: u32) -> bool {
    var directions: array<vec2<f32>, 4>;
    var near = 0u;
    for (var i = 0u; i < count; i = i + 1u) {
        let p = load(entry(start, i).record);
        if p.degenerate || p.count < 3u || raw_alpha(p) != OPAQUE || mode(p) == SHADOW {
            continue;
        }
        let toward = boundary_nearest(p, point) - point;
        if length(toward) > HOLE {
            continue;
        }
        for (var k = 0u; k < near; k = k + 1u) {
            if dot(directions[k], toward) < 0.0 {
                return true;
            }
        }
        if near < 4u {
            directions[near] = toward;
            near = near + 1u;
        }
    }
    return false;
}

fn fill_hole(point: vec2<f32>, pixel: f32, native: vec2<i32>, start: u32, count: u32, state: ptr<function, State>) {
    if (*state).winner != NONE {
        return;
    }
    let winner = opaque_winner(native.x, native.y);
    if winner == NONE {
        return;
    }
    if !(covered_reference(native.x - 1, native.y) && covered_reference(native.x + 1, native.y) && covered_reference(native.x, native.y - 1) && covered_reference(native.x, native.y + 1)) {
        return;
    }
    if !between_polygons(point, start, count) {
        return;
    }
    let p = load(winner);
    let sample = upscaled_sample(p, point, pixel, true);
    apply(p, sample, u32(native.y), state);
}

fn pack(state: State) -> vec4<u32> {
    let color = state.color.x | (state.color.y << 8u) | (state.color.z << 16u) | (state.color.w << 24u);
    var bits = state.opaque_id | (state.translucent_id << 6u) | (state.edges << 15u);
    if state.fog {
        bits = bits | 0x2000u;
    }
    if state.back {
        bits = bits | 0x4000u;
    }
    return vec4<u32>(color, bitcast<u32>(state.depth), bits, select(state.winner, state.under, u.options.y != 0u));
}

fn shade_point(point: vec2<f32>, pixel: f32, native: vec2<i32>, upscaled: bool) -> State {
    let tile = u32(native.y / TILE) * u.offsets.w + u32(native.x / TILE);
    let start = word(tile * 2u);
    let count = word(tile * 2u + 1u);
    var state = rear_state();
    var filled = !upscaled;
    for (var i = 0u; i < count; i = i + 1u) {
        let e = entry(start, i);
        if !filled && e.translucent {
            fill_hole(point, pixel, native, start, count, &state);
            filled = true;
        }
        var near = false;
        if !upscaled {
            near = on_rows(e, native.y) && on_columns(e, native.x);
        } else {
            near = reaches(e, point, near_slack(e, pixel));
        }
        if !near {
            if e.mask && mask_clears(e.record, u32(native.y)) {
                state.stencil = false;
            }
            continue;
        }
        let p = load(e.record);
        var sample = Sample();
        if !upscaled {
            sample = reference_sample(p, native.x, native.y);
        } else {
            sample = upscaled_sample(p, point, pixel, false);
        }
        apply(p, sample, u32(native.y), &state);
    }
    if !filled {
        fill_hole(point, pixel, native, start, count, &state);
    }
    return state;
}

const CHUNK: u32 = 8u;

var<workgroup> tile_list: vec2<u32>;
var<workgroup> chunk_entries: array<Entry, CHUNK>;
var<workgroup> chunk_polys: array<Poly, CHUNK>;
var<workgroup> chunk_rows: array<RowSetup, CHUNK * GROUP_SIDE>;

fn shade_tile(corner: vec2<i32>, native: vec2<i32>, index: u32) -> State {
    if index == 0u {
        let tile = u32(corner.y / TILE) * u.offsets.w + u32(corner.x / TILE);
        tile_list = vec2<u32>(word(tile * 2u), word(tile * 2u + 1u));
    }
    let list = workgroupUniformLoad(&tile_list);
    let start = list.x;
    let count = list.y;
    let columns = vec2<i32>(corner.x, corner.x + TILE - 1);
    let line = u32(native.y - corner.y);
    var state = rear_state();
    for (var base = 0u; base < count; base = base + CHUNK) {
        let size = min(CHUNK, count - base);
        for (var k = index; k < CHUNK * GROUP_SIDE; k = k + GROUP_SIDE * GROUP_SIDE) {
            let j = k / GROUP_SIDE;
            if j < size {
                let e = entry(start, base + j);
                let p = load(e.record);
                if k % GROUP_SIDE == 0u {
                    chunk_entries[j] = e;
                    chunk_polys[j] = p;
                }
                let y = corner.y + i32(k % GROUP_SIDE);
                if on_rows(e, y) {
                    chunk_rows[k] = reference_row(p, y, columns);
                }
            }
        }
        workgroupBarrier();
        for (var j = 0u; j < size; j = j + 1u) {
            let e = chunk_entries[j];
            if on_rows(e, native.y) && on_columns(e, native.x) {
                let p = chunk_polys[j];
                let sample = reference_tail(p, chunk_rows[j * GROUP_SIDE + line], native.x);
                apply(p, sample, u32(native.y), &state);
            } else if e.mask && mask_clears(e.record, u32(native.y)) {
                state.stencil = false;
            }
        }
        workgroupBarrier();
    }
    return state;
}

fn sample_step(samples: u32, index: u32) -> vec2<i32> {
    var four = array<vec2<i32>, 4>(
        vec2<i32>(-2, -6), vec2<i32>(6, -2), vec2<i32>(-6, 2), vec2<i32>(2, 6)
    );
    var eight = array<vec2<i32>, 8>(
        vec2<i32>(1, -3), vec2<i32>(-1, 3), vec2<i32>(5, 1), vec2<i32>(-3, -5),
        vec2<i32>(-5, 5), vec2<i32>(-7, -1), vec2<i32>(3, 7), vec2<i32>(7, -7)
    );
    if samples == 4u {
        return four[index];
    }
    return eight[index];
}

fn sample_point(at: vec2<u32>, pixel: f32, samples: u32, index: u32) -> vec2<f32> {
    return vec2<f32>(vec2<i32>(at * 16u + 8u) + sample_step(samples, index)) * (pixel / 16.0);
}

fn sample_middle(at: vec2<u32>, pixel: f32) -> vec2<f32> {
    return vec2<f32>(at * 16u + 8u) * (pixel / 16.0);
}

fn antialiased(at: vec2<u32>, pixel: f32, native: vec2<i32>, samples: u32) -> State {
    let tile = u32(native.y / TILE) * u.offsets.w + u32(native.x / TILE);
    let start = word(tile * 2u);
    let count = word(tile * 2u + 1u);
    let row = u32(native.y);
    let middle = sample_middle(at, pixel);
    var points: array<vec2<f32>, 8>;
    var states: array<State, 8>;
    for (var index = 0u; index < samples; index = index + 1u) {
        points[index] = sample_point(at, pixel, samples, index);
        states[index] = rear_state();
    }
    var filled = false;
    for (var i = 0u; i < count; i = i + 1u) {
        let e = entry(start, i);
        if !filled && e.translucent {
            for (var index = 0u; index < samples; index = index + 1u) {
                var state = states[index];
                fill_hole(points[index], pixel, native, start, count, &state);
                states[index] = state;
            }
            filled = true;
        }
        let near = reaches(e, middle, near_slack(e, pixel) + 0.4375 * pixel);
        if !near {
            if e.mask && mask_clears(e.record, row) {
                for (var index = 0u; index < samples; index = index + 1u) {
                    states[index].stencil = false;
                }
            }
            continue;
        }
        let p = load(e.record);
        for (var index = 0u; index < samples; index = index + 1u) {
            var sample = Sample();
            if reaches(e, points[index], near_slack(e, pixel)) {
                sample = upscaled_sample(p, points[index], pixel, false);
            }
            var state = states[index];
            apply(p, sample, row, &state);
            states[index] = state;
        }
    }
    var weighted = vec3<u32>(0u);
    var plain = vec3<u32>(0u);
    var coverage = 0u;
    for (var index = 0u; index < samples; index = index + 1u) {
        var state = states[index];
        if !filled {
            fill_hole(points[index], pixel, native, start, count, &state);
        }
        if index == 0u {
            states[0] = state;
        }
        weighted = weighted + state.color.xyz * state.color.w;
        plain = plain + state.color.xyz;
        coverage = coverage + state.color.w;
    }
    var first = states[0];
    var color = (plain + samples / 2u) / samples;
    if coverage != 0u {
        color = (weighted + coverage / 2u) / coverage;
    }
    first.color = vec4<u32>(color, (coverage + samples / 2u) / samples);
    return first;
}

fn differs(a: u32, b: u32) -> bool {
    for (var shift = 0u; shift < 24u; shift = shift + 8u) {
        let x = (a >> shift) & 0xFFu;
        let y = (b >> shift) & 0xFFu;
        if max(x, y) - min(x, y) > CONTRAST {
            return true;
        }
    }
    return (a >> 24u) != (b >> 24u);
}

fn surface(center: vec4<u32>) -> u32 {
    return center.z & 0x1FFFu;
}

fn depth_of(center: vec4<u32>) -> i32 {
    return bitcast<i32>(center.y);
}

fn apart(center: vec4<u32>, near: vec4<u32>) -> bool {
    return surface(near) != surface(center) && differs(center.x, near.x);
}

fn crease(center: vec4<u32>, a: vec4<u32>, b: vec4<u32>) -> bool {
    if !differs(center.x, a.x) && !differs(center.x, b.x) {
        return false;
    }
    let bend = abs(depth_of(a) + depth_of(b) - 2 * depth_of(center));
    return bend > abs(depth_of(a) - depth_of(b)) / 2 + i32(JUMP);
}

const GROUP_SIDE: u32 = 8u;
const BLOCK_SIDE: u32 = 10u;

var<workgroup> block: array<vec4<u32>, 100>;

fn block_at(at: vec2<i32>) -> vec4<u32> {
    return block[u32(at.y) * BLOCK_SIDE + u32(at.x)];
}

fn on_edge(at: vec2<u32>, center: vec4<u32>) -> bool {
    let size = vec2<i32>(textureDimensions(centers)) - 1;
    var steps = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1));
    for (var i = 0; i < 4; i = i + 1) {
        let a = textureLoad(centers, clamp(vec2<i32>(at) + steps[i], vec2<i32>(0), size), 0);
        let b = textureLoad(centers, clamp(vec2<i32>(at) - steps[i], vec2<i32>(0), size), 0);
        if apart(center, a) || apart(center, b) || crease(center, a, b) {
            return true;
        }
    }
    return false;
}

fn on_block_edge(local: vec2<i32>, center: vec4<u32>) -> bool {
    var steps = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(1, -1));
    for (var i = 0; i < 4; i = i + 1) {
        let a = block_at(local + steps[i]);
        let b = block_at(local - steps[i]);
        if apart(center, a) || apart(center, b) || crease(center, a, b) {
            return true;
        }
    }
    return false;
}

struct Spot {
    at: vec2<u32>,
    native: vec2<i32>,
    pixel: f32,
}

fn spot(position: vec4<f32>) -> Spot {
    let scale = u.frame.x;
    let at = vec2<u32>(position.xy);
    return Spot(at, vec2<i32>(at / scale), 1.0 / f32(scale));
}

@fragment
fn fs_raster(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let s = spot(position);
    return pack(shade_point(vec2<f32>(s.at) * s.pixel, s.pixel, s.native, u.frame.x > 1u));
}

@compute @workgroup_size(8, 8)
fn cs_raster(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>, @builtin(local_invocation_index) index: u32) {
    let corner = vec2<i32>(group.xy * GROUP_SIDE);
    let native = corner + vec2<i32>(local.xy);
    let state = shade_tile(corner, native, index);
    if all(native < vec2<i32>(WIDTH, HEIGHT)) {
        textureStore(edged, native, pack(state));
    }
}

@fragment
fn fs_edges(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let s = spot(position);
    let center = textureLoad(centers, s.at, 0);
    if !on_edge(s.at, center) {
        return center;
    }
    return pack(antialiased(s.at, s.pixel, s.native, u.frame.w));
}

var<workgroup> edge_count: atomic<u32>;
var<workgroup> edge_at: array<u32, 64>;
var<workgroup> sample_color: array<u32, 512>;
var<workgroup> first_packed: array<vec4<u32>, 64>;

@compute @workgroup_size(8, 8)
fn cs_edges(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>, @builtin(local_invocation_index) index: u32) {
    let size = vec2<i32>(textureDimensions(centers)) - 1;
    let corner = vec2<i32>(group.xy * GROUP_SIDE) - 1;
    for (var k = index; k < BLOCK_SIDE * BLOCK_SIDE; k = k + GROUP_SIDE * GROUP_SIDE) {
        let at = corner + vec2<i32>(i32(k % BLOCK_SIDE), i32(k / BLOCK_SIDE));
        block[k] = textureLoad(centers, clamp(at, vec2<i32>(0), size), 0);
    }
    if index == 0u {
        atomicStore(&edge_count, 0u);
    }
    workgroupBarrier();
    let at = group.xy * GROUP_SIDE + local.xy;
    let in_range = all(vec2<i32>(at) <= size);
    let inner = vec2<i32>(local.xy) + 1;
    let center = block_at(inner);
    var value = center;
    var slot = 0u;
    var edge = false;
    if in_range {
        edge = on_block_edge(inner, center);
    }
    if edge {
        slot = atomicAdd(&edge_count, 1u);
        edge_at[slot] = index;
    }
    workgroupBarrier();
    let scale = u.frame.x;
    let pixel = 1.0 / f32(scale);
    let samples = u.frame.w;
    let tasks = atomicLoad(&edge_count) * samples;
    for (var t = index; t < tasks; t = t + GROUP_SIDE * GROUP_SIDE) {
        let owner = t / samples;
        let k = t % samples;
        let spot_index = edge_at[owner];
        let spot_at = group.xy * GROUP_SIDE + vec2<u32>(spot_index % GROUP_SIDE, spot_index / GROUP_SIDE);
        let point = sample_point(spot_at, pixel, samples, k);
        let state = shade_point(point, pixel, vec2<i32>(spot_at / scale), true);
        sample_color[t] = packed(state.color);
        if k == 0u {
            first_packed[owner] = pack(state);
        }
    }
    workgroupBarrier();
    if edge {
        var weighted = vec3<u32>(0u);
        var plain = vec3<u32>(0u);
        var coverage = 0u;
        for (var k = 0u; k < samples; k = k + 1u) {
            let color = channels(sample_color[slot * samples + k]);
            weighted = weighted + color.xyz * color.w;
            plain = plain + color.xyz;
            coverage = coverage + color.w;
        }
        var color = (plain + samples / 2u) / samples;
        if coverage != 0u {
            color = (weighted + coverage / 2u) / coverage;
        }
        let alpha = (coverage + samples / 2u) / samples;
        let first = first_packed[slot];
        value = vec4<u32>(color.x | (color.y << 8u) | (color.z << 16u) | (alpha << 24u), first.y, first.z, first.w);
    }
    if in_range {
        textureStore(edged, at, value);
    }
}

fn expand5(color: u32) -> u32 {
    let level = color & 31u;
    if level == 0u {
        return 0u;
    }
    return level * 2u + 1u;
}

fn edge_color(id: u32) -> vec3<u32> {
    let index = (id >> 3u) & 7u;
    return expand_color(u.edge[index >> 1u] >> ((index & 1u) * 16u));
}

fn expand_color(color: u32) -> vec3<u32> {
    return vec3<u32>(expand5(color), expand5(color >> 5u), expand5(color >> 10u));
}

fn fog_entry(index: u32) -> u32 {
    let at = min(max(index, 1u) - 1u, 31u);
    return (u.fog_table[at >> 4u][(at >> 2u) & 3u] >> ((at & 3u) * 8u)) & 0xFFu;
}

fn fog_density(depth: u32) -> u32 {
    let offset = u.fog.y * DEPTH_STEP;
    var index = 0u;
    var fraction = 0u;
    if depth >= offset {
        let z = ((depth - offset) >> 2u) << ((u.frame.y >> 8u) & 0xFu);
        index = min(z >> 17u, 32u);
        if index < 32u {
            fraction = z & 0x1FFFFu;
        }
    }
    let density = (fog_entry(index) * (0x20000u - fraction) + fog_entry(index + 1u) * fraction) >> 17u;
    if density >= 127u {
        return 128u;
    }
    return density;
}

fn channels(color: u32) -> vec4<u32> {
    return vec4<u32>(color & 0xFFu, (color >> 8u) & 0xFFu, (color >> 16u) & 0xFFu, color >> 24u);
}

fn packed(color: vec4<u32>) -> u32 {
    return color.x | (color.y << 8u) | (color.z << 16u) | (color.w << 24u);
}

fn neighbour(at: vec2<i32>) -> vec4<u32> {
    let size = vec2<i32>(textureDimensions(centers));
    if any(at < vec2<i32>(0)) || any(at >= size) {
        return vec4<u32>(u.rear.x, u.rear.y, u.rear.z, NONE);
    }
    return textureLoad(centers, at, 0);
}

fn marked(at: vec2<i32>, here: vec4<u32>) -> vec4<u32> {
    let id = here.z & 0x3Fu;
    let depth = bitcast<i32>(here.y);
    let step = i32(u.frame.x);
    var offsets = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var i = 0u; i < 4u; i = i + 1u) {
        let other = neighbour(at + offsets[i] * step);
        if (other.z & 0x3Fu) != id && depth < bitcast<i32>(other.y) {
            let top = vec4<u32>(edge_color(id), channels(here.x).w);
            if (u.frame.y & ANTIALIASING) == 0u {
                return top;
            }
            let below = channels(here.w);
            var out = (top * EDGE_WEIGHT + below * (32u - EDGE_WEIGHT)) >> vec4<u32>(5u);
            if below.w == 0u {
                out = vec4<u32>(top.xyz, out.w);
            }
            return out;
        }
    }
    return channels(here.x);
}

fn fogged(color: vec4<u32>, depth: u32) -> vec4<u32> {
    let density = fog_density(depth);
    let fog = vec4<u32>(expand_color(u.fog.x), (u.fog.x >> 16u) & 31u);
    let mixed = (fog * density + color * (128u - density)) >> vec4<u32>(7u);
    if (u.frame.y & FOG_ALPHA_ONLY) != 0u {
        return vec4<u32>(color.xyz, mixed.w);
    }
    return mixed;
}

@fragment
fn fs_finish(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let at = vec2<i32>(position.xy);
    let here = textureLoad(centers, at, 0);
    var color = channels(here.x);
    if (u.frame.y & EDGE_MARKING) != 0u && ((here.z >> 15u) != 0u || u.frame.x > 1u) {
        color = marked(at, here);
    }
    if (u.frame.y & FOG_ENABLE) != 0u && (here.z & 0x2000u) != 0u {
        color = fogged(color, here.y);
    }
    return vec4<u32>(packed(color), here.y, here.z, here.w);
}


const FLAT: u32 = 6u;
const CLOCKWISE: u32 = 0x100000u;
const NO_AREA: u32 = 0x200000u;
const SKIRT: f32 = 0.140625;
const INSET: f32 = 0.015625;
const SLAB: u32 = 0x80000000u;
const EXACT: u32 = 0x40000000u;
const SLACK: f32 = 0.015625;

struct HwOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) head: vec4<u32>,
    @location(1) @interpolate(flat) place: vec4<u32>,
    @location(2) @interpolate(flat) corners: vec4<u32>,
    @location(3) @interpolate(flat) la: vec4<u32>,
    @location(4) @interpolate(flat) lb: vec4<u32>,
    @location(5) @interpolate(flat) ra: vec4<u32>,
    @location(6) @interpolate(flat) rb: vec4<u32>,
}

struct HwTargets {
    @location(0) state: vec4<u32>,
    @builtin(frag_depth) depth: f32,
}

fn hw_clip(position: vec2<f32>) -> vec4<f32> {
    let moved = position + vec2<f32>(f32(u.options.z) * 0.5 / f32(u.frame.x));
    return vec4<f32>(moved.x / 128.0 - 1.0, 1.0 - moved.y / 96.0, 0.5, 1.0);
}

fn slab_side(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>, y: f32, rightmost: bool) -> f32 {
    let l0 = edge_x(a, b, y - SLACK);
    let l1 = edge_x(a, b, y + SLACK);
    let r0 = edge_x(c, d, y - SLACK);
    let r1 = edge_x(c, d, y + SLACK);
    if rightmost {
        return max(max(l0, l1), max(r0, r1)) + SLACK;
    }
    return min(min(l0, l1), min(r0, r1)) - SLACK;
}

fn values_word(first: vec4<u32>, second: vec4<u32>) -> vec4<u32> {
    return vec4<u32>(first.z, first.w, second.x, second.y);
}

@vertex
fn vs_hw(@builtin(vertex_index) index: u32, @location(0) instance: vec2<u32>) -> HwOut {
    let record = instance.x;
    let head = texel(record);
    let p = poly_of(record, head);
    var out: HwOut;
    out.head = head;
    out.place = vec4<u32>(record, instance.y, 0u, 0u);
    out.corners = vec4<u32>(0u);
    out.la = vec4<u32>(0u);
    out.lb = vec4<u32>(0u);
    out.ra = vec4<u32>(0u);
    out.rb = vec4<u32>(0u);
    out.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    if (instance.y & SLAB) != 0u {
        let ends = (vec4<u32>(instance.y) >> vec4<u32>(0u, 4u, 8u, 12u)) & vec4<u32>(15u);
        let fa = vertex_first(p, ends.x);
        let fb = vertex_first(p, ends.y);
        let fc = vertex_first(p, ends.z);
        let fd = vertex_first(p, ends.w);
        let upper = vertex_word(p, (instance.y >> 16u) & 15u, 1u) >> 16u;
        let lower = vertex_word(p, (instance.y >> 20u) & 15u, 1u) >> 16u;
        let first_row = vertex_word(p, p.fine_top, 1u) >> 16u;
        let last_row = vertex_word(p, p.fine_bottom, 1u) >> 16u;
        out.place = vec4<u32>(record, instance.y, first_row | (last_row << 16u), upper | (lower << 16u));
        out.corners = vec4<u32>(fa.y, fb.y, fc.y, fd.y);
        out.la = values_word(fa, vertex_second(p, ends.x));
        out.lb = values_word(fb, vertex_second(p, ends.y));
        out.ra = values_word(fc, vertex_second(p, ends.z));
        out.rb = values_word(fd, vertex_second(p, ends.w));
        var y = f32(upper) / 16.0 - SLACK;
        if index == 2u || index >= 4u {
            y = f32(lower) / 16.0 + SLACK;
        }
        let rightmost = index == 1u || index == 2u || index == 4u;
        let x = slab_side(fine_of(fa.y), fine_of(fb.y), fine_of(fc.y), fine_of(fd.y), y, rightmost);
        out.position = hw_clip(vec2<f32>(x, y));
        return out;
    }
    if index < FLAT {
        let top = fine(p, p.fine_top).y;
        if top != fine(p, p.fine_bottom).y {
            return out;
        }
        let xl = min(fine(p, 0u).x, min(fine(p, 1u).x, fine(p, p.count - 1u).x));
        let xr = max(fine(p, 0u).x, max(fine(p, 1u).x, fine(p, p.count - 1u).x));
        let low = top + 1.0 / f32(u.frame.x);
        var at = vec2<f32>(xl, top);
        if index == 1u {
            at = vec2<f32>(xr, top);
        } else if index == 2u || index == 4u {
            at = vec2<f32>(xr, low);
        } else if index == 5u {
            at = vec2<f32>(xl, low);
        }
        out.position = hw_clip(at);
        return out;
    }
    let side = (index - FLAT) / 6u;
    let corner = (index - FLAT) % 6u;
    if side >= p.count {
        return out;
    }
    let a = fine(p, side);
    let b = fine(p, step_index(p, side, true));
    let reach = distance(a, b);
    if reach == 0.0 {
        return out;
    }
    let flags = head.z;
    let along = (b - a) / reach * SKIRT;
    var outward = vec2<f32>(along.y, -along.x);
    if (flags & CLOCKWISE) != 0u {
        outward = -outward;
    }
    var inward = -outward * (INSET / SKIRT);
    if (flags & NO_AREA) != 0u {
        inward = -outward;
    }
    var at = a - along + inward;
    if corner == 1u {
        at = b + along + inward;
    } else if corner == 2u || corner == 4u {
        at = b + along + outward;
    } else if corner == 5u {
        at = a - along + outward;
    }
    out.position = hw_clip(at);
    return out;
}

@fragment
fn fs_hw(in: HwOut) -> HwTargets {
    let record = in.place.x;
    let p = poly_of(record, in.head);
    let pixel = 1.0 / f32(u.frame.x);
    let point = (in.position.xy - vec2<f32>(0.5 * f32(u.options.z))) * pixel;
    var sample: Sample;
    if (in.place.y & (SLAB | EXACT)) == SLAB {
        sample = slab_sample(p, in.place.zw, in.corners, in.la, in.lb, in.ra, in.rb, point, pixel);
    } else {
        sample = upscaled_sample(p, point, pixel, false);
    }
    if !sample.covered || sample.depth >= bitcast<i32>(u.rear.y) {
        discard;
    }
    let fringe = sample.fringe;
    let textured = (u.frame.y & TEXTURES) != 0u && ((p.texture >> 26u) & 7u) != 0u;
    var texel: Texel;
    if textured {
        texel = texture_of(p, sample);
    }
    let color = shade(p, sample.color, raw_alpha(p), textured, texel);
    if color.w <= alpha_threshold() {
        discard;
    }
    var bits = polygon_id(p.attributes) | (NO_ID << 6u) | (sample.edges << 15u) | HW_COVERED;
    if (p.attributes & FOG) != 0u {
        bits = bits | 0x2000u;
    }
    if !p.front {
        bits = bits | 0x4000u;
    }
    if sample.rim {
        bits = bits | HW_RIM;
    }
    if fringe {
        bits = bits | HW_FRINGE;
    }
    var out: HwTargets;
    out.state = vec4<u32>(packed(vec4<u32>(color.xyz, OPAQUE)), bitcast<u32>(sample.depth), bits, record);
    out.depth = clamp(f32(sample.depth), 0.0, 16777215.0) / 33554432.0 + select(0.0, 0.5, fringe);
    return out;
}

fn hw_state(t: vec4<u32>) -> State {
    if (t.z & HW_COVERED) == 0u {
        return rear_state();
    }
    var state: State;
    state.color = channels(t.x);
    state.depth = bitcast<i32>(t.y);
    state.opaque_id = t.z & 0x3Fu;
    state.translucent_id = NO_ID;
    state.fog = (t.z & 0x2000u) != 0u;
    state.back = (t.z & 0x4000u) != 0u;
    state.edges = (t.z >> 15u) & 0xFu;
    state.stencil = false;
    state.winner = t.w;
    state.fringe = (t.z & HW_FRINGE) != 0u;
    state.rim = (t.z & HW_RIM) != 0u;
    state.under = u.rear.x;
    state.under_depth = bitcast<i32>(u.rear.y);
    return state;
}

@fragment
fn fs_tail(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let s = spot(position);
    let point = (vec2<f32>(s.at) + vec2<f32>(0.5 * f32(1u - u.options.z))) * s.pixel;
    let here = textureLoad(centers, s.at, 0);
    let blends = EDGE_MARKING | ANTIALIASING;
    if (u.frame.y & blends) == blends && (here.z & (HW_COVERED | HW_RIM)) == (HW_COVERED | HW_RIM) {
        return pack(shade_point(point, s.pixel, s.native, true));
    }
    var state = hw_state(here);
    let tile = u32(s.native.y / TILE) * u.offsets.w + u32(s.native.x / TILE);
    let start = word(tile * 2u);
    let count = word(tile * 2u + 1u);
    let late_start = word(u.rear.w + tile * 2u);
    let late_count = word(u.rear.w + tile * 2u + 1u);
    let row = u32(s.native.y);
    var filled = false;
    for (var i = 0u; i < late_count; i = i + 1u) {
        let e = entry(late_start, i);
        if !filled && e.translucent {
            fill_hole(point, s.pixel, s.native, start, count, &state);
            filled = true;
        }
        if !reaches(e, point, near_slack(e, s.pixel)) {
            if e.mask && mask_clears(e.record, row) {
                state.stencil = false;
            }
            continue;
        }
        let p = load(e.record);
        let sample = upscaled_sample(p, point, s.pixel, false);
        apply(p, sample, row, &state);
    }
    if !filled {
        fill_hole(point, s.pixel, s.native, start, count, &state);
    }
    return pack(state);
}

struct AtlasOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) entry: vec4<u32>,
}

@vertex
fn vs_atlas(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> AtlasOut {
    let level = index / 6u;
    let corner = index % 6u;
    let e = texel(u.mips.x + instance * 4u);
    var out: AtlasOut;
    out.entry = e;
    out.position = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    if level >= e.w {
        return out;
    }
    let scale = f32(1u << level);
    let origin = vec2<f32>(f32(e.x & 0xFFFFu), f32(e.x >> 16u)) / scale;
    let size = vec2<f32>(f32(8u << ((e.y >> 20u) & 7u)), f32(8u << ((e.y >> 23u) & 7u))) / scale;
    var at = origin;
    if corner == 1u || corner == 2u || corner == 4u {
        at.x = origin.x + size.x;
    }
    if corner == 2u || corner >= 4u {
        at.y = origin.y + size.y;
    }
    let dims = vec2<f32>(f32(u.mips.z >> level), f32(u.mips.w >> level));
    out.position = vec4<f32>(at.x / dims.x * 2.0 - 1.0, 1.0 - at.y / dims.y * 2.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_decode(in: AtlasOut) -> @location(0) vec4<u32> {
    let at = vec2<u32>(in.position.xy) - vec2<u32>(in.entry.x & 0xFFFFu, in.entry.x >> 16u);
    let texel = sample_texture(in.entry.y, in.entry.z, i32(at.x << 4u), i32(at.y << 4u));
    let color = vec3<u32>(texel.color & 0x1Fu, (texel.color >> 5u) & 0x1Fu, (texel.color >> 10u) & 0x1Fu);
    return vec4<u32>(color * texel.alpha * 64u, texel.alpha * 64u);
}

@fragment
fn fs_reduce(in: AtlasOut) -> @location(0) vec4<u32> {
    let at = vec2<i32>(in.position.xy) * 2;
    let sum = textureLoad(atlas, at, 0) + textureLoad(atlas, at + vec2<i32>(1, 0), 0)
        + textureLoad(atlas, at + vec2<i32>(0, 1), 0) + textureLoad(atlas, at + vec2<i32>(1, 1), 0);
    return (sum + vec4<u32>(2u)) / 4u;
}
