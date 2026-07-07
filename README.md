# Craftmjne

A well-optimized 3D Minecraft-style voxel engine **desktop app**, built as a
**framework** you can expand. Electron + Three.js + Vite, with all block
textures procedurally generated as **16×16 pixel art** — no image assets
required.

## Quick start

```bash
npm install
npm run dev      # desktop app in dev mode (Vite HMR + Electron, F12 devtools)
npm run start    # build and run the desktop app in production mode
npm run dist     # package installers (AppImage / NSIS+portable / DMG) to release/
npm run smoke    # headless desktop smoke test (CI-friendly, use xvfb-run on Linux servers)
```

The engine is plain web tech underneath, so it also runs in a browser if you
ever want that: `npm run build && npm run preview` (supports `?seed=42` and
`?rd=10` URL parameters).

In the desktop app, world settings come from the `Engine` constructor call in
`src/main.js` (`seed`, `renderDistance`). Press **F11** for fullscreen.

### Controls

| Input | Action |
|---|---|
| W A S D | move |
| Mouse | look |
| Space | jump / swim up / fly up |
| Shift | fly down |
| Ctrl | sprint |
| F | toggle fly mode |
| Left / right click | break / place block |
| Middle click | pick targeted block |
| 1–9 / mouse wheel | hotbar selection |
| F3 | debug overlay (fps, chunks, draw calls, tris) |

## Performance design

The engine is built around keeping the main thread free for rendering:

- **Worker pool** — terrain generation *and* meshing run on a pool of web
  workers (`hardwareConcurrency − 1`), with least-busy dispatch and
  zero-copy transferable buffers in both directions.
- **Padded-shell meshing** — each mesh job receives the chunk plus a 1-block
  shell from its 8 neighbours, so face culling and ambient occlusion never do
  cross-chunk lookups and chunk borders are seamless.
- **Hidden-face culling** — only faces exposed to air/transparent blocks emit
  geometry; same-type transparent neighbours (water–water, glass–glass) are
  culled too.
- **Baked lighting** — directional sky shading + per-vertex ambient occlusion
  are baked into vertex colors by the mesher. Rendering uses unlit
  `MeshBasicMaterial`: no lights, no normals, no shadow passes. Quads flip
  along the brighter AO diagonal to avoid interpolation artifacts.
- **Two pipeline states total** — one texture atlas + one solid material
  (alpha-cutout handles leaves/glass) + one water material. Per chunk: at most
  two draw calls.
- **Typed arrays everywhere** — `Uint16Array` block storage (Y-major so column
  ops are contiguous `subarray` copies), flat `Uint8Array` lookup tables for
  block properties in every hot loop, `Uint16`/`Uint32` indices chosen per mesh.
- **Streaming with budgets** — chunks generate/mesh sorted by distance with
  capped in-flight jobs; far meshes are dropped (block data — including your
  edits — is kept). Precomputed bounding spheres let Three.js frustum-cull
  chunks without ever computing bounds.
- **Fixed-timestep physics** — 120 Hz substeps, framerate-independent, with
  swept axis-separated AABB collision against the voxel grid.

## Architecture

```
electron/
├── main.cjs                # desktop entry: window, shortcuts, dev/prod loading
├── serve.cjs               # serves dist/ over a secure app:// scheme
└── smoke.cjs               # headless desktop smoke test (npm run smoke)
src/
├── config.js               # chunk size, world height, atlas layout
├── main.js                 # entry point — create Engine, register content
├── core/
│   ├── Engine.js           # composition root, game loop, system registry
│   ├── EventBus.js         # engine-wide events
│   └── Input.js            # keyboard / mouse / pointer lock
├── blocks/
│   └── BlockRegistry.js    # block definitions -> compiled typed-array tables
├── gen/
│   ├── noise.js            # seeded simplex noise, fBm, integer hashes
│   └── TerrainGenerator.js # heightmap, biome surface, caves, ores, trees
├── mesh/
│   └── ChunkMesher.js      # culled + AO-baked chunk meshing (worker-side)
├── world/
│   ├── World.js            # chunk store, streaming, block get/set
│   └── WorkerPool.js       # promise-based worker pool
├── worker/
│   └── chunkWorker.js      # generation + meshing entry point
├── render/
│   ├── Renderer.js         # three.js scene, materials, chunk meshes, fog
│   ├── TextureAtlas.js     # packs 16x16 painted tiles into one texture
│   └── painters.js         # procedural 16x16 pixel-art painters
├── player/
│   ├── Player.js           # AABB physics, swimming, fly mode, camera
│   ├── Interaction.js      # break / place / pick, hotbar
│   └── raycast.js          # voxel DDA raycast
└── ui/
    └── HUD.js              # overlay, crosshair, hotbar, F3 debug panel
```

Data flow for a chunk: `World.scan` → worker `generate` → blocks arrive →
neighbours ready → `World.buildPadded` → worker `mesh` → transferable vertex
buffers → `Renderer.setChunkMesh`. Edits bump a chunk version, mark it (and
border neighbours) dirty, and the same pipeline remeshes them; stale results
are detected by version and re-queued.

## Extending the framework

Register content **before** `engine.start()` (see `src/main.js` for a full
example):

### Add a block with a custom 16×16 texture

```js
engine.atlas.registerPainter('ruby', (ctx, x0, y0, rng) => {
  for (let y = 0; y < 16; y++)
    for (let x = 0; x < 16; x++) {
      const j = (rng() - 0.5) * 60;
      ctx.fillStyle = `rgb(${200 + j | 0}, ${30 + j / 3 | 0}, ${60 + j / 3 | 0})`;
      ctx.fillRect(x0 + x, y0 + y, 1, 1);
    }
});
engine.blocks.register({ name: 'ruby_block', textures: { all: 'ruby' } });
```

Block definition flags (defaults shown):

```js
{
  name: 'my_block',
  solid: true,          // collides with entities
  transparent: false,   // doesn't occlude neighbour faces (glass, leaves)
  translucent: false,   // alpha-blended water pass
  selectable: true,     // crosshair can target it
  replaceable: false,   // placing overwrites it (like water)
  breakable: true,      // can be mined
  textures: 'name' | { all, top, bottom, side, east, west, south, north },
}
```

### Add a game system

```js
engine.registerSystem({
  update(dt) { /* runs every frame after the built-in systems */ },
});
```

### React to events

```js
engine.events.on('block:set', ({ x, y, z, id, prev }) => { ... });
// also: 'chunk:generated', 'chunk:meshed', 'chunk:unloaded',
//       'player:spawned', 'tick'
```

### Customize world generation

`src/gen/TerrainGenerator.js` runs inside the chunk workers. Extend or replace
it (and update the import in `src/worker/chunkWorker.js`). Generation is
deterministic per `(seed, chunkX, chunkZ)` with no cross-chunk dependencies,
so chunks can generate in any order on any worker — keep that property (trees
use a border margin for exactly this reason).

### Console tinkering

The engine is exposed as `window.craft`:

```js
craft.world.setBlock(0, 40, 0, craft.blocks.id('glass'));
craft.player.fly = true;
craft.world.stats();
```

## Roadmap ideas

Natural next steps the architecture is prepared for: greedy meshing as an
alternative mesher, block light propagation (extra vertex-color channel),
`IndexedDB` chunk persistence, entities/mobs as systems, multiplayer via a
shared `World` protocol, day/night cycle (fog + sky uniforms), and biome-driven
generation parameters.

## License

MIT — all textures are generated at runtime; no third-party assets.
