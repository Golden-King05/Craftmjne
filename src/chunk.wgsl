// Chunk fragment shader: unlit atlas texture x baked voxel light, with alpha
// cutout (leaves/glass), water translucency, and distance fog toward the sky
// color. The vertex stage is Bevy's standard mesh vertex shader.
//
// The two light sources the mesher baked into the vertex color are combined
// here rather than there, because only one of them is constant:
//   in.color.rgb - colored block light (torches), already shaded/AO'd and
//                  floored at the ambient minimum. Fixed at mesh time; a
//                  torch is exactly as bright at noon as at midnight.
//   in.color.a   - sky light's RED channel for this vertex, on the same
//   in.uv_b.xy      shading scale; green and blue follow in the second UV
//                  set. Three channels rather than one because media tint
//                  what passes through them - water eats red and keeps blue,
//                  so a pool floor is lit blue-green at noon rather than
//                  merely darker. They're stored as ratios, then scaled and
//                  tinted every frame by params.sky_light, which `sky.rs`
//                  drives from the day/night cycle (and from a special
//                  moon's color), so the whole world's daylight still
//                  changes with zero relighting or remeshing.
//                  uv_b carries light, not texture coordinates - chunks
//                  sample a single atlas through uv, and this was the free
//                  interpolated slot. See `mesher::MeshBucket::sky_gb`.
// max() rather than a sum: a torch shouldn't visibly brighten a surface
// that's already in full daylight, it should just take over once the sun
// goes down - per channel, so a colored light still reads as colored
// against a dimly sky-lit surface.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct ChunkParams {
    fog_color: vec4<f32>,
    // rgb: the sky's current light color and brightness. a: unused.
    sky_light: vec4<f32>,
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
    let sky = vec3<f32>(in.color.a, in.uv_b.x, in.uv_b.y) * params.sky_light.rgb;
    let light = max(in.color.rgb, sky);
    let lit = color.rgb * light;
    let dist = distance(view.world_position, in.world_position.xyz);
    let fog = smoothstep(params.fog_start, params.fog_end, dist);
    return vec4<f32>(mix(lit, params.fog_color.rgb, fog), color.a * params.base_alpha);
}
