// Chunk mesher — turns raw block data into render-ready vertex buffers.
// Runs inside chunk workers (no DOM / three.js imports here).
//
// Optimizations:
//  - Operates on a "padded" copy of the chunk (+1 block shell from the eight
//    neighbouring chunks) so face culling and AO never need chunk lookups.
//  - Hidden-face culling: only faces exposed to air / transparent blocks emit.
//  - Per-vertex ambient occlusion and directional sky shading are baked into
//    vertex colors, so rendering needs no runtime lights and no normals.
//  - Quads are flipped along the brighter AO diagonal to avoid interpolation
//    artifacts.
//  - Two buckets per chunk: 'solid' (opaque + alpha-cutout like leaves/glass)
//    and 'water' (alpha-blended translucent pass).
//
// Output typed arrays are transferable, so results move zero-copy back to the
// main thread.

import { CHUNK_SIZE as CS, WORLD_HEIGHT as H, ATLAS_TILES, TILE_SIZE } from '../config.js';

// Padded array layout: x,z in [-1, CS], y in [-1, H].
export const PAD_XZ = CS + 2;
export const PAD_Y = H + 2;

const SY = 1;
const SX = PAD_Y;
const SZ = PAD_Y * PAD_XZ;
const STRIDES = [SX, SY, SZ];

export function paddedIndex(x, y, z) {
  return (y + 1) * SY + (x + 1) * SX + (z + 1) * SZ;
}

// AO brightness for 0..3 occluders touching a vertex.
const AO_BRIGHT = [1.0, 0.82, 0.64, 0.46];

// Face order: 0:+x 1:-x 2:+y 3:-y 4:+z 5:-z (matches BlockRegistry tiles).
// Corner positions / uvs use the well-tested layout from the three.js voxel
// guide; triangles are (0,1,2)(2,1,3), or (0,1,3)(0,3,2) when AO-flipped.
const FACE_DEFS = [
  { dir: [1, 0, 0], shade: 0.6, corners: [
    { pos: [1, 1, 1], uv: [0, 1] }, { pos: [1, 0, 1], uv: [0, 0] },
    { pos: [1, 1, 0], uv: [1, 1] }, { pos: [1, 0, 0], uv: [1, 0] } ] },
  { dir: [-1, 0, 0], shade: 0.6, corners: [
    { pos: [0, 1, 0], uv: [0, 1] }, { pos: [0, 0, 0], uv: [0, 0] },
    { pos: [0, 1, 1], uv: [1, 1] }, { pos: [0, 0, 1], uv: [1, 0] } ] },
  { dir: [0, 1, 0], shade: 1.0, corners: [
    { pos: [0, 1, 1], uv: [1, 1] }, { pos: [1, 1, 1], uv: [0, 1] },
    { pos: [0, 1, 0], uv: [1, 0] }, { pos: [1, 1, 0], uv: [0, 0] } ] },
  { dir: [0, -1, 0], shade: 0.5, corners: [
    { pos: [1, 0, 1], uv: [1, 0] }, { pos: [0, 0, 1], uv: [0, 0] },
    { pos: [1, 0, 0], uv: [1, 1] }, { pos: [0, 0, 0], uv: [0, 1] } ] },
  { dir: [0, 0, 1], shade: 0.8, corners: [
    { pos: [0, 0, 1], uv: [0, 0] }, { pos: [1, 0, 1], uv: [1, 0] },
    { pos: [0, 1, 1], uv: [0, 1] }, { pos: [1, 1, 1], uv: [1, 1] } ] },
  { dir: [0, 0, -1], shade: 0.8, corners: [
    { pos: [1, 0, 0], uv: [0, 0] }, { pos: [0, 0, 0], uv: [1, 0] },
    { pos: [1, 1, 0], uv: [0, 1] }, { pos: [0, 1, 0], uv: [1, 1] } ] },
];

// Precompile faces: neighbour offset + per-corner AO sample offsets in padded
// index space, so the hot loop is pure integer adds.
const FACES = FACE_DEFS.map((f) => {
  const [dx, dy, dz] = f.dir;
  const normalAxis = dx !== 0 ? 0 : dy !== 0 ? 1 : 2;
  const neighborOfs = dx * SX + dy * SY + dz * SZ;
  const [u, v] = [0, 1, 2].filter((a) => a !== normalAxis);
  const corners = f.corners.map((c) => {
    const du = c.pos[u] ? 1 : -1;
    const dv = c.pos[v] ? 1 : -1;
    const side1 = neighborOfs + du * STRIDES[u];
    const side2 = neighborOfs + dv * STRIDES[v];
    return { pos: c.pos, uv: c.uv, ao: [side1, side2, side1 + dv * STRIDES[v]] };
  });
  return { neighborOfs, shade: f.shade, corners };
});

const UV_TILE = 1 / ATLAS_TILES;
// Half-texel inset prevents atlas bleeding at tile borders.
const UV_PAD = 0.5 / (ATLAS_TILES * TILE_SIZE);
const UV_SPAN = UV_TILE - 2 * UV_PAD;

const WATER_SURFACE = 0.875; // water tops sit slightly below the block top

function makeBucket() {
  return { positions: [], uvs: [], colors: [], indices: [], vcount: 0 };
}

function finalize(b) {
  if (b.vcount === 0) return null;
  return {
    positions: new Float32Array(b.positions),
    uvs: new Float32Array(b.uvs),
    colors: new Uint8Array(b.colors),
    indices: b.vcount <= 65535 ? new Uint16Array(b.indices) : new Uint32Array(b.indices),
  };
}

/**
 * @param {Uint16Array} padded  chunk blocks with a 1-block neighbour shell
 * @param {object} tables       compiled BlockRegistry tables
 * @returns {{ solid: object|null, water: object|null }}
 */
export function meshChunk(padded, tables) {
  const { opaque, translucent, tiles } = tables;
  const solid = makeBucket();
  const water = makeBucket();
  const aoVals = [1, 1, 1, 1];

  for (let z = 0; z < CS; z++) {
    for (let x = 0; x < CS; x++) {
      let idx = paddedIndex(x, 0, z);
      for (let y = 0; y < H; y++, idx++) {
        const id = padded[idx];
        if (id === 0) continue;

        const isWater = translucent[id] === 1;
        const bucket = isWater ? water : solid;
        const cap = isWater && padded[idx + SY] !== id ? WATER_SURFACE : 1;

        for (let f = 0; f < 6; f++) {
          const face = FACES[f];
          const nid = padded[idx + face.neighborOfs];
          // A face is hidden behind opaque blocks and behind its own kind.
          if (nid !== 0 && (opaque[nid] === 1 || nid === id)) continue;

          const tile = tiles[id * 6 + f];
          const tu = (tile % ATLAS_TILES) * UV_TILE;
          const tv = ((tile / ATLAS_TILES) | 0) * UV_TILE;
          const vi = bucket.vcount;

          for (let ci = 0; ci < 4; ci++) {
            const c = face.corners[ci];
            let bright = 1;
            if (!isWater) {
              const s1 = opaque[padded[idx + c.ao[0]]];
              const s2 = opaque[padded[idx + c.ao[1]]];
              const occ = s1 && s2 ? 3 : s1 + s2 + opaque[padded[idx + c.ao[2]]];
              bright = AO_BRIGHT[occ];
            }
            aoVals[ci] = bright;

            bucket.positions.push(
              x + c.pos[0],
              y + (c.pos[1] ? cap : 0),
              z + c.pos[2],
            );
            bucket.uvs.push(
              tu + UV_PAD + c.uv[0] * UV_SPAN,
              tv + UV_PAD + (1 - c.uv[1]) * UV_SPAN,
            );
            const light = Math.round(face.shade * bright * 255);
            bucket.colors.push(light, light, light);
          }

          if (aoVals[0] + aoVals[3] > aoVals[1] + aoVals[2]) {
            bucket.indices.push(vi, vi + 1, vi + 3, vi, vi + 3, vi + 2);
          } else {
            bucket.indices.push(vi, vi + 1, vi + 2, vi + 2, vi + 1, vi + 3);
          }
          bucket.vcount += 4;
        }
      }
    }
  }

  return { solid: finalize(solid), water: finalize(water) };
}
