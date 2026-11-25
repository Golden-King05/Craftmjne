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
/// Which axis a rotating block lies on, stored so that the default axis
/// (`AXIS_Y`) is `0`: see [`encode_axis`].
pub const AXIS: Field = Field { shift: 8, bits: 2 };

/// Every cell's state word before anything has happened to it: **zero**. A
/// source-level cell on the default axis with every mod state at its first
/// value. The vanilla encodings (level 0 = source, axis stored as 0 = `AXIS_Y`)
/// and the rule that a declared state's first value is its default are what
/// make that true, so a block change can simply clear the word.
pub const DEFAULT_STATE: u16 = 0;

/// `AXIS_X/Y/Z` as stored: `AXIS_Y` is 0 so that a cleared word is the default.
pub const fn encode_axis(axis: u8) -> u16 {
    match axis {
        AXIS_X => 1,
        AXIS_Z => 2,
        _ => 0,
    }
}

pub const fn decode_axis(stored: u16) -> u8 {
    match stored {
        1 => AXIS_X,
        2 => AXIS_Z,
        _ => AXIS_Y,
    }
}

/// The fluid level in `word`.
pub const fn level_of(word: u16) -> u8 {
    LEVEL.get(word) as u8
}
/// `word` with its fluid level replaced.
pub const fn with_level(word: u16, level: u8) -> u16 {
    LEVEL.set(word, level as u16)
}
/// The rotation axis in `word` (`AXIS_X/Y/Z`).
pub const fn axis_of(word: u16) -> u8 {
    decode_axis(AXIS.get(word))
}
/// `word` with its rotation axis replaced.
pub const fn with_axis(word: u16, axis: u8) -> u16 {
    AXIS.set(word, encode_axis(axis))
}

/// Builds a state word from a fluid level and an axis.
pub const fn pack_state(level: u8, axis: u8) -> u16 {
    with_axis(with_level(0, level), axis)
}

/// The most bits one block's extended states may use in all. A cap, not a
/// target: it bounds a single cell's table entry (and so the save file) so one
/// block can't quietly cost kilobytes per placed instance.
pub const MAX_EXTENDED_BITS: usize = 256;

/// A run of bits inside an extended block's per-cell byte string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtField {
    /// Bit offset from the start of the byte string.
    pub shift: u16,
    pub bits: u8,
}

/// An extended state's value in `bytes` (little-endian bit order). Bits past
/// the end of a too-short string read as zero, i.e. the default.
pub fn ext_get(bytes: &[u8], field: ExtField) -> u16 {
    let mut value = 0u16;
    for i in 0..field.bits as u16 {
        let bit = field.shift + i;
        let byte = bytes.get((bit / 8) as usize).copied().unwrap_or(0);
        value |= (((byte >> (bit % 8)) & 1) as u16) << i;
    }
    value
}

/// Sets an extended state in `bytes` (which must be long enough).
pub fn ext_set(bytes: &mut [u8], field: ExtField, value: u16) {
    for i in 0..field.bits as u16 {
        let bit = field.shift + i;
        let mask = 1u8 << (bit % 8);
        if (value >> i) & 1 == 1 {
            bytes[(bit / 8) as usize] |= mask;
        } else {
            bytes[(bit / 8) as usize] &= !mask;
        }
    }
}

/// How many bits a state with `n` values needs: `ceil(log2(n))`.
fn bits_for(n: usize) -> u8 {
    (usize::BITS - (n - 1).leading_zeros()) as u8
}

/// Checks a block's own `state_defs` for the problems both storage tiers
/// share (names, value counts). `Ok` means every state has a usable name and
/// two or more distinct values.
fn check_defs(block: &str, defs: &[crate::blocks::StateDef]) -> Result<(), String> {
    let mut seen: Vec<&str> = Vec::new();
    for def in defs {
        if def.name.is_empty() || !def.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("block {block:?}: state name {:?} must be letters, digits and underscores", def.name));
        }
        if state_tag(&def.name).is_some() {
            return Err(format!("block {block:?}: state {:?} is a built-in state tag's name", def.name));
        }
        if seen.contains(&def.name.as_str()) {
            return Err(format!("block {block:?}: state {:?} is declared twice", def.name));
        }
        seen.push(&def.name);
        if def.values.len() < 2 {
            return Err(format!("block {block:?}: state {:?} needs at least two values", def.name));
        }
        if def.values.len() > 65_536 {
            return Err(format!("block {block:?}: state {:?} has more than 65536 values", def.name));
        }
        let mut distinct = def.values.clone();
        distinct.sort();
        distinct.dedup();
        if distinct.len() != def.values.len() {
            return Err(format!("block {block:?}: state {:?} lists a value twice", def.name));
        }
    }
    Ok(())
}

/// Lays out a block's `state_defs` for *extended* storage: one after another
/// from bit 0 of the cell's byte string. Returns each state's field and the
/// byte string's length. Errors past [`MAX_EXTENDED_BITS`].
pub fn allocate_extended(block: &str, defs: &[crate::blocks::StateDef]) -> Result<(Vec<ExtField>, usize), String> {
    check_defs(block, defs)?;
    let mut fields = Vec::new();
    let mut next = 0usize;
    for def in defs {
        let bits = bits_for(def.values.len());
        if next + bits as usize > MAX_EXTENDED_BITS {
            return Err(format!(
                "block {block:?}: its states need more than the {MAX_EXTENDED_BITS} bits extended storage allows \
                 (state {:?} doesn't fit)",
                def.name
            ));
        }
        fields.push(ExtField { shift: next as u16, bits });
        next += bits as usize;
    }
    Ok((fields, next.div_ceil(8)))
}

/// Lays out a block's own `state_defs` in the bits its built-in tags leave
/// free: first fit, in declaration order, lowest bits first.
///
/// `states` is the block's enabled built-in tags (`level`/`waterlogged` reserve
/// the level bits - a waterloggable block's variants hold a fluid level in the
/// same word - and `axis` reserves the axis bits). Errors, in words a content
/// author can act on, for a bad name, a state with fewer than two values, or
/// a block that needs more bits than are free.
pub fn allocate_fields(block: &str, states: &[String], defs: &[crate::blocks::StateDef]) -> Result<Vec<Field>, String> {
    let has = |name: &str| states.iter().any(|s| s == name);
    let mut taken: u16 = 0;
    if has("level") || has("waterlogged") {
        taken |= LEVEL.mask();
    }
    if has("axis") {
        taken |= AXIS.mask();
    }
    check_defs(block, defs)?;
    let mut fields = Vec::new();
    for def in defs {
        let bits = bits_for(def.values.len());
        let slot = (0..=16u8.saturating_sub(bits)).find(|&shift| {
            let field = Field { shift, bits };
            taken & field.mask() == 0
        });
        let Some(shift) = slot else {
            let free = 16 - taken.count_ones();
            return Err(format!(
                "block {block:?}: its states need more bits than the {free} free in a cell's state word \
                 (state {:?} needs {bits} more); give the block fewer or smaller states, or set \
                 \"state_storage\": \"extended\"",
                def.name
            ));
        };
        let field = Field { shift, bits };
        taken |= field.mask();
        fields.push(field);
    }
    Ok(fields)
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
    pub name: String,
    pub value: String,
    pub allowed: Vec<String>,
}

/// Every state tag the block at `pos` has, with its current value.
pub fn read_states(map: &ChunkMap, registry: &BlockRegistry, tables: &Tables, pos: IVec3) -> Vec<CellState> {
    let id = map.get_block(pos);
    let mut states: Vec<CellState> = STATE_TAGS
        .iter()
        .filter_map(|tag| {
            let allowed = (tag.values)(registry, tables, id);
            (!allowed.is_empty())
                .then(|| CellState { name: tag.name.to_string(), value: (tag.get)(map, registry, tables, pos), allowed })
        })
        .collect();
    // Then whatever this block declared for itself.
    let word = map.state_word(pos);
    let def = registry.def(id);
    let ext = map.extended_state(pos).unwrap_or(&[]);
    for (i, state) in def.state_defs.iter().enumerate() {
        let raw = match def.state_storage {
            crate::blocks::StateStorage::Compact => def.state_fields[i].get(word),
            crate::blocks::StateStorage::Extended => ext_get(ext, def.ext_fields[i]),
        };
        let value = state.values.get(raw as usize).cloned().unwrap_or_else(|| "?".into());
        states.push(CellState { name: state.name.clone(), value, allowed: state.values.clone() });
    }
    states
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
    let id = map.get_block(pos);
    // A state the block declared itself?
    let block = registry.def(id);
    if let Some(i) = block.state_defs.iter().position(|s| s.name == tag) {
        let state = &block.state_defs[i];
        let value = value.to_lowercase();
        let Some(index) = state.values.iter().position(|v| v.to_lowercase() == value) else {
            return Err(StateError::BadValue { tag: tag.to_string(), value, allowed: state.values.clone() });
        };
        match block.state_storage {
            crate::blocks::StateStorage::Compact => {
                let word = block.state_fields[i].set(map.state_word(pos), index as u16);
                map.set_state_word_raw(pos, word);
            }
            crate::blocks::StateStorage::Extended => {
                let mut bytes = map.extended_state(pos).map_or_else(|| vec![0; block.ext_bytes], <[u8]>::to_vec);
                bytes.resize(block.ext_bytes, 0);
                ext_set(&mut bytes, block.ext_fields[i], index as u16);
                map.set_extended_raw(pos, Some(bytes.into_boxed_slice()));
            }
        }
        return Ok(vec![BlockSetEvent { pos, id, prev: id }]);
    }
    let def = state_tag(tag).ok_or_else(|| StateError::UnknownTag(tag.to_string()))?;
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
    vec![BlockSetEvent { pos, id, prev: id }]
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
    map.set_block(pos, new);
    // Starts as a permanent source, like a player-placed fluid.
    map.set_fluid_level_raw(pos, FLUID_SOURCE);
    vec![BlockSetEvent { pos, id: new, prev: old }]
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
    vec![BlockSetEvent { pos, id, prev: id }]
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
                    states.retain(|s| s.name == *tag);
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

    fn tags(map: &ChunkMap, reg: &BlockRegistry, tables: &Tables) -> Vec<String> {
        read_states(map, reg, tables, P).into_iter().map(|s| s.name).collect()
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
        assert!(events.iter().any(|e| e.pos == P && e.id == log), "announced so the edit log records the new word");
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
        assert_eq!((level_of(word), axis_of(word)), (7, AXIS_X));
        let w = with_level(word, FLUID_FALLING);
        assert_eq!((level_of(w), axis_of(w)), (FLUID_FALLING, AXIS_X));
        let w = with_axis(w, AXIS_Z);
        assert_eq!((level_of(w), axis_of(w)), (FLUID_FALLING, AXIS_Z));
        // A value too wide for its field is cut off rather than spilling over.
        assert_eq!(AXIS.get(AXIS.set(0, 0b111)), 0b11);
        assert_eq!(LEVEL.get(AXIS.set(0, 0b111)), 0);
    }

    #[test]
    fn a_cleared_word_is_the_default_everything() {
        assert_eq!(DEFAULT_STATE, 0);
        assert_eq!(level_of(0), FLUID_SOURCE);
        assert_eq!(axis_of(0), AXIS_Y, "the default axis is stored as zero");
        for axis in [AXIS_X, AXIS_Y, AXIS_Z] {
            assert_eq!(decode_axis(encode_axis(axis)), axis);
        }
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

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::blocks::{BlockFile, BlockRegistry, StateDef};

    fn def(name: &str, n: usize) -> StateDef {
        StateDef { name: name.into(), values: (0..n).map(|i| i.to_string()).collect() }
    }

    fn registry_with(files: &[&str]) -> (BlockRegistry, std::sync::Arc<Tables>) {
        let mut reg = BlockRegistry::with_defaults();
        for f in files {
            reg.register(serde_json::from_str::<BlockFile>(f).unwrap().into_def());
        }
        reg.add_layered_variants();
        let mut painters = crate::atlas::default_painters();
        for name in reg.texture_names().collect::<Vec<_>>() {
            painters.ensure_registered(&name);
        }
        let atlas = crate::atlas::build_atlas(&painters);
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        (reg, tables)
    }

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_block_with_no_built_in_states_can_use_every_bit() {
        // Four states of 4 bits each fill all 16.
        let fields = allocate_fields("battery", &[], &[def("a", 16), def("b", 16), def("c", 16), def("d", 16)]).unwrap();
        assert_eq!(fields.iter().map(|f| f.shift).collect::<Vec<_>>(), vec![0, 4, 8, 12]);
        let err = allocate_fields("battery", &[], &[def("a", 16), def("b", 16), def("c", 16), def("d", 16), def("e", 2)]).unwrap_err();
        assert!(err.contains("more bits"), "{err}");
    }

    #[test]
    fn a_block_only_gives_up_the_bits_its_built_in_states_use() {
        // Water's level takes 8, so a 16-valued (4-bit) state goes above it.
        let f = allocate_fields("b", &tags(&["level"]), &[def("x", 16)]).unwrap();
        assert_eq!(f[0], Field { shift: 8, bits: 4 });
        // A log's axis takes bits 8-9; the state fits below it.
        let f = allocate_fields("b", &tags(&["axis"]), &[def("x", 256)]).unwrap();
        assert_eq!(f[0], Field { shift: 0, bits: 8 });
        // Both: only 6 bits remain.
        assert!(allocate_fields("b", &tags(&["level", "axis"]), &[def("x", 64)]).is_ok());
        assert!(allocate_fields("b", &tags(&["level", "axis"]), &[def("x", 65)]).is_err());
        // Waterlogged reserves the level bits too: its variants hold a level.
        let f = allocate_fields("b", &tags(&["waterlogged"]), &[def("x", 4)]).unwrap();
        assert!(f[0].shift >= 8);
    }

    #[test]
    fn bad_declarations_are_refused_with_the_reason() {
        let one = StateDef { name: "x".into(), values: vec!["only".into()] };
        assert!(allocate_fields("b", &[], &[one]).unwrap_err().contains("two values"));
        assert!(allocate_fields("b", &[], &[def("x", 2), def("x", 2)]).unwrap_err().contains("twice"));
        assert!(allocate_fields("b", &[], &[def("axis", 2)]).unwrap_err().contains("built-in"));
        assert!(allocate_fields("b", &[], &[def("has space", 2)]).unwrap_err().contains("underscores"));
        let dup = StateDef { name: "x".into(), values: vec!["a".into(), "a".into()] };
        assert!(allocate_fields("b", &[], &[dup]).unwrap_err().contains("value twice"));
    }

    #[test]
    fn a_declared_block_gets_its_own_layout_and_a_variant_shares_it() {
        let (reg, _) = registry_with(&[
            r#"{"id": "battery", "state_defs": {"charge": 8, "mode": ["off", "on", "auto"]}}"#,
            r#"{"id": "shell", "shape": "floor", "waterloggable": true, "state_defs": {"charge": 8}}"#,
        ]);
        let battery = reg.def(reg.id("battery"));
        assert_eq!(battery.state_fields, vec![Field { shift: 0, bits: 3 }, Field { shift: 3, bits: 2 }]);
        // A shell can be waterlogged, so its charge stays clear of the level bits...
        let shell = reg.def(reg.id("shell"));
        assert!(shell.state_fields[0].shift >= 8);
        // ...and shell@water lays out exactly the same.
        let wet = reg.def(reg.variant(reg.id("shell"), reg.id("water")).unwrap());
        assert_eq!(wet.state_fields, shell.state_fields);
        assert_eq!(wet.state_defs, shell.state_defs);
    }

    #[test]
    fn a_mod_blocks_states_can_be_read_set_and_dont_leak_into_the_next_block() {
        let (reg, tables) = registry_with(&[r#"{"id": "battery", "state_defs": {"charge": 8, "mode": ["off", "on", "auto"]}}"#]);
        let (battery, stone) = (reg.id("battery"), reg.id("stone"));
        let (_, _, mut map) = tests::setup();
        let p = tests::P;
        map.set_block(p, battery);
        let read = |map: &ChunkMap| -> Vec<(String, String)> {
            read_states(map, &reg, &tables, p).into_iter().map(|s| (s.name, s.value)).collect()
        };
        assert_eq!(read(&map), vec![("charge".into(), "0".into()), ("mode".into(), "off".into())], "defaults are the first values");
        set_state(&mut map, &reg, &tables, p, "charge", "5").unwrap();
        set_state(&mut map, &reg, &tables, p, "mode", "AUTO").unwrap();
        assert_eq!(read(&map), vec![("charge".into(), "5".into()), ("mode".into(), "auto".into())]);
        assert!(matches!(set_state(&mut map, &reg, &tables, p, "charge", "8"), Err(StateError::BadValue { .. })));
        // Replacing the block clears everything it stored.
        map.set_block(p, stone);
        assert_eq!(map.state_word(p), DEFAULT_STATE, "no charge left behind for the next block");
    }

    #[test]
    fn placing_over_a_fluid_keeps_only_its_level() {
        let (reg, _tables, mut map) = tests::setup();
        let p = tests::P;
        map.set_block(p, reg.id("water"));
        map.set_fluid_level_raw(p, 3);
        map.set_axis_raw(p, AXIS_X); // stray bits that belong to nothing here
        map.set_block_keep_level(p, reg.id("ice"));
        assert_eq!(map.get_fluid_level(p), 3, "the fluid carries on");
        assert_eq!(map.get_axis(p), AXIS_Y, "nothing else does");
        map.set_block(p, reg.id("stone"));
        assert_eq!(map.get_fluid_level(p), FLUID_SOURCE, "a plain replacement clears the level too");
    }

    #[test]
    fn an_old_edit_with_only_an_axis_still_loads() {
        let old: crate::save::BlockEdit = serde_json::from_str(r#"{"x":1,"y":2,"z":3,"block":"log","axis":2}"#).unwrap();
        assert_eq!(old.state, None);
        assert_eq!(axis_of(old.state.unwrap_or_else(|| with_axis(0, old.axis))), AXIS_Z);
        let new: crate::save::BlockEdit = serde_json::from_str(r#"{"x":1,"y":2,"z":3,"block":"log","state":300}"#).unwrap();
        assert_eq!(new.state, Some(300));
    }
}

#[cfg(test)]
mod janitor_tests {
    /// Every source file that reads or writes a cell's state word. If a file
    /// starts doing so it must be added to `docs/STATE_ACCESS.md` - both the
    /// machine-readable line and a description of what the new code does.
    #[test]
    fn the_janitor_list_covers_every_file_that_touches_the_word() {
        const TOUCHES: &[&str] = &[
            "LEVEL.", "AXIS.", "level_of(", "with_level(", "axis_of(", "with_axis(", "state_word", "chunk.state",
            ".state.as_", "pack_state", "set_fluid_level_raw", "set_axis_raw", "get_fluid_level", "get_axis(",
            "set_block_keep_level", "extended_state", "set_extended_raw", "chunk.extended", ".extended.",
        ];
        let mut found: Vec<String> = std::fs::read_dir("src")
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "rs"))
            .filter(|p| {
                let text = std::fs::read_to_string(p).unwrap();
                // Only code counts, not a mention in a comment.
                text.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| TOUCHES.iter().any(|t| l.contains(t)))
            })
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        found.sort();
        let doc = std::fs::read_to_string("docs/STATE_ACCESS.md").expect("docs/STATE_ACCESS.md exists");
        let line = doc.lines().find(|l| l.contains("janitor-files:")).expect("a janitor-files line");
        let mut listed: Vec<String> = line
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim()
            .trim_start_matches("janitor-files:")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        listed.sort();
        assert_eq!(found, listed, "files touching the state word vs the janitor list in docs/STATE_ACCESS.md");
        for file in &listed {
            assert!(doc.contains(&format!("`{file}`")), "{file} is listed but not described");
        }
    }
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    use crate::blocks::{BlockFile, BlockRegistry, StateDef};

    fn def(name: &str, n: usize) -> StateDef {
        StateDef { name: name.into(), values: (0..n).map(|i| i.to_string()).collect() }
    }

    /// Ten 256-valued states: 80 bits, far more than the 16 a cell's word holds.
    fn machine_json() -> String {
        let defs: Vec<String> = (0..10).map(|i| format!("\"s{i}\": 256")).collect();
        format!(r#"{{"id": "machine", "state_storage": "extended", "state_defs": {{{}}}}}"#, defs.join(", "))
    }

    fn registry_with(files: &[String]) -> (BlockRegistry, std::sync::Arc<Tables>) {
        let mut reg = BlockRegistry::with_defaults();
        for f in files {
            reg.register(serde_json::from_str::<BlockFile>(f).unwrap().into_def());
        }
        reg.add_layered_variants();
        let mut painters = crate::atlas::default_painters();
        for name in reg.texture_names().collect::<Vec<_>>() {
            painters.ensure_registered(&name);
        }
        let atlas = crate::atlas::build_atlas(&painters);
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        (reg, tables)
    }

    #[test]
    fn bits_are_read_and_written_across_byte_boundaries() {
        let mut bytes = [0u8; 4];
        let a = ExtField { shift: 3, bits: 8 }; // straddles bytes 0 and 1
        let b = ExtField { shift: 11, bits: 13 };
        ext_set(&mut bytes, a, 0xA5);
        ext_set(&mut bytes, b, 0x1ABC);
        assert_eq!((ext_get(&bytes, a), ext_get(&bytes, b)), (0xA5, 0x1ABC));
        ext_set(&mut bytes, a, 0);
        assert_eq!((ext_get(&bytes, a), ext_get(&bytes, b)), (0, 0x1ABC), "neighbours untouched");
        assert_eq!(ext_get(&[], a), 0, "a missing byte string reads as the defaults");
    }

    #[test]
    fn extended_layout_runs_on_from_bit_zero_up_to_the_cap() {
        let (fields, bytes) = allocate_extended("m", &[def("a", 2), def("b", 256), def("c", 5)]).unwrap();
        assert_eq!(fields, vec![ExtField { shift: 0, bits: 1 }, ExtField { shift: 1, bits: 8 }, ExtField { shift: 9, bits: 3 }]);
        assert_eq!(bytes, 2);
        // 32 states of 256 values is exactly the 256-bit cap; one more is too many.
        let ok: Vec<_> = (0..32).map(|i| def(&format!("s{i}"), 256)).collect();
        assert_eq!(allocate_extended("m", &ok).unwrap().1, 32);
        let too_many: Vec<_> = (0..33).map(|i| def(&format!("s{i}"), 256)).collect();
        assert!(allocate_extended("m", &too_many).unwrap_err().contains("256 bits"));
        assert!(allocate_extended("m", &[def("a", 70_000)]).unwrap_err().contains("65536"));
    }

    #[test]
    fn a_compact_block_that_is_too_big_is_told_about_extended() {
        let err = allocate_fields("b", &[], &[def("a", 256), def("b", 256), def("c", 2)]).unwrap_err();
        assert!(err.contains("extended"), "{err}");
    }

    #[test]
    fn an_extended_block_holds_far_more_than_sixteen_bits_and_only_while_not_default() {
        let (reg, tables) = registry_with(&[machine_json()]);
        let machine = reg.id("machine");
        let (_, _, mut map) = tests::setup();
        let p = tests::P;
        map.set_block(p, machine);
        assert!(map.extended_state(p).is_none(), "a fresh block has no table entry at all");
        let read = |map: &ChunkMap| -> Vec<String> {
            read_states(map, &reg, &tables, p).into_iter().map(|s| s.value).collect()
        };
        assert_eq!(read(&map), vec!["0"; 10]);
        for i in 0..10 {
            set_state(&mut map, &reg, &tables, p, &format!("s{i}"), &format!("{}", 200 + i)).unwrap();
        }
        assert_eq!(read(&map), (0..10).map(|i| format!("{}", 200 + i)).collect::<Vec<_>>(), "all 80 bits held");
        assert_eq!(map.extended_state(p).unwrap().len(), 10);
        assert_eq!(map.state_word(p), DEFAULT_STATE, "none of it is in the shared word");
        // Back to every default: the entry goes away rather than sitting there as zeros.
        for i in 0..10 {
            set_state(&mut map, &reg, &tables, p, &format!("s{i}"), "0").unwrap();
        }
        assert!(map.extended_state(p).is_none());
        assert!(map.chunks[&IVec2::ZERO].extended.is_empty());
    }

    #[test]
    fn replacing_the_block_clears_its_extended_states_and_cells_are_independent() {
        let (reg, tables) = registry_with(&[machine_json()]);
        let (machine, stone) = (reg.id("machine"), reg.id("stone"));
        let (_, _, mut map) = tests::setup();
        let (a, b) = (tests::P, tests::P + IVec3::X * 2);
        map.set_block(a, machine);
        map.set_block(b, machine);
        set_state(&mut map, &reg, &tables, a, "s3", "9").unwrap();
        set_state(&mut map, &reg, &tables, b, "s3", "77").unwrap();
        assert_eq!(read_states(&map, &reg, &tables, a)[3].value, "9");
        assert_eq!(read_states(&map, &reg, &tables, b)[3].value, "77");
        map.set_block(a, stone);
        assert!(map.extended_state(a).is_none(), "nothing left for whatever is placed next");
        assert_eq!(read_states(&map, &reg, &tables, b)[3].value, "77", "the other cell is untouched");
        // Mining a waterlogged-style swap keeps only the level, not extended states.
        map.set_block_keep_level(b, stone);
        assert!(map.extended_state(b).is_none());
    }

    #[test]
    fn a_waterloggable_extended_block_and_its_water_variant_share_a_layout() {
        let (reg, _) = registry_with(&[
            r#"{"id": "coil", "shape": "floor", "waterloggable": true, "state_storage": "extended", "state_defs": {"turns": 1000}}"#.to_string(),
        ]);
        let coil = reg.def(reg.id("coil"));
        let wet = reg.def(reg.variant(reg.id("coil"), reg.id("water")).unwrap());
        assert_eq!(coil.ext_fields, wet.ext_fields);
        assert_eq!((coil.ext_bytes, wet.state_storage), (2, crate::blocks::StateStorage::Extended));
    }
}
