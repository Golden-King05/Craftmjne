//! Entities: things that stand in the world and aren't blocks. Right now
//! that's the training dummy - the player model with a blank skin - but the
//! definition format is data (`entities/*.json`), the same shape as blocks
//! and items, so a mod's entity is a file plus, later, its own model.
//!
//! ```json
//! {
//!   "id": "training_dummy",
//!   "name": "Training Dummy",
//!   "model": "player",
//!   "skin": "blank",
//!   "height": 2.0,
//!   "collisionbox_binding": true,
//!   "collision_size": { "width": 1.0, "depth": 1.0, "height": 2.0 }
//! }
//! ```
//!
//! - `model` / `skin`: which model to draw (only `player` exists) and its
//!   skin (only `blank`). `height` is how tall that model stands, in blocks.
//! - **`collisionbox_binding`** decides what you bump into:
//!   - `true`: the entity's own collision box, `collision_size` centred on
//!     its feet. Walk right up to the model's edge.
//!   - `false`: it behaves like a block - the box is rounded out to whole
//!     block cells, so it occupies every cell it touches.
//!
//! Entities are summoned with `/summon <id>`, removed with `/clearentities`,
//! and saved with the world.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

use crate::blocks::find_data_dir;
use crate::player_model::{blank_skin, ModelData, SKIN_SIZE};
use crate::render::{ChunkMaterial, ChunkMaterials};
use crate::state::AppState;
use crate::world::ChunkMap;

/// An axis-aligned box in world space.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// Whether the two boxes share volume (touching faces don't count).
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }

    /// Whether the block cell `cell` shares volume with this box.
    pub fn overlaps_cell(&self, cell: IVec3) -> bool {
        let min = cell.as_vec3();
        self.overlaps(&Aabb { min, max: min + Vec3::ONE })
    }
}

#[derive(Clone, Copy, Deserialize, Debug, PartialEq)]
pub struct CollisionSize {
    pub width: f32,
    pub depth: f32,
    pub height: f32,
}

#[derive(Clone, Deserialize, Debug)]
pub struct EntityDef {
    pub id: String,
    pub name: String,
    #[serde(default = "player_model")]
    pub model: String,
    #[serde(default = "blank_skin_name")]
    pub skin: String,
    /// How tall the model stands, in blocks.
    pub height: f32,
    /// See the module docs. Defaults to `true`: bound to the model.
    #[serde(default = "yes")]
    pub collisionbox_binding: bool,
    pub collision_size: CollisionSize,
}

fn player_model() -> String {
    "player".into()
}
fn blank_skin_name() -> String {
    "blank".into()
}
fn yes() -> bool {
    true
}

impl EntityDef {
    /// The name to show: the active language's `entity.<id>`, or the English one from the file.
    pub fn display(&self) -> String {
        crate::lang::name_of("entity", &self.id, &self.name)
    }

    /// The box an entity of this kind standing at `feet` collides with.
    pub fn collision_box(&self, feet: Vec3) -> Aabb {
        let s = self.collision_size;
        let bound = Aabb {
            min: Vec3::new(feet.x - s.width / 2.0, feet.y, feet.z - s.depth / 2.0),
            max: Vec3::new(feet.x + s.width / 2.0, feet.y + s.height, feet.z + s.depth / 2.0),
        };
        if self.collisionbox_binding {
            bound
        } else {
            // Like a block: every cell it touches, whole.
            Aabb { min: bound.min.floor(), max: bound.max.ceil() }
        }
    }
}

#[derive(Resource, Default)]
pub struct EntityRegistry {
    pub defs: Vec<EntityDef>,
}

impl EntityRegistry {
    pub fn with_defaults() -> Self {
        Self::load_from_dir(&find_data_dir("entities"))
    }

    /// Loads every `*.json` in `dir`, sorted by file name. Panics, naming the
    /// file, on one that doesn't parse or is nonsensical - same stance as
    /// blocks and items.
    pub fn load_from_dir(dir: &Path) -> Self {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
        paths.sort();
        let mut reg = Self::default();
        for path in paths {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
            let def: EntityDef = serde_json::from_str(&text).unwrap_or_else(|e| panic!("entity {path:?}: {e}"));
            assert!(def.model == "player", "entity {path:?}: the only model is \"player\", not {:?}", def.model);
            assert!(def.skin == "blank", "entity {path:?}: the only skin is \"blank\", not {:?}", def.skin);
            let s = def.collision_size;
            assert!(
                def.height > 0.0 && s.width > 0.0 && s.depth > 0.0 && s.height > 0.0,
                "entity {path:?}: sizes must be positive"
            );
            assert!(reg.by_name(&def.id).is_none(), "entity {path:?}: duplicate id {:?}", def.id);
            reg.defs.push(def);
        }
        reg
    }

    pub fn by_name(&self, id: &str) -> Option<usize> {
        self.defs.iter().position(|d| d.id == id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.defs.iter().map(|d| d.id.clone()).collect()
    }
}

/// One entity standing in the world. `pos` is its feet; `yaw` turns it the
/// way the player's yaw turns the player.
#[derive(Component, Clone, Debug)]
pub struct WorldEntity {
    pub kind: usize,
    pub pos: Vec3,
    pub yaw: f32,
}

/// Something asked for an entity to appear (the `/summon` command, loading a
/// save).
#[derive(Event, Clone, Debug)]
pub struct SpawnEntity {
    pub kind: usize,
    pub pos: Vec3,
    pub yaw: f32,
    /// `Some` for the model of a block (see [`BlockModel`]).
    pub block_cell: Option<IVec3>,
}

impl SpawnEntity {
    /// A free-standing entity.
    pub fn at(kind: usize, pos: Vec3, yaw: f32) -> Self {
        Self { kind, pos, yaw, block_cell: None }
    }
}

/// The visible model of a block whose shape is `Shape::Model` (the training
/// dummy), standing in the block's cell. It has no collision of its own (the
/// blocks provide that), isn't saved (the blocks are) and is created and
/// removed by `block_models.rs` as the blocks come and go.
#[derive(Component, Clone, Copy, Debug)]
pub struct BlockModel {
    pub cell: IVec3,
}

/// Remove every entity (`/clearentities`).
#[derive(Event, Clone, Copy, Debug)]
pub struct ClearEntities;

/// Every entity's collision box this frame, for the player (and block
/// placement) to respect. Rebuilt each frame by `refresh_colliders`.
#[derive(Resource, Default)]
pub struct EntityColliders(pub Vec<Aabb>);

/// Shared GPU assets, built the first time something spawns.
#[derive(Resource, Default)]
struct EntityAssets {
    meshes: HashMap<usize, Handle<Mesh>>,
    skin: Option<Handle<Image>>,
}

fn model_mesh(height: f32) -> Mesh {
    let data = ModelData::build(height);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, data.uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, data.colors);
    mesh.insert_indices(Indices::U32(data.indices));
    mesh
}

fn skin_image() -> Image {
    let mut image = Image::new(
        Extent3d { width: SKIN_SIZE, height: SKIN_SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        blank_skin(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Crisp pixels, like every other texture here.
    image.sampler = ImageSampler::nearest();
    image
}

fn spawn_entities(
    mut commands: Commands,
    mut events: EventReader<SpawnEntity>,
    registry: Res<EntityRegistry>,
    mut assets: ResMut<EntityAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for ev in events.read() {
        let Some(def) = registry.defs.get(ev.kind) else { continue };
        let mesh = assets.meshes.entry(ev.kind).or_insert_with(|| meshes.add(model_mesh(def.height))).clone();
        let skin = assets.skin.get_or_insert_with(|| images.add(skin_image())).clone();
        // One material each: its colour carries the light at its position.
        let material = materials.add(StandardMaterial {
            base_color_texture: Some(skin),
            unlit: true,
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        });
        let mut entity = commands.spawn((
            WorldEntity { kind: ev.kind, pos: ev.pos, yaw: ev.yaw },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(ev.pos).with_rotation(Quat::from_rotation_y(ev.yaw)),
        ));
        if let Some(cell) = ev.block_cell {
            entity.insert(BlockModel { cell });
        }
    }
}

/// Keeps each entity's transform and light colour current.
#[allow(clippy::type_complexity)]
fn sync_entity_visuals(
    registry: Res<EntityRegistry>,
    map: Res<ChunkMap>,
    chunk_materials: Option<Res<ChunkMaterials>>,
    chunk_assets: Res<Assets<ChunkMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut entities: Query<(&WorldEntity, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    // The sky's current colour, as `sky.rs` last wrote it for the world.
    let sky = chunk_materials
        .and_then(|m| chunk_assets.get(&m.solid).map(|a| a.params.sky_light))
        .map(|c| Vec3::new(c.red, c.green, c.blue))
        .unwrap_or(Vec3::ONE);
    for (entity, mut transform, material) in &mut entities {
        transform.translation = entity.pos;
        transform.rotation = Quat::from_rotation_y(entity.yaw);
        let Some(def) = registry.defs.get(entity.kind) else { continue };
        let cell = (entity.pos + Vec3::Y * def.height * 0.5).floor().as_ivec3();
        let light = map.get_light(cell);
        let channel = |i: usize| {
            let block = light.block[i] as f32 / 255.0;
            let sky_part = light.sky[i] as f32 / 255.0 * [sky.x, sky.y, sky.z][i];
            block.max(sky_part)
        };
        let color = Color::linear_rgb(channel(0), channel(1), channel(2));
        if let Some(mat) = materials.get_mut(&material.0) {
            if mat.base_color != color {
                mat.base_color = color;
            }
        }
    }
}

fn refresh_colliders(
    registry: Res<EntityRegistry>,
    entities: Query<&WorldEntity, Without<BlockModel>>,
    mut colliders: ResMut<EntityColliders>,
) {
    colliders.0.clear();
    colliders.0.extend(
        entities.iter().filter_map(|e| registry.defs.get(e.kind).map(|def| def.collision_box(e.pos))),
    );
}

/// Removes every entity - `/clearentities`, and leaving a world.
pub fn despawn_all<F: bevy::ecs::query::QueryFilter>(commands: &mut Commands, entities: &Query<Entity, F>) {
    for e in entities.iter() {
        commands.entity(e).despawn();
    }
}

fn clear_on_request(
    mut commands: Commands,
    mut events: EventReader<ClearEntities>,
    entities: Query<Entity, (With<WorldEntity>, Without<BlockModel>)>,
) {
    // Block models belong to their blocks, not to `/clearentities`.
    if events.read().count() > 0 {
        despawn_all(&mut commands, &entities);
    }
}

fn despawn_on_exit(mut commands: Commands, entities: Query<Entity, With<WorldEntity>>) {
    despawn_all(&mut commands, &entities);
}

/// The entity systems (spawning, colliders, visuals), for others to order against.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntitySet;

pub struct EntitiesPlugin;

impl Plugin for EntitiesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(EntityRegistry::with_defaults())
            .init_resource::<EntityColliders>()
            .init_resource::<EntityAssets>()
            .add_event::<SpawnEntity>()
            .add_event::<ClearEntities>()
            .add_systems(OnExit(AppState::InGame), despawn_on_exit)
            .add_systems(
                Update,
                (clear_on_request, spawn_entities, refresh_colliders, sync_entity_visuals)
                    .chain()
                    .in_set(EntitySet)
                    .before(crate::player::PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> EntityDef {
        let reg = EntityRegistry::with_defaults();
        reg.defs[reg.by_name("training_dummy").expect("the dummy ships")].clone()
    }

    #[test]
    fn the_training_dummy_is_two_blocks_tall_and_a_block_across_and_bound_to_its_model() {
        let d = dummy();
        assert_eq!(d.height, 2.0);
        assert!(d.collisionbox_binding);
        assert_eq!(d.collision_size, CollisionSize { width: 1.0, depth: 1.0, height: 2.0 });
        let b = d.collision_box(Vec3::new(5.5, 10.0, 5.5));
        assert_eq!(b, Aabb { min: Vec3::new(5.0, 10.0, 5.0), max: Vec3::new(6.0, 12.0, 6.0) });
    }

    #[test]
    fn a_bound_box_follows_the_model_but_an_unbound_one_fills_whole_blocks() {
        let mut d = dummy();
        let feet = Vec3::new(5.3, 10.2, 5.3);
        let bound = d.collision_box(feet);
        assert!((bound.min.x - 4.8).abs() < 1e-5 && (bound.max.y - 12.2).abs() < 1e-5, "{bound:?}");
        d.collisionbox_binding = false;
        let blocky = d.collision_box(feet);
        assert_eq!(blocky, Aabb { min: Vec3::new(4.0, 10.0, 4.0), max: Vec3::new(6.0, 13.0, 6.0) });
        // The point of the flag: a sliver the bound box leaves free is
        // solid when it behaves like blocks.
        let sliver = Aabb { min: Vec3::new(4.0, 10.0, 4.0), max: Vec3::new(4.5, 10.5, 4.5) };
        assert!(blocky.overlaps(&sliver));
        assert!(!bound.overlaps(&sliver));
    }

    #[test]
    fn boxes_that_only_touch_do_not_overlap() {
        let a = Aabb { min: Vec3::ZERO, max: Vec3::ONE };
        let b = Aabb { min: Vec3::new(1.0, 0.0, 0.0), max: Vec3::new(2.0, 1.0, 1.0) };
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&Aabb { min: Vec3::splat(0.5), max: Vec3::splat(1.5) }));
    }

    #[test]
    fn the_binding_flag_defaults_to_bound_and_a_bad_file_is_refused() {
        let def: EntityDef = serde_json::from_str(
            r#"{"id":"x","name":"X","height":1.0,"collision_size":{"width":1,"depth":1,"height":1}}"#,
        )
        .unwrap();
        assert!(def.collisionbox_binding);
        assert_eq!((def.model.as_str(), def.skin.as_str()), ("player", "blank"));
        assert!(serde_json::from_str::<EntityDef>(r#"{"id":"x","name":"X"}"#).is_err());
    }

    /// A bare app with just enough of Bevy for the plugin's systems to be
    /// scheduled and run (no window, no renderer).
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::state::app::StatesPlugin))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<ChunkMaterial>()
            .init_state::<AppState>()
            .init_resource::<ChunkMap>()
            .add_plugins(EntitiesPlugin);
        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
        app.update();
        app
    }

    #[test]
    fn a_summoned_dummy_appears_collides_and_is_cleared() {
        let mut app = app();
        let kind = app.world().resource::<EntityRegistry>().by_name("training_dummy").unwrap();
        app.world_mut().send_event(SpawnEntity::at(kind, Vec3::new(5.5, 10.0, 5.5), 0.0));
        app.update();
        app.update();
        let count = app.world_mut().query::<&WorldEntity>().iter(app.world()).count();
        assert_eq!(count, 1);
        let colliders = &app.world().resource::<EntityColliders>().0;
        assert_eq!(colliders.len(), 1);
        assert_eq!(colliders[0].max - colliders[0].min, Vec3::new(1.0, 2.0, 1.0));
        // Its mesh is the full model.
        let meshes = app.world().resource::<Assets<Mesh>>();
        assert_eq!(meshes.len(), 1);

        app.world_mut().send_event(ClearEntities);
        app.update();
        app.update();
        assert_eq!(app.world_mut().query::<&WorldEntity>().iter(app.world()).count(), 0);
        assert!(app.world().resource::<EntityColliders>().0.is_empty());
    }
}
