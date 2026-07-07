import { Engine } from './core/Engine.js';

const params = new URLSearchParams(location.search);
const engine = new Engine({
  seed: params.has('seed') ? Number(params.get('seed')) : 1337,
  renderDistance: params.has('rd') ? Number(params.get('rd')) : 8,
});

// ---------------------------------------------------------------------------
// Framework extension example — register content BEFORE engine.start().
// Uncomment to add a glowing ruby block to the game (press middle-click on it
// once placed via `craft.world.setBlock(...)`, or add it to the hotbar in
// src/player/Interaction.js):
//
// engine.atlas.registerPainter('ruby', (ctx, x0, y0, rng) => {
//   for (let y = 0; y < 16; y++)
//     for (let x = 0; x < 16; x++) {
//       const j = (rng() - 0.5) * 60;
//       ctx.fillStyle = `rgb(${200 + j | 0}, ${30 + j / 3 | 0}, ${60 + j / 3 | 0})`;
//       ctx.fillRect(x0 + x, y0 + y, 1, 1);
//     }
// });
// engine.blocks.register({ name: 'ruby_block', textures: { all: 'ruby' } });
//
// engine.events.on('block:set', ({ x, y, z, id }) => console.log('set', x, y, z, id));
// ---------------------------------------------------------------------------

engine.start(document.getElementById('app'));

// Console access for tinkering: `craft.world.setBlock(0, 40, 0, 1)` etc.
window.craft = engine;
