// Texture atlas: packs 16x16 procedurally painted tiles into one canvas
// texture, so every chunk renders with a single material (minimal state
// changes, minimal draw-call overhead).
//
// Extension point: register a painter before engine.start():
//   engine.atlas.registerPainter('ruby', (ctx, x0, y0, rng) => { ... });
// A painter draws one TILE_SIZE x TILE_SIZE tile at (x0, y0). `rng` is a
// deterministic PRNG seeded from the texture name.

import * as THREE from 'three';
import { TILE_SIZE, ATLAS_TILES } from '../config.js';
import { mulberry32 } from '../gen/noise.js';

function hashString(str) {
  let h = 2166136261;
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export class TextureAtlas {
  constructor() {
    this.painters = new Map(); // name -> painter fn, atlas order = insertion order
    this.nameToIndex = null;
    this.canvas = null;
    this.texture = null;
  }

  registerPainter(name, fn) {
    if (this.nameToIndex) throw new Error('Cannot register painters after engine start');
    this.painters.set(name, fn);
  }

  build() {
    const size = TILE_SIZE * ATLAS_TILES;
    if (this.painters.size > ATLAS_TILES * ATLAS_TILES) {
      throw new Error(`Too many textures for a ${ATLAS_TILES}x${ATLAS_TILES} atlas`);
    }
    this.canvas = document.createElement('canvas');
    this.canvas.width = size;
    this.canvas.height = size;
    const ctx = this.canvas.getContext('2d');
    ctx.clearRect(0, 0, size, size);

    this.nameToIndex = new Map();
    let index = 0;
    for (const [name, painter] of this.painters) {
      const x0 = (index % ATLAS_TILES) * TILE_SIZE;
      const y0 = ((index / ATLAS_TILES) | 0) * TILE_SIZE;
      painter(ctx, x0, y0, mulberry32(hashString(name)));
      this.nameToIndex.set(name, index);
      index++;
    }

    this.texture = new THREE.CanvasTexture(this.canvas);
    this.texture.flipY = false; // mesher UVs measure V from the canvas top
    this.texture.magFilter = THREE.NearestFilter;
    this.texture.minFilter = THREE.NearestFilter;
    this.texture.generateMipmaps = false;
    this.texture.colorSpace = THREE.SRGBColorSpace;
    return this.nameToIndex;
  }

  tileIndex(name) {
    return this.nameToIndex?.get(name);
  }

  // Draws a tile into another 2d context (used by the hotbar icons).
  drawTile(index, ctx, dx, dy, dw, dh) {
    const sx = (index % ATLAS_TILES) * TILE_SIZE;
    const sy = ((index / ATLAS_TILES) | 0) * TILE_SIZE;
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(this.canvas, sx, sy, TILE_SIZE, TILE_SIZE, dx, dy, dw, dh);
  }
}
