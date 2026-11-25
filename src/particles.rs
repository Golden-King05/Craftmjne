//! Particles. The first is the **damage number**: when something is hit, its
//! damage pops up as red digits that shoot up, arc over, and drift back down
//! before fading out completely about halfway down the target's legs.
//!
//! A particle is an ordinary entity with a mesh (a strip of textured quads,
//! one per character) and its own unlit, alpha-blended material, turned to
//! face the camera every frame. The digits come from a tiny 3x5 pixel font
//! generated here, so there's no asset to load.
//!
//! The flight is a plain ballistic arc whose launch speed and gravity are
//! *solved* from the shape asked for (where it starts, how far it rises, and
//! where it must be when it's gone), not hand-tuned, so changing a number
//! changes the arc predictably. See [`Flight`].

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::player::Player;
use crate::state::AppState;

/// Where a damage number starts, in blocks above the target's feet (about
/// the dummy's head).
pub const START_HEIGHT: f32 = 1.7;
/// How far it rises above its start before turning over.
pub const PEAK_RISE: f32 = 0.6;
/// Where it is, above the feet, the moment it has fully faded: about halfway
/// down a 0.75-block pair of legs.
pub const END_HEIGHT: f32 = 0.375;
pub const LIFETIME: f32 = 1.2;
/// It's fully opaque until this fraction of its life, then fades to nothing
/// by the end.
pub const FADE_START: f32 = 0.45;
/// Sideways drift, blocks per second at most.
const DRIFT: f32 = 0.35;
/// Size of one font pixel in blocks (a glyph is 3x5 of them).
const PIXEL: f32 = 0.075;

/// The launch speed and gravity of a damage number's arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    pub speed: f32,
    pub gravity: f32,
}

impl Flight {
    /// Solves the arc that starts at `start`, rises `rise` above it, and is at
    /// `end` after `life` seconds. (With `v = sqrt(2 g rise)`, the height at
    /// `life` is a quadratic in `sqrt(g)`.)
    pub fn solve(start: f32, rise: f32, end: f32, life: f32) -> Self {
        let r = (2.0 * rise).sqrt();
        let root_g = (life * r + (2.0 * rise * life * life + 2.0 * life * life * (start - end)).sqrt()) / (life * life);
        let gravity = root_g * root_g;
        Self { speed: (2.0 * gravity * rise).sqrt(), gravity }
    }

    /// The damage number's own arc.
    pub fn damage_number() -> Self {
        Self::solve(START_HEIGHT, PEAK_RISE, END_HEIGHT, LIFETIME)
    }

    /// Height above the feet `t` seconds after launch.
    pub fn height(&self, t: f32) -> f32 {
        START_HEIGHT + self.speed * t - 0.5 * self.gravity * t * t
    }
}

/// Opacity `t` seconds into a damage number's life.
pub fn alpha(t: f32) -> f32 {
    let fade_start = LIFETIME * FADE_START;
    (1.0 - (t - fade_start) / (LIFETIME - fade_start)).clamp(0.0, 1.0)
}

/// How a damage figure reads: whole numbers without a decimal, anything else
/// to one place (`3`, `2.9`).
pub fn format_damage(amount: f32) -> String {
    let rounded = (amount * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i32)
    } else {
        format!("{rounded:.1}")
    }
}

// --- the font ---------------------------------------------------------------

const GLYPH_W: usize = 3;
const GLYPH_H: usize = 5;
/// Glyphs in the font image, left to right, each `GLYPH_W + 1` pixels apart.
const GLYPHS: &str = "0123456789.";

const FONT: [[&str; GLYPH_H]; 11] = [
    ["111", "101", "101", "101", "111"],
    ["010", "110", "010", "010", "111"],
    ["111", "001", "111", "100", "111"],
    ["111", "001", "111", "001", "111"],
    ["101", "101", "111", "001", "001"],
    ["111", "100", "111", "001", "111"],
    ["111", "100", "111", "101", "111"],
    ["111", "001", "001", "001", "001"],
    ["111", "101", "111", "101", "111"],
    ["111", "101", "111", "001", "111"],
    ["000", "000", "000", "000", "010"],
];

const FONT_W: usize = GLYPHS.len() * (GLYPH_W + 1);

/// The font as RGBA8 pixels: white where a glyph is, transparent elsewhere.
pub fn font_pixels() -> Vec<u8> {
    let mut pixels = vec![0u8; FONT_W * GLYPH_H * 4];
    for (g, glyph) in FONT.iter().enumerate() {
        for (y, row) in glyph.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if c == '1' {
                    let i = (y * FONT_W + g * (GLYPH_W + 1) + x) * 4;
                    pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }
    pixels
}

fn font_image() -> Image {
    let mut image = Image::new(
        Extent3d { width: FONT_W as u32, height: GLYPH_H as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        font_pixels(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Quads for `text`, centred on the origin in the XY plane facing +Z.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextMesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// One quad per character of `text` (characters the font lacks are skipped).
pub fn text_mesh(text: &str, pixel: f32) -> TextMesh {
    let glyphs: Vec<usize> = text.chars().filter_map(|c| GLYPHS.find(c)).collect();
    // A glyph is `GLYPH_W` wide with one pixel between, except that a
    // trailing gap isn't part of the text.
    let width = (glyphs.len() * (GLYPH_W + 1)).saturating_sub(1) as f32 * pixel;
    let mut mesh = TextMesh::default();
    for (i, g) in glyphs.iter().enumerate() {
        let x0 = i as f32 * (GLYPH_W + 1) as f32 * pixel - width / 2.0;
        let (x1, y0, y1) = (x0 + GLYPH_W as f32 * pixel, -(GLYPH_H as f32) * pixel / 2.0, GLYPH_H as f32 * pixel / 2.0);
        let u0 = (g * (GLYPH_W + 1)) as f32 / FONT_W as f32;
        let u1 = (g * (GLYPH_W + 1) + GLYPH_W) as f32 / FONT_W as f32;
        let base = mesh.positions.len() as u32;
        mesh.positions.extend([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]]);
        mesh.uvs.extend([[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]]);
        mesh.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    mesh
}

// --- the particle -------------------------------------------------------------

/// Something asked for a damage number: `origin` is the target's feet.
#[derive(Event, Clone, Copy, Debug)]
pub struct DamageNumberEvent {
    pub origin: Vec3,
    pub amount: f32,
}

#[derive(Component)]
pub struct DamageNumber {
    origin: Vec3,
    /// Horizontal drift in blocks per second.
    drift: Vec2,
    age: f32,
}

#[derive(Resource, Default)]
struct ParticleAssets {
    font: Option<Handle<Image>>,
}

/// A small deterministic spread from a counter, so numbers from rapid hits
/// don't sit exactly on top of each other.
fn drift_for(n: u32) -> Vec2 {
    let h = n.wrapping_mul(2_654_435_761);
    let unit = |bits: u32| ((bits & 0xFFFF) as f32 / 65535.0) * 2.0 - 1.0;
    Vec2::new(unit(h), unit(h >> 16)) * DRIFT
}

fn spawn_damage_numbers(
    mut commands: Commands,
    mut events: EventReader<DamageNumberEvent>,
    mut assets: ResMut<ParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut counter: Local<u32>,
) {
    for ev in events.read() {
        let font = assets.font.get_or_insert_with(|| images.add(font_image())).clone();
        let text = text_mesh(&format_damage(ev.amount), PIXEL);
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, text.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; text.positions.len()]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, text.uvs);
        mesh.insert_indices(Indices::U32(text.indices));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.12, 0.1, 1.0),
            base_color_texture: Some(font),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        });
        *counter = counter.wrapping_add(1);
        commands.spawn((
            DamageNumber { origin: ev.origin, drift: drift_for(*counter), age: 0.0 },
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(ev.origin + Vec3::Y * START_HEIGHT),
        ));
    }
}

fn update_damage_numbers(
    mut commands: Commands,
    time: Res<Time>,
    players: Query<&Transform, (With<Player>, Without<DamageNumber>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut numbers: Query<(Entity, &mut DamageNumber, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let flight = Flight::damage_number();
    let facing = players.single().map(|t| t.rotation).unwrap_or(Quat::IDENTITY);
    for (entity, mut number, mut transform, material) in &mut numbers {
        number.age += time.delta_secs();
        if number.age >= LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let t = number.age;
        transform.translation =
            number.origin + Vec3::new(number.drift.x * t, flight.height(t), number.drift.y * t);
        // Always face the camera, whichever way it looks.
        transform.rotation = facing;
        if let Some(mat) = materials.get_mut(&material.0) {
            mat.base_color.set_alpha(alpha(t));
        }
    }
}

fn despawn_on_exit(mut commands: Commands, numbers: Query<Entity, With<DamageNumber>>) {
    for e in &numbers {
        commands.entity(e).despawn();
    }
}

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ParticleAssets>()
            .add_event::<DamageNumberEvent>()
            .add_systems(OnExit(AppState::InGame), despawn_on_exit)
            .add_systems(
                Update,
                (spawn_damage_numbers, update_damage_numbers).chain().run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arc_starts_at_the_head_peaks_where_asked_and_is_halfway_down_the_legs_when_gone() {
        let f = Flight::damage_number();
        assert!((f.height(0.0) - START_HEIGHT).abs() < 1e-5);
        assert!((f.height(LIFETIME) - END_HEIGHT).abs() < 1e-4, "ends at {}", f.height(LIFETIME));
        // The peak is at t = v / g, PEAK_RISE above the start.
        let top = f.speed / f.gravity;
        assert!((f.height(top) - (START_HEIGHT + PEAK_RISE)).abs() < 1e-4);
        assert!(top > 0.0 && top < LIFETIME, "it rises first, then comes down");
        // Rising, then falling, never touching the floor.
        assert!(f.height(top * 0.5) > START_HEIGHT);
        let lowest = (0..=120).map(|i| f.height(LIFETIME * i as f32 / 120.0)).fold(f32::MAX, f32::min);
        assert!(lowest >= END_HEIGHT - 1e-4 && lowest > 0.0, "{lowest}");
    }

    #[test]
    fn it_is_opaque_at_first_and_completely_gone_by_the_end() {
        assert_eq!(alpha(0.0), 1.0);
        assert_eq!(alpha(LIFETIME * FADE_START), 1.0);
        assert!(alpha(LIFETIME * 0.7) < 1.0 && alpha(LIFETIME * 0.7) > 0.0);
        assert_eq!(alpha(LIFETIME), 0.0);
        let steps: Vec<f32> = (0..=20).map(|i| alpha(LIFETIME * i as f32 / 20.0)).collect();
        assert!(steps.windows(2).all(|w| w[1] <= w[0]), "it only ever fades");
    }

    #[test]
    fn damage_reads_as_a_whole_number_or_one_decimal() {
        assert_eq!(format_damage(3.0), "3");
        assert_eq!(format_damage(2.94), "2.9");
        assert_eq!(format_damage(3.449), "3.4");
        assert_eq!(format_damage(1.0), "1");
        assert_eq!(format_damage(10.04), "10");
    }

    #[test]
    fn the_font_has_every_digit_and_a_point_and_nothing_else() {
        for c in "0123456789.".chars() {
            assert!(GLYPHS.contains(c));
        }
        let px = font_pixels();
        assert_eq!(px.len(), FONT_W * GLYPH_H * 4);
        // Every glyph paints something, and the gap between glyphs stays empty.
        for g in 0..GLYPHS.len() {
            let lit = (0..GLYPH_H)
                .flat_map(|y| (0..GLYPH_W).map(move |x| (x, y)))
                .filter(|&(x, y)| px[(y * FONT_W + g * (GLYPH_W + 1) + x) * 4 + 3] == 255)
                .count();
            assert!(lit >= 1, "glyph {g} is blank");
            for y in 0..GLYPH_H {
                let gap = (y * FONT_W + g * (GLYPH_W + 1) + GLYPH_W) * 4 + 3;
                assert_eq!(px[gap], 0, "gap after glyph {g}");
            }
        }
    }

    #[test]
    fn text_is_centred_one_quad_per_character_with_uvs_on_the_font() {
        let m = text_mesh("12.5", PIXEL);
        assert_eq!(m.positions.len(), 4 * 4);
        assert_eq!(m.indices.len(), 6 * 4);
        let (lo, hi) = m.positions.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        assert!((lo + hi).abs() < 1e-5, "centred: {lo}..{hi}");
        assert!(m.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])));
        assert!(text_mesh("abc", PIXEL).positions.is_empty(), "letters aren't in the font");
        // Each triangle faces the camera (+Z), so it isn't culled or backwards.
        for tri in m.indices.chunks(3) {
            let p: Vec<[f32; 3]> = tri.iter().map(|&i| m.positions[i as usize]).collect();
            let cross_z = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
            assert!(cross_z > 0.0);
        }
    }

    #[test]
    fn drift_is_small_and_varies_between_numbers() {
        let a = drift_for(1);
        let b = drift_for(2);
        assert_ne!(a, b);
        for d in (1..200).map(drift_for) {
            assert!(d.x.abs() <= DRIFT && d.y.abs() <= DRIFT);
        }
    }

    fn app() -> App {
        use bevy::time::TimeUpdateStrategy;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::state::app::StatesPlugin))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_state::<AppState>()
            .add_plugins(ParticlesPlugin)
            // Each update is exactly a tenth of a second.
            .insert_resource(TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(100)));
        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
        app.update();
        app
    }

    #[test]
    fn a_hit_spawns_a_number_that_follows_the_arc_fades_and_is_removed() {
        let mut app = app();
        let feet = Vec3::new(10.5, 20.0, 10.5);
        app.world_mut().send_event(DamageNumberEvent { origin: feet, amount: 2.9 });
        let mut y_seen = Vec::new();
        let mut alphas = Vec::new();
        for _ in 0..20 {
            app.update();
            let found: Vec<(f32, Handle<StandardMaterial>)> = app
                .world_mut()
                .query::<(&DamageNumber, &Transform, &MeshMaterial3d<StandardMaterial>)>()
                .iter(app.world())
                .map(|(_, t, m)| (t.translation.y - feet.y, m.0.clone()))
                .collect();
            let Some((y, handle)) = found.first() else { break };
            y_seen.push(*y);
            alphas.push(app.world().resource::<Assets<StandardMaterial>>().get(handle).unwrap().base_color.alpha());
        }
        assert!(y_seen.len() >= 8, "it lived a while: {} frames", y_seen.len());
        let peak = y_seen.iter().cloned().fold(f32::MIN, f32::max);
        assert!(peak > START_HEIGHT + 0.3, "it rose first: peak {peak}");
        assert!(y_seen.last().unwrap() < &START_HEIGHT, "then came back down: {y_seen:?}");
        assert!(y_seen.iter().all(|&y| y > END_HEIGHT - 0.05), "never below the end height");
        assert!(alphas.windows(2).all(|w| w[1] <= w[0] + 1e-6), "only fades: {alphas:?}");
        assert!(*alphas.last().unwrap() < 0.2, "almost gone by its last frame: {alphas:?}");
        // After its lifetime it's removed.
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world_mut().query::<&DamageNumber>().iter(app.world()).count(), 0);
    }
}
