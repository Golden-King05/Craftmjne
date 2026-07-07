// Procedural terrain generator. Runs inside chunk workers.
// Deterministic per (seed, cx, cz) — no cross-chunk dependencies, so chunks
// can generate in any order on any worker. Trees keep a 2-block margin from
// chunk borders so features never spill across chunks.
//
// To customize world generation, extend or replace this class and update the
// import in src/worker/chunkWorker.js.

import { CHUNK_SIZE as CS, WORLD_HEIGHT as H, SEA_LEVEL, blockIndex } from '../config.js';
import { SimplexNoise, hash2, hash3 } from './noise.js';

const SNOW_LINE = 45;

export class TerrainGenerator {
  constructor(seed, ids) {
    this.seed = seed | 0;
    this.ids = ids; // block name -> id map from the registry
    this.terrainNoise = new SimplexNoise(seed);
    this.mountainNoise = new SimplexNoise(seed ^ 0x9e3779b9);
    this.caveNoiseA = new SimplexNoise(seed ^ 0x85ebca6b);
    this.caveNoiseB = new SimplexNoise(seed ^ 0xc2b2ae35);
  }

  surfaceHeight(wx, wz) {
    // Low-frequency mask blends flat plains into mountains.
    const m = this.mountainNoise.fbm2(wx * 0.0035, wz * 0.0035, 3) * 0.5 + 0.5;
    const mountain = m * m;
    const detail = this.terrainNoise.fbm2(wx * 0.011, wz * 0.011, 4);
    const h = 27 + detail * (5 + mountain * 24) + mountain * 10;
    return Math.max(2, Math.min(H - 8, Math.floor(h)));
  }

  generate(cx, cz) {
    const { ids, seed } = this;
    const blocks = new Uint16Array(CS * CS * H);
    const heights = new Int16Array(CS * CS);
    const surface = new Uint16Array(CS * CS); // surface block per column

    const STONE = ids.stone, DIRT = ids.dirt, GRASS = ids.grass, SAND = ids.sand,
      GRAVEL = ids.gravel, WATER = ids.water, BEDROCK = ids.bedrock, SNOW = ids.snow,
      LOG = ids.log, LEAVES = ids.leaves, COAL = ids.coal_ore, IRON = ids.iron_ore;

    for (let z = 0; z < CS; z++) {
      for (let x = 0; x < CS; x++) {
        const wx = cx * CS + x;
        const wz = cz * CS + z;
        const h = this.surfaceHeight(wx, wz);
        heights[x + CS * z] = h;

        const beach = h <= SEA_LEVEL + 1;
        const snowy = h >= SNOW_LINE;
        const topId = beach ? SAND : snowy ? SNOW : GRASS;
        const fillId = beach ? SAND : DIRT;
        surface[x + CS * z] = topId;

        const base = blockIndex(x, 0, z);
        blocks[base] = BEDROCK;
        for (let y = 1; y <= h; y++) {
          blocks[base + y] = y === h ? topId : y >= h - 3 ? fillId : STONE;
        }
        // Flood water up to sea level.
        for (let y = h + 1; y <= SEA_LEVEL; y++) {
          blocks[base + y] = WATER;
        }
        // Gravel patches on the sea floor.
        if (h < SEA_LEVEL && hash2(wx, wz, seed ^ 0x1234) < 0.3) {
          blocks[base + h] = GRAVEL;
        }

        // Carve "spaghetti" caves on land columns (kept away from water so we
        // don't punch holes into the sea floor).
        if (h > SEA_LEVEL + 1) {
          for (let y = 4; y < h - 2; y++) {
            const a = this.caveNoiseA.noise3(wx * 0.045, y * 0.075, wz * 0.045);
            if (a > 0.09 || a < -0.09) continue;
            const b = this.caveNoiseB.noise3(wx * 0.045, y * 0.075, wz * 0.045);
            if (b > -0.09 && b < 0.09) blocks[base + y] = 0;
          }
        }

        // Ore veins.
        for (let y = 2; y < Math.min(h - 3, 40); y++) {
          if (blocks[base + y] !== STONE) continue;
          const r = hash3(wx, y, wz, seed ^ 0xabcd);
          if (r < 0.006 && y < 28) blocks[base + y] = IRON;
          else if (r < 0.018) blocks[base + y] = COAL;
        }
      }
    }

    // Trees (second pass; margin keeps canopies inside this chunk).
    for (let z = 2; z < CS - 2; z++) {
      for (let x = 2; x < CS - 2; x++) {
        const col = x + CS * z;
        if (surface[col] !== GRASS) continue;
        const wx = cx * CS + x;
        const wz = cz * CS + z;
        const r = hash2(wx, wz, seed ^ 0x51f3);
        if (r >= 0.012) continue;

        const h = heights[x + CS * z];
        const trunkH = 4 + ((r * 1000) | 0) % 3;
        if (h + trunkH + 2 >= H) continue;
        const base = blockIndex(x, 0, z);
        if (blocks[base + h] !== GRASS) continue; // surface was carved away

        blocks[base + h] = DIRT;
        for (let t = 1; t <= trunkH; t++) blocks[base + h + t] = LOG;

        // Canopy: two 5x5-ish layers, a 3x3 layer, and a plus-shaped cap.
        for (let ly = trunkH - 2; ly <= trunkH + 1; ly++) {
          const radius = ly <= trunkH - 1 ? 2 : 1;
          for (let dz = -radius; dz <= radius; dz++) {
            for (let dx = -radius; dx <= radius; dx++) {
              if (Math.abs(dx) === radius && Math.abs(dz) === radius && radius > 0) {
                if (radius === 2 || ly === trunkH + 1) continue; // clip corners
              }
              const idx = blockIndex(x + dx, h + ly, z + dz);
              if (blocks[idx] === 0) blocks[idx] = LEAVES;
            }
          }
        }
        blocks[base + h + trunkH + 1] = LEAVES;
      }
    }

    return blocks;
  }
}
