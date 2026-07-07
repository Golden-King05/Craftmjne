// DOM HUD: start overlay, crosshair, hotbar and F3 debug panel.

export class HUD {
  constructor(engine) {
    this.engine = engine;
    this.overlay = document.getElementById('overlay');
    this.hotbarEl = document.getElementById('hotbar');
    this.debugEl = document.getElementById('debug');
    this.debugVisible = false;
    this.debugTimer = 0;
    this.fps = 0;
    this.frames = 0;
    this.fpsTimer = 0;
    this.lastSelected = -1;
    this.slots = [];

    this.overlay.addEventListener('click', () => engine.input.lock());
    document.addEventListener('pointerlockchange', () => {
      this.overlay.classList.toggle('hidden', document.pointerLockElement != null);
    });

    this.buildHotbar();
  }

  buildHotbar() {
    const { interaction, atlas, blocks } = this.engine;
    this.hotbarEl.innerHTML = '';
    this.slots = [];
    interaction.hotbar.forEach((id, i) => {
      const slot = document.createElement('div');
      slot.className = 'slot';
      const num = document.createElement('span');
      num.className = 'num';
      num.textContent = String(i + 1);
      const icon = document.createElement('canvas');
      icon.width = 16;
      icon.height = 16;
      const def = blocks.def(id);
      const tex = blocks.resolveFaceTexture(def, 0); // east face as the icon
      atlas.drawTile(atlas.tileIndex(tex), icon.getContext('2d'), 0, 0, 16, 16);
      slot.append(num, icon);
      this.hotbarEl.appendChild(slot);
      this.slots.push({ slot, id });
    });
  }

  update(dt) {
    const { interaction, input } = this.engine;

    // Rebuild icons if block picking changed a slot.
    for (let i = 0; i < this.slots.length; i++) {
      if (this.slots[i].id !== interaction.hotbar[i]) {
        this.buildHotbar();
        this.lastSelected = -1;
        break;
      }
    }
    if (interaction.selected !== this.lastSelected) {
      this.lastSelected = interaction.selected;
      this.slots.forEach(({ slot }, i) => slot.classList.toggle('selected', i === interaction.selected));
    }

    if (input.justPressed('F3')) {
      this.debugVisible = !this.debugVisible;
      this.debugEl.classList.toggle('hidden', !this.debugVisible);
    }

    this.frames++;
    this.fpsTimer += dt;
    if (this.fpsTimer >= 0.5) {
      this.fps = Math.round(this.frames / this.fpsTimer);
      this.frames = 0;
      this.fpsTimer = 0;
    }

    if (!this.debugVisible) return;
    this.debugTimer -= dt;
    if (this.debugTimer > 0) return;
    this.debugTimer = 0.25;

    const { player, world, renderer } = this.engine;
    const p = player.pos;
    const w = world.stats();
    const r = renderer.info.render;
    this.debugEl.textContent =
      `fps      ${this.fps}\n` +
      `pos      ${p.x.toFixed(1)} ${p.y.toFixed(1)} ${p.z.toFixed(1)}\n` +
      `chunk    ${Math.floor(p.x / 16)} ${Math.floor(p.z / 16)}\n` +
      `mode     ${player.fly ? 'fly' : player.onGround ? 'ground' : 'air'}\n` +
      `chunks   ${w.meshed} meshed / ${w.generated} generated\n` +
      `jobs     gen ${w.genJobs}  mesh ${w.meshJobs}\n` +
      `draws    ${r.calls}  tris ${(r.triangles / 1000).toFixed(0)}k`;
  }
}
