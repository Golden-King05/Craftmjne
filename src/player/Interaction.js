// Block targeting, breaking, placing, block picking and hotbar selection.

import { WORLD_HEIGHT as H } from '../config.js';
import { raycastVoxel } from './raycast.js';
import { EYE_HEIGHT } from './Player.js';

const REACH = 6;
const ACTION_REPEAT = 0.22; // seconds between repeats while a button is held

const DIGIT_CODES = ['Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit5', 'Digit6', 'Digit7', 'Digit8', 'Digit9'];

export class Interaction {
  constructor(engine) {
    this.engine = engine;
    const b = engine.blocks;
    this.hotbar = ['grass', 'dirt', 'stone', 'cobblestone', 'planks', 'log', 'leaves', 'glass', 'bricks']
      .map((name) => b.id(name));
    this.selected = 0;
    this.breakTimer = 0;
    this.placeTimer = 0;
    this.target = null;
  }

  get selectedBlockId() {
    return this.hotbar[this.selected];
  }

  update(dt) {
    const { input, player, world, renderer, blocks } = this.engine;

    // Hotbar selection.
    for (let i = 0; i < DIGIT_CODES.length; i++) {
      if (input.justPressed(DIGIT_CODES[i])) this.selected = i;
    }
    const wheel = input.takeWheel();
    if (wheel !== 0) {
      this.selected = (this.selected + wheel + this.hotbar.length * 8) % this.hotbar.length;
    }

    // Crosshair target.
    const dir = player.getLookDir();
    const ox = player.pos.x;
    const oy = player.pos.y + EYE_HEIGHT;
    const oz = player.pos.z;
    const defs = blocks.defs;
    this.target = player.spawned
      ? raycastVoxel(ox, oy, oz, dir.x, dir.y, dir.z, REACH, (x, y, z) => {
          const id = world.getBlock(x, y, z);
          return id !== 0 && defs[id].selectable !== false;
        })
      : null;
    renderer.setHighlight(this.target);

    this.breakTimer -= dt;
    this.placeTimer -= dt;
    if (!input.buttonDown(0)) this.breakTimer = 0;
    if (!input.buttonDown(2)) this.placeTimer = 0;
    if (!input.locked || !this.target) return;

    // Break (left click / hold).
    if (input.buttonDown(0) && this.breakTimer <= 0) {
      const t = this.target;
      if (defs[world.getBlock(t.x, t.y, t.z)].breakable !== false) {
        world.setBlock(t.x, t.y, t.z, 0);
      }
      this.breakTimer = ACTION_REPEAT;
    }

    // Place (right click / hold).
    if (input.buttonDown(2) && this.placeTimer <= 0) {
      const t = this.target;
      const px = t.x + t.nx;
      const py = t.y + t.ny;
      const pz = t.z + t.nz;
      const id = this.selectedBlockId;
      if (py >= 0 && py < H) {
        const existing = world.getBlock(px, py, pz);
        const replaceable = existing === 0 || defs[existing].replaceable === true;
        const blocked = defs[id].solid && player.intersectsBlock(px, py, pz);
        if (replaceable && !blocked) {
          world.setBlock(px, py, pz, id);
        }
      }
      this.placeTimer = ACTION_REPEAT;
    }

    // Pick block (middle click): put the targeted block in the current slot.
    if (input.buttonJustPressed(1)) {
      const id = world.getBlock(this.target.x, this.target.y, this.target.z);
      if (id !== 0) {
        const existing = this.hotbar.indexOf(id);
        if (existing >= 0) this.selected = existing;
        else this.hotbar[this.selected] = id;
      }
    }
  }
}
