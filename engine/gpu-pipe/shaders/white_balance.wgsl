// white_balance.wgsl — stage 1 della pipeline (docs/ARCHITECTURE.md, §3.2).
// Applica un guadagno per canale (bilanciamento del bianco) in spazio lineare.

struct WhiteBalanceParams {
    gain: vec3<f32>,
    _padding: f32,
};

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<uniform> params: WhiteBalanceParams;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(input_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) {
        return;
    }
    let coord = vec2<i32>(i32(gid.x), i32(gid.y));
    let color = textureLoad(input_tex, coord, 0);
    let balanced = vec4<f32>(color.rgb * params.gain, color.a);
    textureStore(output_tex, coord, balanced);
}
