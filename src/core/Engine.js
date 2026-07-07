// Engine: composition root and game loop.
//
// Framework usage:
//   const engine = new Engine({ seed: 42, renderDistance: 10 });
//   engine.blocks.register({ ... });          // custom blocks
//   engine.atlas.registerPainter('x', fn);    // custom 16x16 textures
//   engine.registerSystem({ update(dt) {} }); // custom game systems
//   engine.events.on('block:set', handler);   // react to game events
//   await engine.start(container);

import { DEFAULT_CONFIG } from '../config.js';
import { EventBus } from './EventBus.js';
import { Input } from './Input.js';
import { BlockRegistry, registerDefaultBlocks } from '../blocks/BlockRegistry.js';
import { TextureAtlas } from '../render/TextureAtlas.js';
import { registerDefaultPainters } from '../render/painters.js';
import { Renderer } from '../render/Renderer.js';
import { WorkerPool } from '../world/WorkerPool.js';
import { World } from '../world/World.js';
import { Player } from '../player/Player.js';
import { Interaction } from '../player/Interaction.js';
import { HUD } from '../ui/HUD.js';

export class Engine {
  constructor(config = {}) {
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.events = new EventBus();
    this.blocks = new BlockRegistry();
    this.atlas = new TextureAtlas();
    this.systems = [];
    this.running = false;
    this._lastTime = 0;

    registerDefaultBlocks(this.blocks);
    registerDefaultPainters(this.atlas);
  }

  registerSystem(system) {
    this.systems.push(system);
    return system;
  }

  async start(container) {
    // Bake content registries into flat tables.
    const nameToIndex = this.atlas.build();
    this.blocks.compile(nameToIndex);

    this.input = new Input(container);
    this.renderer = new Renderer(container, this.atlas, this.config);

    // Chunk workers get the compiled tables once, up front.
    this.pool = new WorkerPool(
      () => new Worker(new URL('../worker/chunkWorker.js', import.meta.url), { type: 'module' }),
    );
    this.pool.broadcast({
      type: 'init',
      seed: this.config.seed,
      ids: this.blocks.ids,
      tables: this.blocks.tables,
    });

    this.player = new Player(this);
    this.world = new World(this);
    this.interaction = new Interaction(this);
    this.hud = new HUD(this);

    // Default system order: input-driven systems first, then world streaming.
    this.systems.unshift(this.player, this.interaction, this.world, this.hud);

    this.running = true;
    this._lastTime = performance.now();
    const loop = (now) => {
      if (!this.running) return;
      const dt = Math.min((now - this._lastTime) / 1000, 0.1);
      this._lastTime = now;
      for (const system of this.systems) system.update(dt);
      this.events.emit('tick', dt);
      this.renderer.render();
      this.input.endFrame();
      requestAnimationFrame(loop);
    };
    requestAnimationFrame(loop);
    return this;
  }

  stop() {
    this.running = false;
    this.pool?.dispose();
  }
}
