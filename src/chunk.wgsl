// Chunk fragment shader: unlit atlas texture x baked vertex light, with
// alpha cutout (leaves/glass), water translucency, and distance fog toward
// the sky color. The vertex stage is Bevy's standard mesh vertex shader.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct ChunkParams {
    fog_color: vec4<f32>,
    fog_start: f32,
    fog_end: f32,
    base_alpha: f32,
    alpha_cutoff: f32,
}

@group(2) @binding(0) var atlas_texture: texture_2d<f32>;
@group(2) @binding(1) var atlas_sampler: sampler;
@group(2) @binding(2) var<uniform> params: ChunkParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(atlas_texture, atlas_sampler, in.uv);
    if (color.a < params.alpha_cutoff) {
        discard;
    }
    let lit = color.rgb * in.color.rgb;
    let dist = distance(view.world_position, in.world_position.xyz);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    return vec4<f32>(mix(lit, params.fog_color.rgb, fog), color.a * params.base_alpha);
}
