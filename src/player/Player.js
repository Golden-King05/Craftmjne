// First-person player: AABB physics against the voxel grid, swimming,
// fly mode, and fixed-timestep integration (120 Hz substeps) so behaviour is
// framerate-independent.

import { WORLD_HEIGHT as H, SEA_LEVEL } from '../config.js';

const HALF_W = 0.3;
const HEIGHT = 1.8;
export const EYE_HEIGHT = 1.62;
const EPS = 1e-3;

const GRAVITY = -30;
const JUMP_SPEED = 8.8;
const WALK_SPEED = 5.2;
const SPRINT_MULT = 1.6;
const FLY_SPEED = 16;
const STEP = 1 / 120;

export class Player {
  constructor(engine) {
    this.engine = engine;
    this.pos = { x: 8.5, y: H + 4, z: 8.5 };
    this.vel = { x: 0, y: 0, z: 0 };
    this.yaw = Math.PI * 0.75;
    this.pitch = -0.25;
    this.onGround = false;
    this.fly = false;
    this.spawned = false;
    this.accumulator = 0;
    this.waterId = engine.blocks.id('water');
  }

  update(dt) {
    const { input, world } = this.engine;

    const [mdx, mdy] = input.consumeMouseDelta();
    this.yaw -= mdx * 0.0022;
    this.pitch = Math.max(-1.553, Math.min(1.553, this.pitch - mdy * 0.0022));

    if (input.justPressed('KeyF')) {
      this.fly = !this.fly;
      this.vel.y = 0;
    }

    if (!this.spawned) {
      this.trySpawn(world);
      this.syncCamera();
      return;
    }

    this.accumulator = Math.min(this.accumulator + dt, 0.25);
    while (this.accumulator >= STEP) {
      this.accumulator -= STEP;
      this.step(STEP, input, world);
    }
    this.syncCamera();
  }

  // Wait for terrain, then drop the player on a dry column near the origin.
  trySpawn(world) {
    let best = null;
    for (let r = 0; r <= 24; r += 4) {
      for (let dz = -r; dz <= r; dz += 4) {
        for (let dx = -r; dx <= r; dx += 4) {
          const y = world.getSurfaceY(8 + dx, 8 + dz);
          if (y === null || y <= SEA_LEVEL) continue;
          best = { x: 8 + dx + 0.5, y: y + 1 + EPS, z: 8 + dz + 0.5 };
          break;
        }
        if (best) break;
      }
      if (best) break;
    }
    if (!best) return;
    this.pos = { ...best };
    this.vel = { x: 0, y: 0, z: 0 };
    this.spawned = true;
    this.engine.events.emit('player:spawned', { player: this });
  }

  getLookDir() {
    const cp = Math.cos(this.pitch);
    return {
      x: -Math.sin(this.yaw) * cp,
      y: Math.sin(this.pitch),
      z: -Math.cos(this.yaw) * cp,
    };
  }

  isInWater(world) {
    const { x, y, z } = this.pos;
    return world.getBlock(Math.floor(x), Math.floor(y + 0.6), Math.floor(z)) === this.waterId;
  }

  step(dt, input, world) {
    // Wish direction from WASD, rotated by yaw.
    const f = (input.isDown('KeyW') ? 1 : 0) - (input.isDown('KeyS') ? 1 : 0);
    const r = (input.isDown('KeyD') ? 1 : 0) - (input.isDown('KeyA') ? 1 : 0);
    const sy = Math.sin(this.yaw);
    const cy = Math.cos(this.yaw);
    let wx = -sy * f + cy * r;
    let wz = -cy * f - sy * r;
    const wl = Math.hypot(wx, wz);
    if (wl > 0) {
      wx /= wl;
      wz /= wl;
    }
    const sprint = input.isDown('ControlLeft') || input.isDown('ControlRight');

    if (this.fly) {
      const speed = FLY_SPEED * (sprint ? 2.5 : 1);
      this.vel.x = wx * speed;
      this.vel.z = wz * speed;
      this.vel.y = (input.isDown('Space') ? speed : 0) - (input.isDown('ShiftLeft') ? speed : 0);
    } else {
      const inWater = this.isInWater(world);
      const speed = WALK_SPEED * (sprint ? SPRINT_MULT : 1) * (inWater ? 0.55 : 1);
      const control = this.onGround || inWater ? 20 : 5;
      const blend = Math.min(1, control * dt);
      this.vel.x += (wx * speed - this.vel.x) * blend;
      this.vel.z += (wz * speed - this.vel.z) * blend;

      if (inWater) {
        this.vel.y += GRAVITY * 0.3 * dt;
        if (input.isDown('Space')) this.vel.y += (4.5 - this.vel.y) * Math.min(1, 12 * dt);
        this.vel.y = Math.max(this.vel.y, -4);
      } else {
        this.vel.y = Math.max(this.vel.y + GRAVITY * dt, -50);
        if (input.isDown('Space') && this.onGround) this.vel.y = JUMP_SPEED;
      }
    }

    this.onGround = false;
    this.moveAxis(world, 0, this.vel.x * dt);
    this.moveAxis(world, 2, this.vel.z * dt);
    this.moveAxis(world, 1, this.vel.y * dt);
  }

  // Axis-separated AABB sweep: move, then clamp against overlapping voxels.
  moveAxis(world, axis, delta) {
    if (delta === 0) return;
    const p = this.pos;
    const comp = axis === 0 ? 'x' : axis === 1 ? 'y' : 'z';
    p[comp] += delta;

    const minX = Math.floor(p.x - HALF_W);
    const maxX = Math.floor(p.x + HALF_W);
    const minY = Math.floor(p.y);
    const maxY = Math.floor(p.y + HEIGHT);
    const minZ = Math.floor(p.z - HALF_W);
    const maxZ = Math.floor(p.z + HALF_W);

    let bound = delta > 0 ? Infinity : -Infinity;
    for (let y = minY; y <= maxY; y++) {
      for (let z = minZ; z <= maxZ; z++) {
        for (let x = minX; x <= maxX; x++) {
          if (!world.isSolidAt(x, y, z)) continue;
          const v = axis === 0 ? x : axis === 1 ? y : z;
          bound = delta > 0 ? Math.min(bound, v) : Math.max(bound, v + 1);
        }
      }
    }
    if (!Number.isFinite(bound)) return;

    if (axis === 1) {
      if (delta > 0) {
        p.y = bound - HEIGHT - EPS;
      } else {
        p.y = bound + EPS;
        this.onGround = true;
      }
    } else if (axis === 0) {
      p.x = delta > 0 ? bound - HALF_W - EPS : bound + HALF_W + EPS;
    } else {
      p.z = delta > 0 ? bound - HALF_W - EPS : bound + HALF_W + EPS;
    }
    this.vel[comp] = 0;
  }

  intersectsBlock(x, y, z) {
    const p = this.pos;
    return (
      x + 1 > p.x - HALF_W && x < p.x + HALF_W &&
      y + 1 > p.y && y < p.y + HEIGHT &&
      z + 1 > p.z - HALF_W && z < p.z + HALF_W
    );
  }

  syncCamera() {
    const cam = this.engine.renderer.camera;
    cam.position.set(this.pos.x, this.pos.y + EYE_HEIGHT, this.pos.z);
    cam.rotation.set(this.pitch, this.yaw, 0);
  }
}
