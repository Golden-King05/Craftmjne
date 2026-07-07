// World: owns chunk data, streams chunks around the player, and coordinates
// generation/meshing jobs on the worker pool.
//
// Chunk lifecycle:
//   requested -> generating (worker) -> ready -> meshing (worker) -> rendered
//
// Block data for visited chunks is kept in memory even when their meshes are
// unloaded, so player edits persist while roaming. Meshing requires the eight
// neighbouring chunks to exist (the mesher gets a 1-block padded shell for
// seamless culling and ambient occlusion across chunk borders).

import { CHUNK_SIZE as CS, WORLD_HEIGHT as H, blockIndex } from '../config.js';
import { PAD_XZ, PAD_Y, paddedIndex } from '../mesh/ChunkMesher.js';

const chunkKey = (cx, cz) => cx + ',' + cz;

const NEIGHBORS_8 = [
  [-1, -1], [0, -1], [1, -1],
  [-1, 0], [1, 0],
  [-1, 1], [0, 1], [1, 1],
];

class Chunk {
  constructor(cx, cz) {
    this.cx = cx;
    this.cz = cz;
    this.blocks = null;   // Uint16Array once generated
    this.version = 0;     // bumped on every edit
    this.dirty = false;   // needs (re)meshing
    this.meshing = false; // a mesh job is in flight
    this.meshed = false;  // renderer currently holds a mesh for this chunk
  }
}

export class World {
  constructor(engine) {
    this.engine = engine;
    this.registry = engine.blocks;
    this.pool = engine.pool;
    this.renderer = engine.renderer;
    this.events = engine.events;

    this.chunks = new Map();
    this.renderDistance = engine.config.renderDistance;
    this.genInFlight = 0;
    this.meshInFlight = 0;
    this.maxGenJobs = this.pool.size * 3;
    this.maxMeshJobs = this.pool.size * 2;
    this.needsScan = true;
    this.lastPlayerChunk = null;
  }

  update() {
    const p = this.engine.player.pos;
    const pcx = Math.floor(p.x / CS);
    const pcz = Math.floor(p.z / CS);
    if (!this.lastPlayerChunk || pcx !== this.lastPlayerChunk[0] || pcz !== this.lastPlayerChunk[1]) {
      this.lastPlayerChunk = [pcx, pcz];
      this.needsScan = true;
    }
    if (this.needsScan) {
      this.needsScan = false;
      this.scan(pcx, pcz);
    }
  }

  // Figure out what to generate/mesh/unload. Cheap (a few hundred map hits),
  // and only runs when something changed.
  scan(pcx, pcz) {
    const R = this.renderDistance;
    const RD = R + 1; // data radius: one ring beyond meshes for padded shells
    const genCandidates = [];
    const meshCandidates = [];

    for (let dz = -RD; dz <= RD; dz++) {
      for (let dx = -RD; dx <= RD; dx++) {
        const d2 = dx * dx + dz * dz;
        if (d2 > RD * RD + 1) continue;
        const cx = pcx + dx;
        const cz = pcz + dz;
        const chunk = this.chunks.get(chunkKey(cx, cz));
        if (!chunk) {
          genCandidates.push([d2, cx, cz]);
          continue;
        }
        if (!chunk.blocks || chunk.meshing) continue;
        const wantMesh = (!chunk.meshed && d2 <= R * R + 1) || (chunk.meshed && chunk.dirty);
        if (wantMesh && this.neighborsReady(cx, cz)) meshCandidates.push([d2, chunk]);
      }
    }

    genCandidates.sort((a, b) => a[0] - b[0]);
    for (const [, cx, cz] of genCandidates) {
      if (this.genInFlight >= this.maxGenJobs) break;
      this.requestGenerate(cx, cz);
    }

    meshCandidates.sort((a, b) => a[0] - b[0]);
    for (const [, chunk] of meshCandidates) {
      if (this.meshInFlight >= this.maxMeshJobs) break;
      this.requestMesh(chunk);
    }

    // Drop meshes far outside the view (block data is kept).
    const unloadR2 = (R + 2) * (R + 2);
    for (const chunk of this.chunks.values()) {
      if (!chunk.meshed) continue;
      const dx = chunk.cx - pcx;
      const dz = chunk.cz - pcz;
      if (dx * dx + dz * dz > unloadR2) {
        this.renderer.removeChunkMesh(chunkKey(chunk.cx, chunk.cz));
        chunk.meshed = false;
        this.events.emit('chunk:unloaded', { chunk });
      }
    }
  }

  neighborsReady(cx, cz) {
    for (const [dx, dz] of NEIGHBORS_8) {
      const n = this.chunks.get(chunkKey(cx + dx, cz + dz));
      if (!n || !n.blocks) return false;
    }
    return true;
  }

  requestGenerate(cx, cz) {
    const chunk = new Chunk(cx, cz);
    this.chunks.set(chunkKey(cx, cz), chunk);
    this.genInFlight++;
    this.pool.run({ type: 'generate', cx, cz }).then((res) => {
      this.genInFlight--;
      chunk.blocks = res.blocks;
      this.needsScan = true;
      this.events.emit('chunk:generated', { chunk });
    });
  }

  requestMesh(chunk) {
    chunk.meshing = true;
    chunk.dirty = false;
    const version = chunk.version;
    const padded = this.buildPadded(chunk.cx, chunk.cz);
    this.meshInFlight++;
    this.pool.run({ type: 'mesh', padded }, [padded.buffer]).then((res) => {
      this.meshInFlight--;
      chunk.meshing = false;
      chunk.meshed = true;
      this.renderer.setChunkMesh(chunkKey(chunk.cx, chunk.cz), chunk.cx, chunk.cz, res);
      if (chunk.version !== version) chunk.dirty = true; // edited while meshing
      this.needsScan = true;
      this.events.emit('chunk:meshed', { chunk });
    });
  }

  // Copy chunk blocks plus a 1-block shell from the 8 neighbours into a
  // padded array. Y-major layout keeps this a fast series of column copies.
  buildPadded(cx, cz) {
    const padded = new Uint16Array(PAD_XZ * PAD_XZ * PAD_Y);
    for (let pz = -1; pz <= CS; pz++) {
      const ncz = cz + (pz < 0 ? -1 : pz >= CS ? 1 : 0);
      const lz = (pz + CS) % CS;
      for (let px = -1; px <= CS; px++) {
        const ncx = cx + (px < 0 ? -1 : px >= CS ? 1 : 0);
        const lx = (px + CS) % CS;
        const src = this.chunks.get(chunkKey(ncx, ncz)).blocks;
        const srcBase = blockIndex(lx, 0, lz);
        const dstBase = paddedIndex(px, 0, pz);
        padded.set(src.subarray(srcBase, srcBase + H), dstBase);
        padded[dstBase - 1] = 1; // below the world: solid, culls bottom faces
        // above the world stays 0 (air)
      }
    }
    return padded;
  }

  getChunk(cx, cz) {
    return this.chunks.get(chunkKey(cx, cz));
  }

  getBlock(wx, wy, wz) {
    if (wy < 0 || wy >= H) return 0;
    const chunk = this.chunks.get(chunkKey(wx >> 4, wz >> 4));
    if (!chunk || !chunk.blocks) return 0;
    return chunk.blocks[blockIndex(wx & 15, wy, wz & 15)];
  }

  // Physics-safe solidity: unloaded terrain counts as solid.
  isSolidAt(wx, wy, wz) {
    if (wy < 0) return true;
    if (wy >= H) return false;
    const chunk = this.chunks.get(chunkKey(wx >> 4, wz >> 4));
    if (!chunk || !chunk.blocks) return true;
    return this.registry.tables.solid[chunk.blocks[blockIndex(wx & 15, wy, wz & 15)]] === 1;
  }

  setBlock(wx, wy, wz, id) {
    if (wy < 0 || wy >= H) return false;
    const cx = wx >> 4;
    const cz = wz >> 4;
    const chunk = this.chunks.get(chunkKey(cx, cz));
    if (!chunk || !chunk.blocks) return false;

    const lx = wx & 15;
    const lz = wz & 15;
    const idx = blockIndex(lx, wy, lz);
    const prev = chunk.blocks[idx];
    if (prev === id) return false;
    chunk.blocks[idx] = id;
    chunk.version++;
    chunk.dirty = true;

    // Border edits affect neighbouring chunks' culling/AO shells too.
    const dxs = lx === 0 ? [-1, 0] : lx === CS - 1 ? [0, 1] : [0];
    const dzs = lz === 0 ? [-1, 0] : lz === CS - 1 ? [0, 1] : [0];
    for (const dx of dxs) {
      for (const dz of dzs) {
        if (dx === 0 && dz === 0) continue;
        const n = this.chunks.get(chunkKey(cx + dx, cz + dz));
        if (n) {
          n.dirty = true;
          n.version++;
        }
      }
    }

    this.needsScan = true;
    this.events.emit('block:set', { x: wx, y: wy, z: wz, id, prev });
    return true;
  }

  // Topmost solid block in a column, or null if the chunk isn't generated.
  getSurfaceY(wx, wz) {
    const chunk = this.chunks.get(chunkKey(wx >> 4, wz >> 4));
    if (!chunk || !chunk.blocks) return null;
    const solid = this.registry.tables.solid;
    const base = blockIndex(wx & 15, 0, wz & 15);
    for (let y = H - 1; y >= 0; y--) {
      if (solid[chunk.blocks[base + y]] === 1) return y;
    }
    return null;
  }

  stats() {
    let generated = 0;
    let meshed = 0;
    for (const c of this.chunks.values()) {
      if (c.blocks) generated++;
      if (c.meshed) meshed++;
    }
    return {
      chunks: this.chunks.size,
      generated,
      meshed,
      genJobs: this.genInFlight,
      meshJobs: this.meshInFlight,
    };
  }
}
