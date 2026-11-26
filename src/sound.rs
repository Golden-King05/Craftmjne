//! Sound: what the game plays, where each sound comes from, and how loud.
//!
//! Every sound is an *event name* - `block.stone.break`, `block.grass.step`, `item.pickup`,
//! `entity.hit`, `player.hurt`, `ui.click`. A block picks its group with the `sound` tag
//! (`stone`, `dirt`, `grass`, `wood`, `sand`, `gravel`, `glass`, `snow`, ...), and `block.<group>.
//! <action>` is looked up for `break`, `place` and `step`.
//!
//! The game ships no audio files: every event has a default sound *synthesised* here (seeded
//! noise shaped per group, deterministic), encoded as a small WAV in memory. A resource pack
//! overrides or adds one by dropping a file at the event's path under its `sounds/` folder:
//! `sounds/block/stone/break.ogg` (or `.wav`). Several variants are `break1.ogg`, `break2.ogg`...;
//! the highest layer that has any of an event replaces the lower layers' entirely. A group no pack
//! or default knows (a mod's `granite`) falls back to `stone` until its pack supplies sounds.
//! Sound paths are *not* namespaced - the event names are one shared vocabulary - so a mod's pack
//! overrides vanilla's simply by providing the same path, and a player's pack outranks both.
//!
//! The pure parts (synthesis, WAV encoding, pack lookup, volume maths) are tested without audio;
//! playback goes through Bevy's audio and silently does nothing where there is no sound device.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::noise::hash_str;
use crate::overlay::KindDir;

/// Sound groups every block may name; others fall back to [`DEFAULT_GROUP`].
pub const GROUPS: [&str; 9] = ["stone", "dirt", "grass", "wood", "sand", "gravel", "glass", "snow", "water"];
pub const DEFAULT_GROUP: &str = "stone";
/// What a block with no `sound` tag uses.
pub const ACTIONS: [&str; 3] = ["break", "place", "step"];
/// The events that aren't per-block-group.
pub const FIXED_EVENTS: [&str; 4] = ["item.pickup", "entity.hit", "player.hurt", "ui.click"];
const SAMPLE_RATE: u32 = 22_050;
/// Variants synthesised per event, and the most a pack may supply.
const DEFAULT_VARIANTS: u32 = 3;
const MAX_VARIANTS: u32 = 9;

pub fn block_event(group: &str, action: &str) -> String {
    format!("block.{group}.{action}")
}

/// The volume category an event belongs to (what the Sound settings page sliders control).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundCategory {
    Blocks,
    Player,
    Ui,
}

impl SoundCategory {
    pub fn of(event: &str) -> Self {
        if event.starts_with("block.") {
            Self::Blocks
        } else if event.starts_with("ui.") {
            Self::Ui
        } else {
            Self::Player
        }
    }
}

// ---------------------------------------------------------------------------
// Volume settings
// ---------------------------------------------------------------------------

/// The player's volume sliders, whole percents (so the struct stays `Eq`, like `ControlOptions`).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SoundOptions {
    #[serde(default = "full")]
    pub master: u8,
    #[serde(default = "full")]
    pub blocks: u8,
    #[serde(default = "full")]
    pub player: u8,
    #[serde(default = "full")]
    pub ui: u8,
}

fn full() -> u8 {
    100
}

impl Default for SoundOptions {
    fn default() -> Self {
        Self { master: 100, blocks: 100, player: 100, ui: 100 }
    }
}

/// Which slider.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slider {
    Master,
    Blocks,
    Player,
    Ui,
}

impl Slider {
    pub const ALL: [Slider; 4] = [Slider::Master, Slider::Blocks, Slider::Player, Slider::Ui];
    pub fn label(self) -> &'static str {
        match self {
            Slider::Master => "Master volume",
            Slider::Blocks => "Blocks",
            Slider::Player => "Player and items",
            Slider::Ui => "Menus",
        }
    }
}

impl SoundOptions {
    pub fn get(&self, slider: Slider) -> u8 {
        match slider {
            Slider::Master => self.master,
            Slider::Blocks => self.blocks,
            Slider::Player => self.player,
            Slider::Ui => self.ui,
        }
    }

    /// Moves a slider by `delta` percent, stopping at 0 and 100.
    pub fn nudge(&mut self, slider: Slider, delta: i32) {
        let v = (self.get(slider) as i32 + delta).clamp(0, 100) as u8;
        match slider {
            Slider::Master => self.master = v,
            Slider::Blocks => self.blocks = v,
            Slider::Player => self.player = v,
            Slider::Ui => self.ui = v,
        }
    }

    /// The linear gain for an event: master times its category, each `0..=1`.
    pub fn gain(&self, event: &str) -> f32 {
        let category = match SoundCategory::of(event) {
            SoundCategory::Blocks => self.blocks,
            SoundCategory::Player => self.player,
            SoundCategory::Ui => self.ui,
        };
        self.master.min(100) as f32 / 100.0 * category.min(100) as f32 / 100.0
    }
}

/// How much of its volume a sound `distance` blocks away keeps: full within 4, none by 32.
pub fn falloff(distance: f32) -> f32 {
    ((32.0 - distance) / 28.0).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Synthesis
// ---------------------------------------------------------------------------

/// How one group's sounds are shaped.
struct Voice {
    /// Pitch of the tonal part, Hz (0 = none), and how much of the sound it is.
    tone: f32,
    tonal: f32,
    /// One-pole lowpass strength on the noise (0 = bright, near 1 = dull).
    dull: f32,
    /// Decay rate: bigger dies faster.
    decay: f32,
}

fn voice(group: &str) -> Voice {
    match group {
        "dirt" => Voice { tone: 90.0, tonal: 0.35, dull: 0.85, decay: 22.0 },
        "grass" => Voice { tone: 0.0, tonal: 0.0, dull: 0.55, decay: 16.0 },
        "wood" => Voice { tone: 190.0, tonal: 0.7, dull: 0.7, decay: 20.0 },
        "sand" => Voice { tone: 0.0, tonal: 0.0, dull: 0.35, decay: 11.0 },
        "gravel" => Voice { tone: 0.0, tonal: 0.0, dull: 0.2, decay: 14.0 },
        "glass" => Voice { tone: 1900.0, tonal: 0.75, dull: 0.1, decay: 9.0 },
        "snow" => Voice { tone: 0.0, tonal: 0.0, dull: 0.8, decay: 13.0 },
        "water" => Voice { tone: 0.0, tonal: 0.0, dull: 0.9, decay: 7.0 },
        _ => Voice { tone: 260.0, tonal: 0.3, dull: 0.25, decay: 24.0 },
    }
}

/// A seeded stream of noise in -1..1 (xorshift), so a sound is the same every run.
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// The default samples for `event` (variant `variant`): mono, [`SAMPLE_RATE`], within -1..1.
pub fn synthesize(event: &str, variant: u32) -> Vec<f32> {
    let seed = hash_str(event).wrapping_add(variant.wrapping_mul(7919)) | 1;
    let mut noise = Noise(seed);
    let parts: Vec<&str> = event.split('.').collect();
    // (group voice, length seconds, gain)
    let (v, secs, gain) = match parts.as_slice() {
        ["block", group, "break"] => (voice(group), 0.28, 0.9),
        ["block", group, "place"] => (voice(group), 0.16, 0.7),
        ["block", group, _] => (voice(group), 0.09, 0.4),
        ["item", "pickup"] => (Voice { tone: 880.0, tonal: 1.0, dull: 0.0, decay: 14.0 }, 0.14, 0.45),
        ["entity", "hit"] => (Voice { tone: 140.0, tonal: 0.5, dull: 0.7, decay: 18.0 }, 0.2, 0.9),
        ["player", "hurt"] => (Voice { tone: 100.0, tonal: 0.8, dull: 0.6, decay: 9.0 }, 0.3, 0.9),
        ["ui", "click"] => (Voice { tone: 1200.0, tonal: 0.6, dull: 0.2, decay: 60.0 }, 0.05, 0.5),
        _ => (voice(DEFAULT_GROUP), 0.1, 0.5),
    };
    // Each variant detunes slightly so repeats don't sound identical.
    let tone = v.tone * (1.0 + 0.06 * variant as f32);
    let n = (SAMPLE_RATE as f32 * secs) as usize;
    let mut low = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SAMPLE_RATE as f32;
        // A pickup rises; everything else is steady.
        let pitch = if event == "item.pickup" { tone * (1.0 + 0.8 * t / secs) } else { tone };
        low = low * v.dull + noise.next() * (1.0 - v.dull);
        let tonal = if v.tone > 0.0 { (t * pitch * std::f32::consts::TAU).sin() } else { 0.0 };
        let env = (-t * v.decay).exp() * (1.0 - (-t * 400.0).exp()); // quick attack, exponential tail
        out.push(((low * (1.0 - v.tonal) + tonal * v.tonal) * env * gain).clamp(-1.0, 1.0));
    }
    out
}

/// A 16-bit mono PCM WAV file holding `samples`.
pub fn encode_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// The set of sounds, from defaults and packs
// ---------------------------------------------------------------------------

/// Every event's clips as encoded audio files (WAV or Ogg), ready to hand to Bevy.
#[derive(Resource, Clone, Default, Debug)]
pub struct SoundSet {
    pub clips: HashMap<String, Vec<Arc<[u8]>>>,
    /// Pack sound files that were skipped (and why), for the log and the pack's author.
    pub notes: Vec<String>,
}

impl SoundSet {
    pub fn get(&self, event: &str) -> &[Arc<[u8]>] {
        self.clips.get(event).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Which variant plays the `n`th time: cycles, so repeats alternate.
    pub fn pick(&self, event: &str, n: u32) -> Option<&Arc<[u8]>> {
        let clips = self.get(event);
        (!clips.is_empty()).then(|| &clips[n as usize % clips.len()])
    }
}

/// Whether `bytes` look like a WAV or Ogg file (all Bevy is built to decode here). A cheap check
/// that keeps a mislabelled or empty file from reaching the audio thread.
pub fn looks_like_audio(bytes: &[u8]) -> bool {
    (bytes.len() > 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE") || (bytes.len() > 27 && &bytes[0..4] == b"OggS")
}

/// The files a pack layer holds for `path` (`block/stone/break`): `path.ext` and `path1..9.ext`.
fn layer_files(kd: &KindDir, path: &str) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for ext in ["ogg", "wav"] {
        let plain = kd.dir.join(format!("{path}.{ext}"));
        if plain.is_file() {
            found.push(plain);
        }
        for n in 1..=MAX_VARIANTS {
            let p = kd.dir.join(format!("{path}{n}.{ext}"));
            if p.is_file() {
                found.push(p);
            }
        }
    }
    found
}

/// The pack clips for an event: the highest layer holding any, skipping unreadable or non-audio
/// files with a note. Empty if no layer overrides it.
fn pack_clips(dirs: &[KindDir], event: &str, notes: &mut Vec<String>) -> Vec<Arc<[u8]>> {
    let path = event.replace('.', "/");
    for kd in dirs.iter().rev() {
        let mut clips = Vec::new();
        for file in layer_files(kd, &path) {
            match std::fs::read(&file) {
                Ok(bytes) if looks_like_audio(&bytes) => clips.push(Arc::from(bytes)),
                Ok(_) => notes.push(format!("{}: not a WAV or Ogg file, skipped", file.display())),
                Err(e) => notes.push(format!("{}: {e}", file.display())),
            }
        }
        if !clips.is_empty() {
            return clips;
        }
    }
    Vec::new()
}

/// Builds the sound set: defaults for every event, replaced wherever a pack (layers lowest first)
/// supplies files. `groups` are the sound groups the loaded blocks use, so a mod's own group gets
/// events too; one with no default voice of its own is shaped like [`DEFAULT_GROUP`].
pub fn build_sound_set(dirs: &[KindDir], groups: &[String]) -> SoundSet {
    let mut set = SoundSet::default();
    let mut all_groups: Vec<String> = GROUPS.iter().map(|g| g.to_string()).collect();
    for g in groups {
        if !all_groups.contains(g) {
            all_groups.push(g.clone());
        }
    }
    let mut events: Vec<String> = FIXED_EVENTS.iter().map(|e| e.to_string()).collect();
    for g in &all_groups {
        for a in ACTIONS {
            events.push(block_event(g, a));
        }
    }
    for event in events {
        let mut clips = pack_clips(dirs, &event, &mut set.notes);
        if clips.is_empty() {
            clips = (0..DEFAULT_VARIANTS).map(|v| Arc::from(encode_wav(&synthesize(&event, v), SAMPLE_RATE))).collect();
        }
        set.clips.insert(event, clips);
    }
    set
}

/// The event a block's sound should use: its group's, or the default group's if the set has none.
pub fn resolve_event(set: &SoundSet, group: &str, action: &str) -> String {
    let own = block_event(group, action);
    if set.clips.contains_key(&own) { own } else { block_event(DEFAULT_GROUP, action) }
}

/// Where a pack's `sounds/` folder reads from, for docs and messages.
pub fn describe_layout() -> &'static str {
    "sounds/<event path>.ogg|.wav  (e.g. sounds/block/stone/break.ogg; variants break1.ogg, break2.ogg...)"
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

/// Play `name` (an event, or a group event resolved by the sender), optionally at a world
/// position (quieter with distance from the player).
#[derive(Event, Clone, Debug)]
pub struct SoundEvent {
    pub name: String,
    pub pos: Option<Vec3>,
}

impl SoundEvent {
    pub fn global(name: &str) -> Self {
        Self { name: name.to_string(), pos: None }
    }
    pub fn at(name: &str, pos: Vec3) -> Self {
        Self { name: name.to_string(), pos: Some(pos) }
    }
}

/// The set's clips as Bevy audio handles, rebuilt whenever [`SoundSet`] changes.
#[derive(Resource, Default)]
pub struct SoundBank {
    clips: HashMap<String, Vec<Handle<AudioSource>>>,
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        let options = app.world().get_resource::<crate::save::SaveStore>().map(|s| s.load_sound_options()).unwrap_or_default();
        app.add_event::<SoundEvent>()
            .insert_resource(options)
            .init_resource::<SoundSet>()
            .init_resource::<SoundBank>()
            .add_systems(
                Update,
                (rebuild_bank, ui_clicks, footsteps, damage_sounds, hurt_sound, play_sounds).chain(),
            );
    }
}

fn rebuild_bank(set: Res<SoundSet>, mut bank: ResMut<SoundBank>, assets: Option<ResMut<Assets<AudioSource>>>) {
    if !set.is_changed() {
        return;
    }
    let Some(mut assets) = assets else { return };
    bank.clips = set
        .clips
        .iter()
        .map(|(name, clips)| (name.clone(), clips.iter().map(|b| assets.add(AudioSource { bytes: b.clone() })).collect()))
        .collect();
}

fn ui_clicks(buttons: Query<&Interaction, (Changed<Interaction>, With<Button>)>, mut out: EventWriter<SoundEvent>) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        out.write(SoundEvent::global("ui.click"));
    }
}

fn footsteps(
    players: Query<&crate::player::Player>,
    map: Res<crate::world::ChunkMap>,
    registry: Option<Res<crate::blocks::BlockRegistry>>,
    set: Res<SoundSet>,
    mut walked: Local<(f32, Option<Vec3>)>,
    mut out: EventWriter<SoundEvent>,
) {
    let (Ok(player), Some(registry)) = (players.single(), registry) else { return };
    let last = walked.1.replace(player.pos);
    let Some(last) = last else { return };
    let moved = Vec2::new(player.pos.x - last.x, player.pos.z - last.z).length();
    if !player.on_ground || player.fly || moved > 3.0 {
        return; // airborne, flying, or teleported
    }
    walked.0 += moved;
    if walked.0 < STEP_DISTANCE {
        return;
    }
    walked.0 = 0.0;
    let under = (player.pos - Vec3::Y * 0.1).floor().as_ivec3();
    let id = map.get_block(under);
    if id == crate::blocks::AIR {
        return;
    }
    let event = resolve_event(&set, &registry.def(id).sound, "step");
    out.write(SoundEvent::at(&event, player.pos));
}

/// Blocks walked between footsteps.
pub const STEP_DISTANCE: f32 = 1.8;

fn damage_sounds(mut events: EventReader<crate::damage::DamageEvent>, mut out: EventWriter<SoundEvent>) {
    for e in events.read() {
        out.write(SoundEvent::at("entity.hit", e.cell.as_vec3() + Vec3::splat(0.5)));
    }
}

fn hurt_sound(players: Query<&crate::player::Player>, mut last: Local<Option<f32>>, mut out: EventWriter<SoundEvent>) {
    let Ok(player) = players.single() else { return };
    let health = player.vitals.health;
    if matches!(*last, Some(prev) if health < prev) {
        out.write(SoundEvent::global("player.hurt"));
    }
    *last = Some(health);
}

fn play_sounds(
    mut events: EventReader<SoundEvent>,
    bank: Res<SoundBank>,
    options: Res<SoundOptions>,
    players: Query<&crate::player::Player>,
    mut commands: Commands,
    mut counter: Local<u32>,
) {
    let listener = players.single().map(|p| p.pos).ok();
    for e in events.read() {
        let mut gain = options.gain(&e.name);
        if let (Some(at), Some(me)) = (e.pos, listener) {
            gain *= falloff(at.distance(me));
        }
        let Some(handles) = bank.clips.get(&e.name) else { continue };
        if gain <= 0.0 || handles.is_empty() {
            continue;
        }
        *counter = counter.wrapping_add(1);
        let handle = handles[*counter as usize % handles.len()].clone();
        commands.spawn((AudioPlayer::new(handle), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn scratch(tag: &str) -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-sound-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn layer(dir: &Path) -> KindDir {
        KindDir { ns: "pack".into(), dir: dir.to_path_buf(), locals: Default::default(), extends: true }
    }

    fn put(dir: &Path, rel: &str, bytes: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn wav() -> Vec<u8> {
        encode_wav(&[0.0, 0.5, -0.5, 0.25, 0.0, 0.1, -0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 8000)
    }

    #[test]
    fn synthesis_is_deterministic_audible_and_in_range() {
        for event in ["block.stone.break", "block.grass.step", "item.pickup", "ui.click", "block.unknown.place"] {
            let a = synthesize(event, 0);
            assert_eq!(a, synthesize(event, 0), "{event} must be reproducible");
            assert!(a.iter().all(|s| (-1.0..=1.0).contains(s)));
            assert!(a.iter().any(|s| s.abs() > 0.05), "{event} is silent");
        }
        assert_ne!(synthesize("block.stone.break", 0), synthesize("block.stone.break", 1), "variants differ");
        assert_ne!(synthesize("block.stone.break", 0), synthesize("block.glass.break", 0), "groups differ");
        assert!(synthesize("block.stone.break", 0).len() > synthesize("block.stone.step", 0).len(), "breaking rings longer than a step");
    }

    #[test]
    fn a_wav_has_a_valid_header_and_the_right_length() {
        let bytes = encode_wav(&[0.0; 100], 22_050);
        assert!(looks_like_audio(&bytes));
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 200);
        assert_eq!(bytes.len(), 44 + 200);
    }

    #[test]
    fn every_event_has_a_default_with_variants() {
        let set = build_sound_set(&[], &["granite".to_string()]);
        for g in GROUPS.iter().copied().chain(["granite"]) {
            for a in ACTIONS {
                assert_eq!(set.get(&block_event(g, a)).len(), DEFAULT_VARIANTS as usize, "{g}.{a}");
            }
        }
        for e in FIXED_EVENTS {
            assert!(!set.get(e).is_empty(), "{e}");
        }
        assert!(set.get("block.stone.break").iter().all(|c| looks_like_audio(c)));
    }

    #[test]
    fn a_pack_file_replaces_the_default_and_the_highest_layer_wins() {
        let (low, high) = (scratch("low"), scratch("high"));
        let (a, b) = (wav(), encode_wav(&[0.3; 40], 8000));
        put(&low, "sounds/block/stone/break.wav", &a);
        put(&high, "sounds/block/stone/break.wav", &b);
        // Directories here are the `sounds` folders themselves.
        let dirs = [layer(&low.join("sounds")), layer(&high.join("sounds"))];
        let set = build_sound_set(&dirs, &[]);
        assert_eq!(set.get("block.stone.break").len(), 1);
        assert_eq!(&*set.get("block.stone.break")[0], b.as_slice());
        // Untouched events keep the synthesised default.
        assert_eq!(set.get("block.stone.place").len(), DEFAULT_VARIANTS as usize);
        // Dropping the high layer shows the lower one.
        let set = build_sound_set(&dirs[..1], &[]);
        assert_eq!(&*set.get("block.stone.break")[0], a.as_slice());
    }

    #[test]
    fn numbered_files_are_variants_of_one_event() {
        let d = scratch("variants");
        put(&d, "block/dirt/step.wav", &wav());
        put(&d, "block/dirt/step1.wav", &wav());
        put(&d, "block/dirt/step2.wav", &wav());
        let set = build_sound_set(&[layer(&d)], &[]);
        assert_eq!(set.get("block.dirt.step").len(), 3);
    }

    #[test]
    fn a_file_that_is_not_audio_is_skipped_with_a_note_not_trusted() {
        let d = scratch("bad");
        put(&d, "block/stone/break.wav", b"this is not a wav file at all, not even close............");
        let set = build_sound_set(&[layer(&d)], &[]);
        assert_eq!(set.get("block.stone.break").len(), DEFAULT_VARIANTS as usize, "falls back to the default");
        assert!(set.notes.iter().any(|n| n.contains("break.wav") && n.contains("not a WAV")), "{:?}", set.notes);
    }

    #[test]
    fn an_unknown_group_falls_back_to_the_default_groups_sound() {
        let set = build_sound_set(&[], &[]);
        assert_eq!(resolve_event(&set, "stone", "step"), "block.stone.step");
        assert_eq!(resolve_event(&set, "mystery", "step"), "block.stone.step");
    }

    #[test]
    fn variants_cycle() {
        let set = build_sound_set(&[], &[]);
        let a = set.pick("block.stone.step", 0).unwrap();
        let b = set.pick("block.stone.step", 1).unwrap();
        assert_ne!(**a, **b);
        assert_eq!(**a, **set.pick("block.stone.step", DEFAULT_VARIANTS).unwrap());
        assert!(set.pick("no.such.event", 0).is_none());
    }

    #[test]
    fn volume_is_master_times_category_and_fades_with_distance() {
        let mut o = SoundOptions::default();
        assert_eq!(o.gain("block.stone.break"), 1.0);
        o.master = 50;
        o.blocks = 50;
        assert!((o.gain("block.stone.break") - 0.25).abs() < 1e-6);
        assert!((o.gain("ui.click") - 0.5).abs() < 1e-6, "menus use their own slider");
        o.ui = 0;
        assert_eq!(o.gain("ui.click"), 0.0);
        assert_eq!(SoundCategory::of("item.pickup"), SoundCategory::Player);
        assert_eq!(falloff(2.0), 1.0);
        assert_eq!(falloff(40.0), 0.0);
        assert!(falloff(18.0) > 0.0 && falloff(18.0) < 1.0);
    }

    #[test]
    fn sliders_stop_at_the_ends_and_read_old_files() {
        let mut o = SoundOptions::default();
        o.nudge(Slider::Master, 25);
        assert_eq!(o.master, 100);
        o.nudge(Slider::Ui, -250);
        assert_eq!(o.ui, 0);
        let parsed: SoundOptions = serde_json::from_str(r#"{"master": 30}"#).unwrap();
        assert_eq!((parsed.master, parsed.blocks, parsed.ui), (30, 100, 100));
    }
}
