// Block registry — the central extension point for adding content.
//
//   engine.blocks.register({
//     name: 'ruby_block',
//     textures: { all: 'ruby' },   // or { top, bottom, side } or per-face
//   });
//
// Block definition flags (all optional, defaults shown):
//   solid: true         collides with entities
//   transparent: false  does not occlude neighbouring faces (glass, leaves, water)
//   translucent: false  rendered in the alpha-blended pass (water)
//   selectable: true    can be targeted by the crosshair raycast
//   replaceable: false  placing a block into this cell overwrites it (water)
//   breakable: true     can be mined (bedrock is not)
//   textures: name | { all?, top?, bottom?, side?, east?, west?, south?, north? }
//             defaults to the block name itself.
//
// After `compile()`, flat typed-array lookup tables are available in
// `registry.tables` — these are what the mesher and physics use (and what is
// posted to the chunk workers), so hot loops never touch the def objects.

// Face order used across the whole engine (mesher, tiles table):
// 0:+x east, 1:-x west, 2:+y top, 3:-y bottom, 4:+z south, 5:-z north
const FACE_KEYS = ['east', 'west', 'top', 'bottom', 'south', 'north'];

export class BlockRegistry {
  constructor() {
    this.defs = [{ name: 'air', solid: false, transparent: true, selectable: false, replaceable: true }];
    this.byName = new Map([['air', 0]]);
    this.tables = null;
  }

  register(def) {
    if (!def.name) throw new Error('Block def requires a name');
    if (this.byName.has(def.name)) throw new Error(`Block "${def.name}" already registered`);
    if (this.tables) throw new Error('Cannot register blocks after engine start');
    const id = this.defs.length;
    this.defs.push({ solid: true, transparent: false, translucent: false, ...def });
    this.byName.set(def.name, id);
    return id;
  }

  id(name) {
    const id = this.byName.get(name);
    if (id === undefined) throw new Error(`Unknown block "${name}"`);
    return id;
  }

  def(id) {
    return this.defs[id];
  }

  resolveFaceTexture(def, face) {
    const t = def.textures;
    if (!t) return def.name;
    if (typeof t === 'string') return t;
    const key = FACE_KEYS[face];
    if (t[key]) return t[key];
    if (face === 2) return t.top ?? t.all ?? def.name;
    if (face === 3) return t.bottom ?? t.all ?? def.name;
    return t.side ?? t.all ?? def.name;
  }

  // Bakes flat lookup tables. `nameToIndex` maps texture names -> atlas tiles.
  compile(nameToIndex) {
    const n = this.defs.length;
    const solid = new Uint8Array(n);
    const opaque = new Uint8Array(n);
    const translucent = new Uint8Array(n);
    const tiles = new Int16Array(n * 6).fill(-1);

    for (let id = 1; id < n; id++) {
      const def = this.defs[id];
      solid[id] = def.solid ? 1 : 0;
      opaque[id] = def.transparent || def.translucent ? 0 : 1;
      translucent[id] = def.translucent ? 1 : 0;
      for (let f = 0; f < 6; f++) {
        const tex = this.resolveFaceTexture(def, f);
        const tile = nameToIndex.get(tex);
        if (tile === undefined) {
          throw new Error(`Block "${def.name}": no texture painter registered for "${tex}"`);
        }
        tiles[id * 6 + f] = tile;
      }
    }

    this.tables = { solid, opaque, translucent, tiles };
    this.ids = Object.fromEntries(this.byName);
    return this.tables;
  }
}

// The default block set. IDs are assigned in registration order (air is 0).
export function registerDefaultBlocks(registry) {
  registry.register({ name: 'stone' });
  registry.register({ name: 'dirt' });
  registry.register({ name: 'grass', textures: { top: 'grass_top', bottom: 'dirt', side: 'grass_side' } });
  registry.register({ name: 'sand' });
  registry.register({ name: 'gravel' });
  registry.register({
    name: 'water',
    solid: false,
    transparent: true,
    translucent: true,
    selectable: false,
    replaceable: true,
  });
  registry.register({ name: 'log', textures: { top: 'log_top', bottom: 'log_top', side: 'log_side' } });
  registry.register({ name: 'leaves', transparent: true });
  registry.register({ name: 'planks' });
  registry.register({ name: 'cobblestone' });
  registry.register({ name: 'glass', transparent: true });
  registry.register({ name: 'bedrock', breakable: false });
  registry.register({ name: 'snow' });
  registry.register({ name: 'bricks' });
  registry.register({ name: 'coal_ore' });
  registry.register({ name: 'iron_ore' });
}
