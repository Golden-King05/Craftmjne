// Procedural 16x16 pixel-art painters for the default block set.
// Every painter is deterministic (seeded rng), so the atlas is identical on
// every run. All art is generated — no external image assets needed.

import { TILE_SIZE } from '../config.js';

const S = TILE_SIZE;

function px(ctx, x0, y0, x, y, r, g, b, a = 1) {
  ctx.fillStyle = `rgba(${r | 0},${g | 0},${b | 0},${a})`;
  ctx.fillRect(x0 + x, y0 + y, 1, 1);
}

// Fills the tile with a base color, jittering each pixel's brightness.
function noisyFill(ctx, x0, y0, rng, [r, g, b], jitter, a = 1) {
  for (let y = 0; y < S; y++) {
    for (let x = 0; x < S; x++) {
      const j = (rng() - 0.5) * jitter;
      px(ctx, x0, y0, x, y, r + j, g + j, b + j, a);
    }
  }
}

export function registerDefaultPainters(atlas) {
  atlas.registerPainter('stone', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [127, 127, 127], 26);
  });

  atlas.registerPainter('dirt', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [134, 96, 67], 30);
    for (let i = 0; i < 10; i++) {
      px(ctx, x0, y0, (rng() * S) | 0, (rng() * S) | 0, 100, 70, 48);
    }
  });

  atlas.registerPainter('grass_top', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [104, 168, 62], 30);
    for (let i = 0; i < 14; i++) {
      px(ctx, x0, y0, (rng() * S) | 0, (rng() * S) | 0, 88, 148, 52);
    }
  });

  atlas.registerPainter('grass_side', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [134, 96, 67], 30);
    for (let x = 0; x < S; x++) {
      const depth = 2 + ((rng() * 2.4) | 0); // ragged grass fringe
      for (let y = 0; y < depth; y++) {
        const j = (rng() - 0.5) * 26;
        px(ctx, x0, y0, x, y, 104 + j, 168 + j, 62 + j);
      }
    }
  });

  atlas.registerPainter('sand', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [219, 207, 160], 18);
  });

  atlas.registerPainter('gravel', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [130, 124, 120], 20);
    for (let i = 0; i < 18; i++) {
      const x = (rng() * (S - 1)) | 0;
      const y = (rng() * (S - 1)) | 0;
      const c = 90 + rng() * 80;
      px(ctx, x0, y0, x, y, c, c * 0.96, c * 0.9);
      px(ctx, x0, y0, x + 1, y, c * 0.8, c * 0.78, c * 0.75);
    }
  });

  atlas.registerPainter('water', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [50, 108, 190], 16);
    for (let i = 0; i < 6; i++) {
      const y = (rng() * S) | 0;
      const x = (rng() * (S - 4)) | 0;
      for (let dx = 0; dx < 4; dx++) px(ctx, x0, y0, x + dx, y, 92, 148, 216);
    }
  });

  atlas.registerPainter('log_side', (ctx, x0, y0, rng) => {
    for (let x = 0; x < S; x++) {
      const stripe = x % 4 < 2;
      for (let y = 0; y < S; y++) {
        const j = (rng() - 0.5) * 18;
        const c = stripe ? [109, 85, 50] : [88, 66, 38];
        px(ctx, x0, y0, x, y, c[0] + j, c[1] + j, c[2] + j);
      }
    }
  });

  atlas.registerPainter('log_top', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [109, 85, 50], 14);
    for (let y = 0; y < S; y++) {
      for (let x = 0; x < S; x++) {
        const d = Math.max(Math.abs(x - 7.5), Math.abs(y - 7.5));
        if ((d | 0) % 2 === 0 && d < 7) {
          const j = (rng() - 0.5) * 12;
          px(ctx, x0, y0, x, y, 168 + j, 138 + j, 92 + j);
        }
      }
    }
  });

  atlas.registerPainter('leaves', (ctx, x0, y0, rng) => {
    ctx.clearRect(x0, y0, S, S);
    for (let y = 0; y < S; y++) {
      for (let x = 0; x < S; x++) {
        if (rng() < 0.14) continue; // see-through holes (alpha cutout)
        const j = (rng() - 0.5) * 44;
        px(ctx, x0, y0, x, y, 58 + j * 0.4, 128 + j, 44 + j * 0.4);
      }
    }
  });

  atlas.registerPainter('planks', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [176, 142, 88], 16);
    for (const y of [3, 7, 11, 15]) {
      for (let x = 0; x < S; x++) px(ctx, x0, y0, x, y, 122, 96, 56);
    }
    px(ctx, x0, y0, 4, 1, 122, 96, 56);
    px(ctx, x0, y0, 12, 5, 122, 96, 56);
    px(ctx, x0, y0, 2, 9, 122, 96, 56);
    px(ctx, x0, y0, 10, 13, 122, 96, 56);
  });

  atlas.registerPainter('cobblestone', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [110, 110, 110], 18);
    for (let i = 0; i < 7; i++) {
      const cx = 1 + ((rng() * (S - 5)) | 0);
      const cy = 1 + ((rng() * (S - 5)) | 0);
      const w = 3 + ((rng() * 3) | 0);
      const h = 3 + ((rng() * 3) | 0);
      const c = 118 + rng() * 34;
      for (let y = 0; y < h; y++) {
        for (let x = 0; x < w; x++) {
          const edge = x === 0 || y === 0 || x === w - 1 || y === h - 1;
          const v = edge ? 74 : c + (rng() - 0.5) * 14;
          px(ctx, x0, y0, Math.min(cx + x, S - 1), Math.min(cy + y, S - 1), v, v, v);
        }
      }
    }
  });

  atlas.registerPainter('glass', (ctx, x0, y0, rng) => {
    ctx.clearRect(x0, y0, S, S);
    for (let i = 0; i < S; i++) {
      px(ctx, x0, y0, i, 0, 208, 232, 238);
      px(ctx, x0, y0, i, S - 1, 208, 232, 238);
      px(ctx, x0, y0, 0, i, 208, 232, 238);
      px(ctx, x0, y0, S - 1, i, 208, 232, 238);
    }
    for (let i = 2; i < 7; i++) px(ctx, x0, y0, i, 8 - i, 226, 244, 248); // streak
    for (let i = 4; i < 12; i++) px(ctx, x0, y0, i, 16 - i, 226, 244, 248);
  });

  atlas.registerPainter('bedrock', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [70, 70, 70], 60);
  });

  atlas.registerPainter('snow', (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [241, 246, 250], 10);
  });

  atlas.registerPainter('bricks', (ctx, x0, y0, rng) => {
    for (let y = 0; y < S; y++) {
      const row = (y / 4) | 0;
      for (let x = 0; x < S; x++) {
        const mortarY = y % 4 === 3;
        const mortarX = (x + (row % 2 ? 4 : 0)) % 8 === 7;
        if (mortarY || (mortarX && !mortarY)) {
          px(ctx, x0, y0, x, y, 178, 170, 160);
        } else {
          const j = (rng() - 0.5) * 20;
          px(ctx, x0, y0, x, y, 150 + j, 72 + j * 0.5, 62 + j * 0.5);
        }
      }
    }
  });

  const orePainter = (spec) => (ctx, x0, y0, rng) => {
    noisyFill(ctx, x0, y0, rng, [127, 127, 127], 26);
    for (let i = 0; i < 5; i++) {
      const cx = 1 + ((rng() * (S - 4)) | 0);
      const cy = 1 + ((rng() * (S - 4)) | 0);
      for (const [dx, dy] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
        if (rng() < 0.85) {
          const j = (rng() - 0.5) * 20;
          px(ctx, x0, y0, cx + dx, cy + dy, spec[0] + j, spec[1] + j, spec[2] + j);
        }
      }
    }
  };
  atlas.registerPainter('coal_ore', orePainter([38, 38, 40]));
  atlas.registerPainter('iron_ore', orePainter([216, 175, 147]));
}
