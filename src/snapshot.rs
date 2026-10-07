//! Chunk snapshots: once a chunk has been generated, its terrain is saved
//! and every later visit loads it back instead of generating it again. A
//! world's existing land therefore never changes when the generator does -
//! only chunks nobody has visited yet use the new generator, and those
//! blend into the old ones beside them (`TerrainGenerator::generate_beside`).
//!
//! A snapshot is the chunk *as generated*, before any player edit: edits
//! and fluid state are still saved and reapplied on top exactly as before
//! (`world.rs`'s `EditLog`/`PendingFluids`), so nothing about them changes.
//!
//! **Which neighbours count as old** is decided by a fingerprint of the
//! generator's own output (`ChunkStore::fingerprint`), stored in every
//! snapshot. A chunk is only blended against if its fingerprint differs
//! from the current one. Treating *every* saved neighbour as old would
//! make each new chunk blend into the last new chunk, which already blended
//! into the one before, and the blend would creep outward forever. Old
//! chunks are never written again, so the set of chunks a new chunk blends
//! against is fixed. That's what keeps the result the same in any order.
//!
//! File layout (`saves/<world>/chunks/c.<cx>.<cz>.bin`): a small plain
//! header - magic, format, chunk dimensions, fingerprint, then each
//! column's ground and water height, which is all a neighbour needs to
//! blend - followed by a deflate-compressed body: a block-name palette and
//! one `u16` palette index per block. Names, not ids, so adding a block
//! file that shifts ids doesn't scramble saved terrain.

use crate::blocks::{BlockId, BlockRegistry, AIR, AXIS_Y, FLUID_SOURCE};
use bevy::log::warn;
use crate::config::{CS, H};
use crate::terrain::{ColumnSurface, GeneratedChunk, OldChunk, TerrainGenerator};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

const MAGIC: &[u8; 4] = b"CMCK";
const FORMAT: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 1 + 1 + 8 + CS * CS * 2;
/// No water over a column, in the header's one-byte water height.
const NO_WATER: u8 = 0;

/// Mixed into the fingerprint. The fingerprint already changes whenever
/// the sampled chunks generate differently; bump this to force it for a
/// change that the samples happen to miss.
const GENERATOR_REVISION: u64 = 1;
/// Chunks whose output makes up the fingerprint - spread out, so a change
/// to oceans, rivers or mountains is unlikely to miss all of them.
const FINGERPRINT_CHUNKS: [(i32, i32); 4] = [(0, 0), (23, -41), (-57, 19), (90, 64)];

/// How many chunks out a new chunk looks for old neighbours. Two, not one:
/// a column one block outside a chunk (which the levee rule reads) can be
/// within `BLEND_DISTANCE` of a chunk two away.
const NEIGHBOUR_RADIUS: i32 = 2;

/// One world's chunk snapshots.
pub struct ChunkStore {
    /// `None` keeps nothing on disk - every chunk is just generated.
    dir: Option<PathBuf>,
    /// Block name per id, for writing.
    names: Vec<String>,
    by_name: HashMap<String, BlockId>,
    fingerprint: OnceLock<u64>,
}

/// A snapshot's header: everything except the blocks themselves.
struct Header {
    fingerprint: u64,
    columns: Vec<ColumnSurface>,
}

impl ChunkStore {
    pub fn new(dir: Option<PathBuf>, registry: &BlockRegistry) -> Self {
        let names: Vec<String> = registry.defs.iter().map(|d| d.id.clone()).collect();
        let by_name = names.iter().enumerate().map(|(i, n)| (n.clone(), i as BlockId)).collect();
        Self { dir, names, by_name, fingerprint: OnceLock::new() }
    }

    /// A hash of what this generator actually produces for this world,
    /// computed once. Two generators that make the same terrain share a
    /// fingerprint, so an unrelated change (a refactor, a new block) never
    /// makes existing chunks count as old.
    pub fn fingerprint(&self, gen: &TerrainGenerator) -> u64 {
        *self.fingerprint.get_or_init(|| {
            let mut h = Fnv::new();
            h.u64(GENERATOR_REVISION);
            for (cx, cz) in FINGERPRINT_CHUNKS {
                let chunk = gen.generate(cx, cz);
                for &id in &chunk.blocks {
                    h.bytes(self.names.get(id as usize).map_or(b"?".as_slice(), |n| n.as_bytes()));
                }
                for c in &chunk.columns {
                    h.u64(c.ground as u64);
                    h.u64(c.water.map_or(u64::MAX, |w| w as u64));
                }
            }
            h.0
        })
    }

    fn path(&self, cx: i32, cz: i32) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(format!("c.{cx}.{cz}.bin")))
    }

    /// Chunk `(cx, cz)`: its snapshot if it has one, otherwise freshly
    /// generated (blending into any old neighbours) and saved.
    pub fn load_or_generate(&self, gen: &TerrainGenerator, cx: i32, cz: i32) -> GeneratedChunk {
        if let Some(chunk) = self.load(gen, cx, cz) {
            return chunk;
        }
        let fingerprint = self.fingerprint(gen);
        let mut old = Vec::new();
        for dz in -NEIGHBOUR_RADIUS..=NEIGHBOUR_RADIUS {
            for dx in -NEIGHBOUR_RADIUS..=NEIGHBOUR_RADIUS {
                if (dx, dz) == (0, 0) {
                    continue;
                }
                let coord = (cx + dx, cz + dz);
                if let Some(header) = self.read_header(coord.0, coord.1) {
                    if header.fingerprint != fingerprint {
                        old.push(OldChunk { coord, columns: header.columns });
                    }
                }
            }
        }
        let chunk = gen.generate_beside(cx, cz, &old);
        self.save(cx, cz, fingerprint, &chunk);
        chunk
    }

    fn read_header(&self, cx: i32, cz: i32) -> Option<Header> {
        let mut file = fs::File::open(self.path(cx, cz)?).ok()?;
        let mut buf = [0u8; HEADER_LEN];
        file.read_exact(&mut buf).ok()?;
        parse_header(&buf)
    }

    /// The saved chunk, or `None` if there isn't one or it can't be read -
    /// in which case it's generated again and the bad file replaced.
    fn load(&self, gen: &TerrainGenerator, cx: i32, cz: i32) -> Option<GeneratedChunk> {
        let path = self.path(cx, cz)?;
        let bytes = fs::read(&path).ok()?;
        let result = self.decode(gen, &bytes);
        if result.is_none() {
            warn!("chunk snapshot {} is unreadable, generating it again", path.display());
        }
        result
    }

    fn decode(&self, gen: &TerrainGenerator, bytes: &[u8]) -> Option<GeneratedChunk> {
        let header = parse_header(bytes.get(..HEADER_LEN)?)?;
        let mut body = Vec::new();
        DeflateDecoder::new(&bytes[HEADER_LEN..]).read_to_end(&mut body).ok()?;
        let mut r = body.as_slice();
        let count = u16::from_le_bytes(take(&mut r, 2)?.try_into().ok()?) as usize;
        let mut palette = Vec::with_capacity(count);
        for _ in 0..count {
            let len = take(&mut r, 1)?[0] as usize;
            let name = std::str::from_utf8(take(&mut r, len)?).ok()?;
            // A block that no longer exists (an uninstalled mod's) becomes
            // air rather than refusing the whole chunk.
            palette.push(self.by_name.get(name).copied().unwrap_or(AIR));
        }
        let indices = take(&mut r, CS * CS * H * 2)?;
        let blocks: Vec<BlockId> = indices
            .chunks_exact(2)
            .map(|b| palette.get(u16::from_le_bytes([b[0], b[1]]) as usize).copied().unwrap_or(AIR))
            .collect();
        Some(GeneratedChunk {
            fluid: vec![FLUID_SOURCE; blocks.len()],
            axis: vec![AXIS_Y; blocks.len()],
            light: gen.sky_columns(&blocks),
            blocks,
            columns: header.columns,
            restored: true,
        })
    }

    fn encode(&self, fingerprint: u64, chunk: &GeneratedChunk) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + 4096);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&[FORMAT, CS as u8, H as u8]);
        out.extend_from_slice(&fingerprint.to_le_bytes());
        for c in &chunk.columns {
            // Heights are 0..H, so `water + 1` never collides with NO_WATER.
            out.push(c.ground.clamp(0, H as i32 - 1) as u8);
            out.push(c.water.map_or(NO_WATER, |w| w.clamp(0, H as i32 - 2) as u8 + 1));
        }

        let mut palette: Vec<BlockId> = Vec::new();
        let mut index_of: HashMap<BlockId, u16> = HashMap::new();
        let mut indices = Vec::with_capacity(chunk.blocks.len() * 2);
        for &id in &chunk.blocks {
            let i = *index_of.entry(id).or_insert_with(|| {
                palette.push(id);
                (palette.len() - 1) as u16
            });
            indices.extend_from_slice(&i.to_le_bytes());
        }
        let mut body = Vec::new();
        body.extend_from_slice(&(palette.len() as u16).to_le_bytes());
        for id in palette {
            let name = self.names.get(id as usize).map_or("", |n| n.as_str());
            let name = &name.as_bytes()[..name.len().min(255)];
            body.push(name.len() as u8);
            body.extend_from_slice(name);
        }
        body.extend_from_slice(&indices);

        let mut encoder = DeflateEncoder::new(out, Compression::fast());
        // Writing into a Vec can't fail.
        encoder.write_all(&body).unwrap();
        encoder.finish().unwrap()
    }

    /// Saves the chunk, via a temporary file renamed into place, so a
    /// neighbour reading it at the same moment sees the whole file or none
    /// of it. A failed write is only logged: the chunk generates again next
    /// time, the same as before snapshots existed.
    fn save(&self, cx: i32, cz: i32, fingerprint: u64, chunk: &GeneratedChunk) {
        static TEMP: AtomicU64 = AtomicU64::new(0);
        let Some(path) = self.path(cx, cz) else { return };
        let bytes = self.encode(fingerprint, chunk);
        let temp = path.with_extension(format!("tmp{}", TEMP.fetch_add(1, Ordering::Relaxed)));
        let result = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&temp, &bytes))
            .and_then(|_| fs::rename(&temp, &path));
        if let Err(e) = result {
            let _ = fs::remove_file(&temp);
            warn!("couldn't save chunk snapshot {}: {e}", path.display());
        }
    }
}

fn parse_header(buf: &[u8]) -> Option<Header> {
    if buf.len() < HEADER_LEN || &buf[..4] != MAGIC || buf[4] != FORMAT || buf[5] as usize != CS || buf[6] as usize != H {
        return None;
    }
    let fingerprint = u64::from_le_bytes(buf[7..15].try_into().ok()?);
    let columns = buf[15..HEADER_LEN]
        .chunks_exact(2)
        .map(|c| ColumnSurface {
            ground: c[0] as i32,
            water: (c[1] != NO_WATER).then(|| c[1] as i32 - 1),
        })
        .collect();
    Some(Header { fingerprint, columns })
}

fn take<'a>(r: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    if r.len() < n {
        return None;
    }
    let (head, tail) = r.split_at(n);
    *r = tail;
    Some(head)
}

/// FNV-1a: stable across Rust versions, unlike `DefaultHasher`, so a
/// toolchain update can't make every chunk look old.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        // Separator, so ["ab","c"] and ["a","bc"] differ.
        self.0 = (self.0 ^ 0xff).wrapping_mul(0x0100_0000_01b3);
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A scratch directory unique per call (see CLAUDE.md on why the
    /// counter matters), removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("craftmjne-snapshot-test-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
        fn store(&self, reg: &BlockRegistry) -> ChunkStore {
            ChunkStore::new(Some(self.0.clone()), reg)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap().flatten() {
            fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }

    #[test]
    fn a_snapshot_round_trips_exactly() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(3, &reg);
        let store = ChunkStore::new(None, &reg);
        let chunk = gen.generate(2, -1);
        let back = store.decode(&gen, &store.encode(42, &chunk)).unwrap();
        assert!(back.blocks == chunk.blocks);
        assert_eq!(back.columns, chunk.columns);
        assert_eq!(parse_header(&store.encode(42, &chunk)).unwrap().fingerprint, 42);
    }

    /// The feature itself: a chunk, once generated, stays what it was even
    /// after the generator changes (a different seed stands in for a
    /// reworked generator - it makes entirely different terrain).
    #[test]
    fn a_visited_chunk_keeps_its_terrain_when_the_generator_changes() {
        let reg = BlockRegistry::with_defaults();
        let dir = TempDir::new();
        let old_gen = TerrainGenerator::new(1, &reg);
        let first = dir.store(&reg).load_or_generate(&old_gen, 0, 0);
        let new_gen = TerrainGenerator::new(2, &reg);
        assert!(new_gen.generate(0, 0).blocks != first.blocks, "test setup: the generators should differ");
        let again = dir.store(&reg).load_or_generate(&new_gen, 0, 0);
        assert!(again.blocks == first.blocks);
    }

    #[test]
    fn a_new_chunk_blends_into_an_old_neighbour_but_not_a_current_one() {
        let reg = BlockRegistry::with_defaults();
        let dir = TempDir::new();
        let old_gen = TerrainGenerator::new(1, &reg);
        dir.store(&reg).load_or_generate(&old_gen, 0, 0);

        // Same generator: the neighbour isn't old, nothing to blend.
        let same = TempDir::new();
        copy_dir(&dir.0, &same.0);
        assert!(same.store(&reg).load_or_generate(&old_gen, 1, 0).blocks == old_gen.generate(1, 0).blocks);

        // A changed generator: the new chunk blends into the saved one.
        let new_gen = TerrainGenerator::new(2, &reg);
        let blended = dir.store(&reg).load_or_generate(&new_gen, 1, 0);
        let old = parse_header(&fs::read(dir.0.join("c.0.0.bin")).unwrap()).unwrap();
        let expected = new_gen.generate_beside(1, 0, &[OldChunk { coord: (0, 0), columns: old.columns }]);
        assert!(blended.blocks == expected.blocks);
        assert!(blended.blocks != new_gen.generate(1, 0).blocks, "test setup: the blend should change something");
    }

    /// New chunks blend only into *old* chunks, never into each other, so
    /// it can't matter which of two new neighbours generated first.
    #[test]
    fn new_chunks_come_out_the_same_whichever_generates_first() {
        let reg = BlockRegistry::with_defaults();
        let (a, b) = (TempDir::new(), TempDir::new());
        let old_gen = TerrainGenerator::new(1, &reg);
        a.store(&reg).load_or_generate(&old_gen, 0, 0);
        copy_dir(&a.0, &b.0);

        let new_gen = TerrainGenerator::new(2, &reg);
        let (sa, sb) = (a.store(&reg), b.store(&reg));
        let a1 = sa.load_or_generate(&new_gen, 1, 0);
        let a2 = sa.load_or_generate(&new_gen, 1, 1);
        let b2 = sb.load_or_generate(&new_gen, 1, 1);
        let b1 = sb.load_or_generate(&new_gen, 1, 0);
        assert!(a1.blocks == b1.blocks && a2.blocks == b2.blocks);
    }

    #[test]
    fn an_unreadable_snapshot_is_generated_again_and_replaced() {
        let reg = BlockRegistry::with_defaults();
        let dir = TempDir::new();
        let gen = TerrainGenerator::new(1, &reg);
        let store = dir.store(&reg);
        let good = store.load_or_generate(&gen, 0, 0);
        let path = dir.0.join("c.0.0.bin");
        let mut bytes = fs::read(&path).unwrap();
        bytes.truncate(HEADER_LEN + 10);
        fs::write(&path, bytes).unwrap();
        assert!(store.load_or_generate(&gen, 0, 0).blocks == good.blocks);
        assert!(store.load(&gen, 0, 0).is_some(), "the bad file should have been replaced");
    }

    #[test]
    fn a_block_that_no_longer_exists_loads_as_air() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(3, &reg);
        let mut store = ChunkStore::new(None, &reg);
        let chunk = gen.generate(0, 0);
        let stone = reg.id("stone");
        assert!(chunk.blocks.contains(&stone));
        store.names[stone as usize] = "uninstalled_mod_stone".into();
        let bytes = store.encode(0, &chunk);
        let store = ChunkStore::new(None, &reg);
        let back = store.decode(&gen, &bytes).unwrap();
        for (a, b) in chunk.blocks.iter().zip(&back.blocks) {
            assert_eq!(*b, if *a == stone { AIR } else { *a });
        }
    }
}
