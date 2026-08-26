// Sun/moon fragment shader: an unlit, alpha-blended billboard - just the
// texture's own color/alpha multiplied by a tint whose alpha channel
// `sky.rs`'s `update_sky` drives for the horizon fade-in/out. The vertex
// stage is Bevy's standard mesh vertex shader.

#import bevy_pbr::forward_io::VertexOutput

struct CelestialParams {
    tint: vec4<f32>,
}

@group(2) @binding(0) var celestial_texture: texture_2d<f32>;
@group(2) @binding(1) var celestial_sampler: sampler;
@group(2) @binding(2) var<uniform> params: CelestialParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(celestial_texture, celestial_sampler, in.uv);
    return vec4<f32>(color.rgb * params.tint.rgb, color.a * params.tint.a);
}
