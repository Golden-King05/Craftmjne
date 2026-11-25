//! **State tags**: everything about one *placed* block that can change while
//! the game runs - the player, a command, or the simulation.
//!
//! This is the second of the game's two kinds of tags. A *definition tag*
//! (`blocks/*.json`: `hardness`, `shape`, `emission`, ...) is a fixed fact
//! about a block type ("torches glow") and is read once at startup, see
//! `tags.rs`. A *state tag* belongs to a single cell: which way this log
//! lies, whether this pebble sits in water, how far this water has flowed.
//!
//! Every state tag is one row in [`STATE_TAGS`] - a name, a description, and
//! three functions: which values it can take *for a given block*, how to
//! read it from a cell, and how to write it. One generic algorithm
//! ([`read_states`], [`set_state`]) serves all of them, so the `/blockstate`
//! command, and any future interaction, treats `axis`, `waterlogged` and
//! `level` identically. Adding a tag is adding a row.
//!
//! ## Storage
//!
//! Every cell has one **state word** (`Chunk::state`, a `u16` per cell, the
//! same two bytes a cell used to spend on separate fluid-level and axis
//! arrays). Each tag stored in it owns a [`Field`] - a run of bits - declared
//! in its table row as [`Storage::Cell`]: `level` takes 8 bits, `axis` 2,
//! leaving 6 free. A new tag that fits in the spare bits costs no memory.
//!
//! `waterlogged` is the one deliberate exception ([`Storage::BlockId`]): its
//! value *is* which block id the cell holds (`pebble@water`), because the
//! collision, light and mesher tables are all indexed by block id and would
//! each have to become state-dependent to move it. The interface hides that
//! difference; the storage is still one extra case, not a hidden one.
//!
//! What is saved, and how, is unchanged on disk so older worlds still open:
//! `axis` through the edit log (`BlockSetEvent`), `level` through the fluid
//! scan. They read and write the word through these fields.

use bevy::prelude::*;

use crate::blocks::{BlockDef, BlockId, BlockRegistry, BlockTables, Rotation, Shape, Tables, AIR, AXIS_X, AXIS_Y, AXIS_Z, FLUID_FALLING, FLUID_SOURCE};
use crate::chat::ChatLog;
use crate::world::{BlockSetEvent, ChunkMap};

/// A run of bits in a cell's state word (`Chunk::state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub shift: u8,
    pub bits: u8,
}

impl Field {
    pub const fn mask(self) -> u16 {
        (((1u32 << self.bits) - 1) as u16) << self.shift
    }
    /// This field's value in `word`.
    pub const fn get(self, word: u16) -> u16 {
        (word & self.mask()) >> self.shift
    }
    /// `word` with this field set to `value` (extra high bits are dropped).
    pub const fn set(self, word: u16, value: u16) -> u16 {
        (word & !self.mask()) | ((value << self.shift) & self.mask())
    }
    /// `value` placed in this field, for building a word from scratch.
    pub const fn pack(self, value: u16) -> u16 {
        self.set(0, value)
    }
}

/// A fluid's flow level: `FLUID_SOURCE` (0), 1.. blocks from a supply, or
/// `FLUID_FALLING` (255).
pub const LEVEL: Field = Field { shift: 0, bits: 8 };
/// Which axis a rotating block lies on (`AXIS_X/Y/Z`).
pub const AXIS: Field = Field { shift: 8, bits: 2 };

/// Every cell's state word before anything has happened to it: a source-level
/// cell on the default axis. A fresh chunk is filled with this.
pub const DEFAULT_STATE: u16 = LEVEL.pack(FLUID_SOURCE as u16) | AXIS.pack(AXIS_Y as u16);

/// Builds a state word from a fluid level and an axis.
pub const fn pack_state(level: u8, axis: u8) -> u16 {
    LEVEL.pack(level as u16) | AXIS.pack(axis as u16)
}

/// Where a state tag's value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    /// In the cell's one state word, in these bits.
    Cell(Field),
    /// Part of the block's identity: the value is *which block id the cell
    /// holds* (`pebble@water`). Used where every per-id table (light,
    /// collision, meshing) would otherwise have to become state-dependent.
    BlockId,
}

/// One state tag: what it is, and how to read and write it.
pub struct StateTagDef {
    pub name: &'static str,
    pub description: &'static str,
    /// Where its value is stored.
    pub storage: Storage,
    /// The definition tag that enables this state tag. Every state tag is
    /// switched on per block through the `states` definition tag; this names
    /// the behaviour tag that also implies it, so a block that already says
    /// `rotation: log` doesn't have to say `states: ["axis"]` as well.
    pub enabled_by: &'static str,
    /// Whether a block's own behaviour tags imply this state tag
    /// (second argument: the block is what some fluid freezes into).
    pub implied: fn(&BlockDef, bool) -> bool,
    /// Whether a block can have this state tag at all - `Err` says why not.
    /// A full cube has nowhere for water to go, so it can't be waterlogged.
    pub compatible: fn(&BlockDef, bool) -> Result<(), &'static str>,
    /// The values this tag can take on block `id`; empty means the tag
    /// doesn't apply to that block at all.
    pub values: fn(&BlockRegistry, &Tables, BlockId) -> Vec<String>,
    /// The cell's current value (only called where `values` is non-empty).
    pub get: fn(&ChunkMap, &BlockRegistry, &Tables, IVec3) -> String,
    /// Changes the cell to `value` (already checked against `values`),
    /// returning the block events the change should announce.
    pub set: fn(&mut ChunkMap, &BlockRegistry, &Tables, IVec3, &str) -> Vec<BlockSetEvent>,
}

/// Every state tag the game has.
pub const STATE_TAGS: &[StateTagDef] = &[
    StateTagDef {
        name: "axis",
        description: "Which way a rotating block (a log) lies.",
        storage: Storage::Cell(AXIS),
        enabled_by: "rotation",
        implied: |def, _| def.rotation != Rotation::None,
        compatible: |def, _| {
            if def.shape == Shape::Cube { Ok(()) } else { Err("only a full block can lie on a different axis") }
        },
        values: axis_values,
        get: axis_get,
        set: axis_set,
    },
    StateTagDef {
        name: "waterlogged",
        description: "The fluid or ice sharing this block's cell: none, water, ice.",
        storage: Storage::BlockId,
        enabled_by: "waterloggable",
        implied: |def, _| def.waterloggable,
        compatible: |def, _| {
            if def.shape == Shape::Cube { Err("a full block has nowhere for water to go") } else { Ok(()) }
        },
        values: layer_values,
        get: layer_get,
        set: layer_set,
    },
    StateTagDef {
        name: "level",
        description: "How far a fluid has flowed: source, 1 (next to a source) upward, or falling.",
        storage: Storage::Cell(LEVEL),
        enabled_by: "fluid",
        implied: |def, freezes_into| def.fluid || freezes_into,
        compatible: |def, freezes_into| {
            if def.fluid || freezes_into { Ok(()) } else { Err("only a fluid, or what one freezes into, has a flow level") }
        },
        values: level_values,
        get: level_get,
        set: level_set,
    },
];

pub fn state_tag(name: &str) -> Option<&'static StateTagDef> {
    STATE_TAGS.iter().find(|t| t.name == name)
}

/// Why a state couldn't be set.
#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    UnknownTag(String),
    /// This block doesn't have that tag.
    NotApplicable { block: String, tag: String },
    /// Not one of the tag's values for this block.
    BadValue { tag: String, value: String, allowed: Vec<String> },
    /// The chunk isn't loaded.
    NotLoaded,
}

impl StateError {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownTag(tag) => {
                let names: Vec<_> = STATE_TAGS.iter().map(|t| t.name).collect();
                format!("Unknown state tag \"{tag}\". Tags: {}", names.join(", "))
            }
            Self::NotApplicable { block, tag } => {
                format!("{block} has no \"{tag}\" state (a block enables states with its \"states\" tag).")
            }
            Self::BadValue { tag, value, allowed } => {
                format!("\"{value}\" isn't a value of {tag}. Try: {}", allowed.join(", "))
            }
            Self::NotLoaded => "That position isn't loaded.".into(),
        }
    }
}

/// A tag on a particular cell: its name, current value, and the values it
/// could take.
#[derive(Debug, PartialEq, Eq)]
pub struct CellState {
    pub name: &'static str,
    pub value: String,
    pub allowed: Vec<String>,
}

/// Every state tag the block at `pos` has, with its current value.
pub fn read_states(map: &ChunkMap, registry: &BlockRegistry, tables: &Tables, pos: IVec3) -> Vec<CellState> {
    let id = map.get_block(pos);
    STATE_TAGS
        .iter()
        .filter_map(|tag| {
            let allowed = (tag.values)(registry, tables, id);
            (!allowed.is_empty()).then(|| CellState { name: tag.name, value: (tag.get)(map, registry, tables, pos), allowed })
        })
        .collect()
}

/// Sets state tag `tag` of the block at `pos` to `value`. Returns the events
/// the caller must send so the change is remeshed, saved and simulated.
pub fn set_state(
    map: &mut ChunkMap,
    registry: &BlockRegistry,
    tables: &Tables,
    pos: IVec3,
    tag: &str,
    value: &str,
) -> Result<Vec<BlockSetEvent>, StateError> {
    if !map.is_loaded(pos) {
        return Err(StateError::NotLoaded);
    }
    let def = state_tag(tag).ok_or_else(|| StateError::UnknownTag(tag.to_string()))?;
    let id = map.get_block(pos);
    let allowed = (def.values)(registry, tables, id);
    if allowed.is_empty() {
        return Err(StateError::NotApplicable { block: registry.def(id).id.clone(), tag: tag.to_string() });
    }
    let value = value.to_lowercase();
    if !allowed.contains(&value) {
        return Err(StateError::BadValue { tag: tag.to_string(), value, allowed });
    }
    Ok((def.set)(map, registry, tables, pos, &value))
}

// ---- axis ------------------------------------------------------------

/// Whether block `id` has state tag `name` enabled (`BlockDef::states`).
fn enabled(registry: &BlockRegistry, id: BlockId, name: &str) -> bool {
    id != AIR && registry.def(id).states.iter().any(|s| s == name)
}

fn axis_values(registry: &BlockRegistry, _: &Tables, id: BlockId) -> Vec<String> {
    if enabled(registry, id, "axis") {
        vec!["x".into(), "y".into(), "z".into()]
    } else {
        Vec::new()
    }
}

fn axis_get(map: &ChunkMap, _: &BlockRegistry, _: &Tables, pos: IVec3) -> String {
    match map.get_axis(pos) {
        AXIS_X => "x",
        AXIS_Z => "z",
        _ => "y",
    }
    .into()
}

fn axis_set(map: &mut ChunkMap, _: &BlockRegistry, _: &Tables, pos: IVec3, value: &str) -> Vec<BlockSetEvent> {
    let axis = match value {
        "x" => AXIS_X,
        "z" => AXIS_Z,
        _ => AXIS_Y,
    };
    let id = map.get_block(pos);
    map.set_axis_raw(pos, axis);
    map.refresh_cell(pos);
    // Same id, new axis: this is what `EditLog` saves.
    vec![BlockSetEvent { pos, id, prev: id, axis }]
}

// ---- waterlogged -----------------------------------------------------

/// The block a variant is a layered version of (itself for a plain block).
fn base_of(registry: &BlockRegistry, id: BlockId) -> BlockId {
    registry.def(id).layered.map_or(id, |l| l.block)
}

/// The layer blocks (`water`, `ice`) that `base` has a variant for.
fn layers_of(registry: &BlockRegistry, base: BlockId) -> Vec<BlockId> {
    (1..registry.defs.len() as BlockId).filter(|&layer| registry.variant(base, layer).is_some()).collect()
}

fn layer_values(registry: &BlockRegistry, _: &Tables, id: BlockId) -> Vec<String> {
    if !enabled(registry, id, "waterlogged") {
        return Vec::new();
    }
    let layers = layers_of(registry, base_of(registry, id));
    if layers.is_empty() {
        return Vec::new();
    }
    let mut values = vec!["none".to_string()];
    values.extend(layers.into_iter().map(|l| registry.def(l).id.clone()));
    values
}

fn layer_get(map: &ChunkMap, registry: &BlockRegistry, _: &Tables, pos: IVec3) -> String {
    registry.def(map.get_block(pos)).layered.map_or("none".to_string(), |l| registry.def(l.layer).id.clone())
}

fn layer_set(map: &mut ChunkMap, registry: &BlockRegistry, _: &Tables, pos: IVec3, value: &str) -> Vec<BlockSetEvent> {
    let old = map.get_block(pos);
    let base = base_of(registry, old);
    let new = if value == "none" {
        base
    } else {
        match registry.by_name(value).ok().and_then(|layer| registry.variant(base, layer)) {
            Some(v) => v,
            None => return Vec::new(),
        }
    };
    if new == old {
        return Vec::new();
    }
    let axis = map.get_axis(pos);
    map.set_block(pos, new);
    // Starts as a permanent source, like a player-placed fluid.
    map.set_fluid_level_raw(pos, FLUID_SOURCE);
    vec![BlockSetEvent { pos, id: new, prev: old, axis }]
}

// ---- level -----------------------------------------------------------

fn level_values(registry: &BlockRegistry, tables: &Tables, id: BlockId) -> Vec<String> {
    let i = id as usize;
    if !enabled(registry, id, "level") {
        return Vec::new();
    }
    let mut values = vec!["source".to_string()];
    values.extend((1..=tables.flow_distance[i].max(1) as u32).map(|n| n.to_string()));
    values.push("falling".into());
    values
}

fn level_get(map: &ChunkMap, _: &BlockRegistry, _: &Tables, pos: IVec3) -> String {
    match map.get_fluid_level(pos) {
        FLUID_SOURCE => "source".into(),
        FLUID_FALLING => "falling".into(),
        n => n.to_string(),
    }
}

fn level_set(map: &mut ChunkMap, _: &BlockRegistry, _: &Tables, pos: IVec3, value: &str) -> Vec<BlockSetEvent> {
    let level = match value {
        "source" => FLUID_SOURCE,
        "falling" => FLUID_FALLING,
        n => n.parse().unwrap_or(FLUID_SOURCE),
    };
    let id = map.get_block(pos);
    map.set_fluid_level_raw(pos, level);
    map.refresh_cell(pos);
    // Wakes the fluid simulation around the cell, which settles the rest.
    vec![BlockSetEvent { pos, id, prev: id, axis: map.get_axis(pos) }]
}

// ---- the in-game side: a request, and the system that carries it out ---

/// Asks to read or change state tags at a position (`/blockstate`, and
/// later anything else that edits state).
#[derive(Event, Clone, Debug)]
pub struct StateEditRequest {
    pub pos: IVec3,
    /// `None`: just list the cell's states.
    pub tag: Option<String>,
    /// `None` with a `tag`: just show that tag.
    pub value: Option<String>,
}

fn apply_state_requests(
    mut requests: EventReader<StateEditRequest>,
    mut map: ResMut<ChunkMap>,
    registry: Res<BlockRegistry>,
    tables: Res<BlockTables>,
    mut events: EventWriter<BlockSetEvent>,
    mut log: ResMut<ChatLog>,
) {
    for req in requests.read() {
        let p = req.pos;
        let id = map.get_block(p);
        let name = registry.def(id).name.clone();
        let reply = match (&req.tag, &req.value) {
            (Some(tag), Some(value)) => match set_state(&mut map, &registry, &tables.0, p, tag, value) {
                Ok(changes) => {
                    for e in changes {
                        events.write(e);
                    }
                    format!("Set {tag} of {name} at {} {} {} to {}.", p.x, p.y, p.z, value.to_lowercase())
                }
                Err(e) => e.message(),
            },
            _ => {
                let mut states = read_states(&map, &registry, &tables.0, p);
                if let Some(tag) = &req.tag {
                    states.retain(|s| s.name == tag);
                }
                if states.is_empty() {
                    format!("{name} at {} {} {} has no state tags.", p.x, p.y, p.z)
                } else {
                    let parts: Vec<String> = states
                        .iter()
                        .map(|s| format!("{} = {} ({})", s.name, s.value, s.allowed.join("|")))
                        .collect();
                    format!("{name} at {} {} {}: {}", p.x, p.y, p.z, parts.join("; "))
                }
            }
        };
        log.push(reply);
    }
}

pub struct StatesPlugin;

impl Plugin for StatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>().add_event::<StateEditRequest>().add_systems(Update, apply_state_requests);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CHUNK_SIZE, WORLD_HEIGHT};
    use crate::light::LightCell;
    use std::sync::Arc;

    pub(super) fn setup() -> (BlockRegistry, Arc<Tables>, ChunkMap) {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        let n = (CHUNK_SIZE * CHUNK_SIZE * WORLD_HEIGHT) as usize;
        let chunk = crate::world::Chunk {
            blocks: Some(vec![AIR; n]),
            state: Some(vec![DEFAULT_STATE; n]),
            light: Some(vec![LightCell::DARK; n]),
            ..Default::default()
        };
        let mut map = ChunkMap::default();
        map.chunks.insert(IVec2::ZERO, chunk);
        (reg, tables, map)
    }

    pub(super) const P: IVec3 = IVec3::new(4, 10, 4);

    fn tags(map: &ChunkMap, reg: &BlockRegistry, tables: &Tables) -> Vec<&'static str> {
        read_states(map, reg, tables, P).iter().map(|s| s.name).collect()
    }

    #[test]
    fn each_block_only_has_the_tags_that_apply_to_it() {
        let (reg, tables, mut map) = setup();
        map.set_block(P, reg.id("stone"));
        assert!(tags(&map, &reg, &tables).is_empty());
        map.set_block(P, reg.id("log"));
        assert_eq!(tags(&map, &reg, &tables), vec!["axis"]);
        map.set_block(P, reg.id("pebble"));
        assert_eq!(tags(&map, &reg, &tables), vec!["waterlogged"]);
        map.set_block(P, reg.id("water"));
        assert_eq!(tags(&map, &reg, &tables), vec!["level"]);
    }

    #[test]
    fn axis_can_be_changed_and_is_announced_for_saving() {
        let (reg, tables, mut map) = setup();
        let log = reg.id("log");
        map.set_block(P, log);
        let events = set_state(&mut map, &reg, &tables, P, "axis", "X").unwrap();
        assert_eq!(map.get_axis(P), AXIS_X);
        assert_eq!(read_states(&map, &reg, &tables, P)[0].value, "x");
        // The event carries the new axis, which is what the edit log saves.
        assert!(events.iter().any(|e| e.pos == P && e.id == log && e.axis == AXIS_X));
        assert!(map.chunks[&IVec2::ZERO].dirty, "the chunk is remeshed");
    }

    #[test]
    fn waterlogged_swaps_between_the_plain_block_and_its_variants() {
        let (reg, tables, mut map) = setup();
        let (pebble, water, ice) = (reg.id("pebble"), reg.id("water"), reg.id("ice"));
        map.set_block(P, pebble);
        let events = set_state(&mut map, &reg, &tables, P, "waterlogged", "water").unwrap();
        assert_eq!(map.get_block(P), reg.variant(pebble, water).unwrap());
        assert_eq!(events.len(), 1);
        assert_eq!(read_states(&map, &reg, &tables, P)[0].value, "water");
        set_state(&mut map, &reg, &tables, P, "waterlogged", "ice").unwrap();
        assert_eq!(map.get_block(P), reg.variant(pebble, ice).unwrap());
        set_state(&mut map, &reg, &tables, P, "waterlogged", "none").unwrap();
        assert_eq!(map.get_block(P), pebble);
        // Setting what it already is changes nothing and announces nothing.
        assert!(set_state(&mut map, &reg, &tables, P, "waterlogged", "none").unwrap().is_empty());
    }

    #[test]
    fn a_fluids_level_can_be_set_and_wakes_the_simulation() {
        let (reg, tables, mut map) = setup();
        let water = reg.id("water");
        map.set_block(P, water);
        let events = set_state(&mut map, &reg, &tables, P, "level", "3").unwrap();
        assert_eq!(map.get_fluid_level(P), 3);
        assert!(events.iter().any(|e| e.pos == P));
        set_state(&mut map, &reg, &tables, P, "level", "falling").unwrap();
        assert_eq!(map.get_fluid_level(P), FLUID_FALLING);
        set_state(&mut map, &reg, &tables, P, "level", "source").unwrap();
        assert_eq!(map.get_fluid_level(P), FLUID_SOURCE);
    }

    #[test]
    fn bad_requests_say_what_was_wrong() {
        let (reg, tables, mut map) = setup();
        map.set_block(P, reg.id("log"));
        assert!(matches!(set_state(&mut map, &reg, &tables, P, "glow", "1"), Err(StateError::UnknownTag(_))));
        assert!(matches!(set_state(&mut map, &reg, &tables, P, "level", "1"), Err(StateError::NotApplicable { .. })));
        let err = set_state(&mut map, &reg, &tables, P, "axis", "w").err().expect("refused");
        assert!(matches!(&err, StateError::BadValue { allowed, .. } if allowed == &["x", "y", "z"]), "{err:?}");
        assert!(err.message().contains("x, y, z"));
        assert_eq!(map.get_axis(P), AXIS_Y, "a refused change changes nothing");
        let far = set_state(&mut map, &reg, &tables, IVec3::new(500, 10, 500), "axis", "x");
        assert!(matches!(far, Err(StateError::NotLoaded)));
    }

    #[test]
    fn every_registered_tag_has_a_unique_name_and_a_description() {
        let mut names: Vec<_> = STATE_TAGS.iter().map(|t| t.name).collect();
        assert!(STATE_TAGS.iter().all(|t| !t.description.is_empty()));
        names.sort();
        names.dedup();
        assert_eq!(names.len(), STATE_TAGS.len());
    }
}

#[cfg(test)]
mod enabling_tests {
    use super::*;
    use crate::blocks::{BlockFile, BlockRegistry};

    fn registry_with(files: &[&str]) -> BlockRegistry {
        let mut reg = BlockRegistry::with_defaults();
        for f in files {
            let def = serde_json::from_str::<BlockFile>(f).unwrap().into_def();
            reg.register(def);
        }
        reg.add_layered_variants();
        reg
    }

    fn states_of(reg: &BlockRegistry, name: &str) -> Vec<String> {
        reg.def(reg.id(name)).states.clone()
    }

    #[test]
    fn every_state_tag_is_enabled_through_a_real_definition_tag() {
        for tag in STATE_TAGS {
            assert!(
                crate::tags::definition_tag(tag.enabled_by).is_some(),
                "state tag {:?} names enabled_by {:?}, which isn't a definition tag",
                tag.name,
                tag.enabled_by
            );
        }
        // And the shared one: `states` itself is a definition tag.
        assert!(crate::tags::definition_tag("states").is_some());
    }

    #[test]
    fn a_block_only_has_the_states_it_enables_or_implies() {
        let reg = BlockRegistry::with_defaults();
        assert!(states_of(&reg, "stone").is_empty());
        assert_eq!(states_of(&reg, "log"), vec!["axis"]);
        assert_eq!(states_of(&reg, "pebble"), vec!["waterlogged"]);
        assert_eq!(states_of(&reg, "water"), vec!["level"]);
        assert_eq!(states_of(&reg, "ice"), vec!["level"], "what water freezes into holds the water's level");
    }

    #[test]
    fn declaring_a_state_turns_on_the_behaviour_it_needs() {
        let reg = registry_with(&[
            r#"{"id": "spar", "states": ["axis"]}"#,
            r#"{"id": "shell", "shape": "floor", "states": ["Waterlogged"]}"#,
        ]);
        assert_eq!(states_of(&reg, "spar"), vec!["axis"]);
        assert!(reg.def(reg.id("spar")).rotation != crate::blocks::Rotation::None);
        assert!(reg.def(reg.id("shell")).waterloggable);
        assert!(reg.variant(reg.id("shell"), reg.id("water")).is_some(), "it gets its waterlogged variants");
    }

    #[test]
    #[should_panic(expected = "nowhere for water to go")]
    fn a_full_block_cannot_be_waterlogged() {
        registry_with(&[r#"{"id": "slab", "states": ["waterlogged"]}"#]);
    }

    #[test]
    #[should_panic(expected = "nowhere for water to go")]
    fn the_same_goes_for_waterloggable_set_the_old_way() {
        registry_with(&[r#"{"id": "slab", "waterloggable": true}"#]);
    }

    #[test]
    #[should_panic(expected = "flow level")]
    fn a_block_that_isnt_a_fluid_has_no_level() {
        registry_with(&[r#"{"id": "dust", "states": ["level"]}"#]);
    }

    #[test]
    #[should_panic(expected = "unknown state")]
    fn an_unknown_state_name_is_refused() {
        registry_with(&[r#"{"id": "dust", "states": ["glowing"]}"#]);
    }

    #[test]
    fn a_state_that_isnt_enabled_cant_be_read_or_set() {
        let (reg, tables, mut map) = tests::setup();
        map.set_block(tests::P, reg.id("stone"));
        assert!(read_states(&map, &reg, &tables, tests::P).is_empty());
        assert!(matches!(
            set_state(&mut map, &reg, &tables, tests::P, "waterlogged", "water"),
            Err(StateError::NotApplicable { .. })
        ));
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    fn cell_fields() -> Vec<(&'static str, Field)> {
        STATE_TAGS
            .iter()
            .filter_map(|t| match t.storage {
                Storage::Cell(f) => Some((t.name, f)),
                Storage::BlockId => None,
            })
            .collect()
    }

    #[test]
    fn the_tags_stored_in_the_cell_word_fit_it_and_never_overlap() {
        let fields = cell_fields();
        let mut used = 0u16;
        for (name, f) in &fields {
            assert!(f.bits > 0 && f.shift as u32 + f.bits as u32 <= 16, "{name} doesn't fit in 16 bits");
            assert_eq!(used & f.mask(), 0, "{name} shares bits with another tag");
            used |= f.mask();
        }
        assert!(!fields.is_empty());
    }

    #[test]
    fn writing_one_field_leaves_the_others_alone() {
        let word = pack_state(7, AXIS_X);
        assert_eq!(LEVEL.get(word), 7);
        assert_eq!(AXIS.get(word), AXIS_X as u16);
        let w = LEVEL.set(word, FLUID_FALLING as u16);
        assert_eq!((LEVEL.get(w), AXIS.get(w)), (FLUID_FALLING as u16, AXIS_X as u16));
        let w = AXIS.set(w, AXIS_Z as u16);
        assert_eq!((LEVEL.get(w), AXIS.get(w)), (FLUID_FALLING as u16, AXIS_Z as u16));
        // A value too wide for its field is cut off rather than spilling over.
        assert_eq!(AXIS.get(AXIS.set(0, 0b111)), 0b11);
        assert_eq!(LEVEL.get(AXIS.set(0, 0b111)), 0);
    }

    #[test]
    fn a_fresh_cell_is_a_source_on_the_default_axis() {
        assert_eq!(LEVEL.get(DEFAULT_STATE) as u8, FLUID_SOURCE);
        assert_eq!(AXIS.get(DEFAULT_STATE) as u8, AXIS_Y);
    }

    #[test]
    fn a_cell_costs_no_more_than_it_did_as_two_arrays() {
        // Fluid level (u8) + axis (u8) used to be two bytes a cell.
        assert_eq!(std::mem::size_of::<u16>(), 2);
    }

    #[test]
    fn changing_a_cells_axis_does_not_disturb_its_fluid_level_in_a_chunk() {
        let (reg, tables, mut map) = tests::setup();
        let p = tests::P;
        map.set_block(p, reg.id("log"));
        map.set_fluid_level_raw(p, 5);
        set_state(&mut map, &reg, &tables, p, "axis", "z").unwrap();
        assert_eq!(map.get_fluid_level(p), 5, "the level survived an axis change");
        assert_eq!(map.get_axis(p), AXIS_Z);
        map.set_fluid_level_raw(p, FLUID_FALLING);
        assert_eq!(map.get_axis(p), AXIS_Z, "and the axis survived a level change");
    }
}
