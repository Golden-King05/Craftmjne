# Custom block models

Every block renders as a plain textured cube by default. Drop a
[Blockbench](https://www.blockbench.net/) file in this folder to give one
specific block a real shape instead — no export step, no conversion: save
your project in Blockbench and put the `.bbmodel` file straight in here.

## Status: scaffolding, not a working loader yet

**There is no model loader or renderer in the game yet.** This folder and
the wiring below exist so the pieces are in place — a block can already
*point at* a model file, and that reference round-trips through the block
registry — but nothing currently reads the `.bbmodel` file's contents.
A block set to use a custom model still renders as an ordinary cube in the
world, and falls back to the flat single-face icon (same as `item_model:
"face"`) in the hotbar/inventory, exactly like `src/blocks.rs`'s module
docs say. Building the actual `.bbmodel` parser and a non-cube mesh path
through `src/mesher.rs` is future work — this is the folder and naming
convention that work will read from when it lands, not a feature you can
see yet.

## Rules

- **Format: Blockbench's native project file (`.bbmodel`)** — the JSON
  format Blockbench itself saves to, not an exported Java/Bedrock model.
  Just hit Save in Blockbench with the file living in this folder (or
  save anywhere and copy it in) and it's ready.
- **Filename = the block's `id`, exactly** — same convention as
  `blocks/<id>.json` and `textures/blocks/<id>.png`. A torch's model is
  `models/torch.bbmodel`, matching `blocks/torch.json`.

## Wiring a block to its model

In the block's `blocks/<id>.json`, set `block_model` to the bare filename
(not a path) — mirroring how `textures` names are bare names relative to
`textures/blocks/`. `blocks/torch.json` is already wired up:

```json
{
  "id": "torch",
  "name": "Torch",
  "light": { "level": 16, "color": [255, 236, 208] },
  "block_model": "torch.bbmodel"
}
```

### `block_model` vs `item_model` — two different things

`block_model` is the block's shape **in the world**. It is deliberately
*not* the same field as `item_model`/`custom_item_model`, which only ever
control the **inventory/hotbar icon**.

Keeping them separate is what lets a torch have a real torch shape in the
world while still showing the standard baked isometric icon in your
hotbar. If one field drove both, giving the torch its shape would also
have silently downgraded its icon to a flat square — two unrelated
behaviors moving together because one block happened to want both.

## Where this folder needs to live

Same rule as `blocks/` and `textures/`: next to the game's `.exe` for an
installed build, or the repo root for `cargo run`/`cargo test` (Cargo runs
both with the package root as the working directory, so nothing extra to
set up for development).
