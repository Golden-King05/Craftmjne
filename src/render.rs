//! Rendering: the chunk material/shader, atlas GPU image, and camera fog.
//!
//! Rendering strategy (for speed):
//!  - All lighting is pre-baked into vertex colors by the mesher, so the chunk
//!    shader is fully unlit — no lights, no normals, no shadow passes.
//!  - One shared material for all solid geometry (shader-side alpha discard
//!    handles leaf/glass cutouts) and one for water: two pipeline states.
//!  - Distance fog toward the sky color is computed in the same fragment
//!    shader, hiding the streaming edge of the world.

use bevy::asset::embedded_asset;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderRef, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};

use crate::atlas::ATLAS_PX;
use crate::config::{WorldSettings, CHUNK_SIZE, SKY_COLOR};
use crate::mesher::MeshBucket;
use crate::world::Atlas;

#[derive(Clone, ShaderType)]
#[allow(dead_code)] // the ShaderType derive generates per-field check fns
pub struct ChunkMaterialParams {
    pub fog_color: LinearRgba,
    pub fog_start: f32,
    pub fog_end: f32,
    /// Multiplied into texture alpha (water translucency).
    pub base_alpha: f32,
    /// Fragments below this alpha are discarded (leaf/glass cutouts).
    pub alpha_cutoff: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct ChunkMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub atlas: Handle<Image>,
    #[uniform(2)]
    pub params: ChunkMaterialParams,
    pub alpha_mode: AlphaMode,
}

impl Material for ChunkMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://craftmjne/chunk.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Water surfaces must be visible from below; solid geometry has no
        // reachable backfaces anyway (interior faces are culled by the mesher).
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Shared handles used by every chunk entity.
#[derive(Resource)]
pub struct ChunkMaterials {
    pub solid: Handle<ChunkMaterial>,
    pub water: Handle<ChunkMaterial>,
}

/// The atlas as a GPU image (nearest-neighbour sampled, sRGB).
#[derive(Resource)]
pub struct AtlasImage(pub Handle<Image>);

pub fn bucket_to_mesh(bucket: MeshBucket) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, bucket.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, bucket.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, bucket.colors);
    mesh.insert_indices(Indices::U32(bucket.indices));
    mesh
}

/// Startup (after `world::compile_content`): upload the atlas and create the
/// two chunk materials.
fn setup_render(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    atlas: Res<Atlas>,
    settings: Res<WorldSettings>,
) {
    let mut image = Image::new(
        Extent3d {
            width: ATLAS_PX as u32,
            height: ATLAS_PX as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        atlas.0.pixels.clone(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = bevy::image::ImageSampler::nearest();
    let atlas_handle = images.add(image);

    let view_dist = (settings.render_distance * CHUNK_SIZE) as f32;
    let fog_color = LinearRgba::from(SKY_COLOR);
    let fog = |base_alpha: f32, alpha_cutoff: f32| ChunkMaterialParams {
        fog_color,
        fog_start: view_dist * 0.55,
        fog_end: view_dist * 0.95,
        base_alpha,
        alpha_cutoff,
    };

    let solid = materials.add(ChunkMaterial {
        atlas: atlas_handle.clone(),
        params: fog(1.0, 0.5),
        alpha_mode: AlphaMode::Opaque,
    });
    let water = materials.add(ChunkMaterial {
        atlas: atlas_handle.clone(),
        params: fog(0.72, 0.0),
        alpha_mode: AlphaMode::Blend,
    });

    commands.insert_resource(AtlasImage(atlas_handle));
    commands.insert_resource(ChunkMaterials { solid, water });
}

pub struct RenderSetupPlugin;

impl Plugin for RenderSetupPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "chunk.wgsl");
        app.add_plugins(MaterialPlugin::<ChunkMaterial>::default())
            .insert_resource(ClearColor(SKY_COLOR))
            .add_systems(Startup, setup_render.after(crate::world::compile_content));
    }
}
