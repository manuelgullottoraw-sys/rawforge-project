// color_grade.wgsl — stage di color grading (docs/ARCHITECTURE.md, §6.2):
// esposizione, contrasto e HSL per-banda (8 bande Red..Magenta) in un solo
// compute pass. Gli array a 8 elementi sono impacchettati come 2x vec4<f32>
// per rispettare lo stride a 16 byte richiesto dagli array in uniform buffer.

struct GradeParams {
    exposure_mul: f32,
    contrast: f32,
    saturation: f32,
    _padding: f32,
    hsl_hue_shift: array<vec4<f32>, 2>,
    hsl_sat_mul: array<vec4<f32>, 2>,
    hsl_lum_shift: array<vec4<f32>, 2>,
};

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: GradeParams;

fn sample_band(packed: array<vec4<f32>, 2>, index: u32) -> f32 {
    let vec_index = index / 4u;
    let component = index % 4u;
    // Indicizzazione dinamica di array in uniform buffer non supportata:
    // selezione esplicita del vec4 (solo 2 possibilità) via branch statico.
    var v = packed[0];
    if (vec_index == 1u) {
        v = packed[1];
    }
    if (component == 0u) { return v.x; }
    if (component == 1u) { return v.y; }
    if (component == 2u) { return v.z; }
    return v.w;
}

fn rgb_to_hsl(c: vec3<f32>) -> vec3<f32> {
    let maxc = max(c.r, max(c.g, c.b));
    let minc = min(c.r, min(c.g, c.b));
    let l = (maxc + minc) * 0.5;
    var h = 0.0;
    var s = 0.0;
    let d = maxc - minc;
    if (d > 0.00001) {
        s = d / (1.0 - abs(2.0 * l - 1.0) + 0.00001);
        if (maxc == c.r) {
            h = ((c.g - c.b) / d) % 6.0;
        } else if (maxc == c.g) {
            h = (c.b - c.r) / d + 2.0;
        } else {
            h = (c.r - c.g) / d + 4.0;
        }
        h = h * 60.0;
        if (h < 0.0) {
            h = h + 360.0;
        }
    }
    return vec3<f32>(h, s, l);
}

fn hsl_to_rgb(hsl: vec3<f32>) -> vec3<f32> {
    let h = hsl.x;
    let s = hsl.y;
    let l = hsl.z;
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    let x = c * (1.0 - abs(((h / 60.0) % 2.0) - 1.0));
    let m = l - c * 0.5;
    var rgb = vec3<f32>(0.0, 0.0, 0.0);
    if (h < 60.0) {
        rgb = vec3<f32>(c, x, 0.0);
    } else if (h < 120.0) {
        rgb = vec3<f32>(x, c, 0.0);
    } else if (h < 180.0) {
        rgb = vec3<f32>(0.0, c, x);
    } else if (h < 240.0) {
        rgb = vec3<f32>(0.0, x, c);
    } else if (h < 300.0) {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }
    return rgb + vec3<f32>(m, m, m);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));

    var color = textureLoad(input_tex, coord, 0).rgb;
    color = color * params.exposure_mul;
    color = (color - vec3<f32>(0.5, 0.5, 0.5)) * params.contrast + vec3<f32>(0.5, 0.5, 0.5);

    var hsl = rgb_to_hsl(clamp(color, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0)));
    let band = u32(hsl.x / 45.0) % 8u;

    hsl.x = hsl.x + sample_band(params.hsl_hue_shift, band);
    hsl.y = clamp(hsl.y * sample_band(params.hsl_sat_mul, band) * params.saturation, 0.0, 1.0);
    hsl.z = clamp(hsl.z + sample_band(params.hsl_lum_shift, band), 0.0, 1.0);

    let out = hsl_to_rgb(hsl);
    textureStore(output_tex, coord, vec4<f32>(clamp(out, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0)), 1.0));
}
