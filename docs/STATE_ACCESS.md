# Who touches a cell's state word

Every cell has one 16-bit **state word** (`Chunk::state`, see `src/states.rs`).
Which bits mean what depends on *which block is in the cell*:

- Built-in state tags sit at fixed positions, on the blocks that enable them
  (`level` = bits 0-7, `axis` = bits 8-9).
- A block's own `state_defs` fill the bits its built-in tags leave free, so a
  block with none can use all 16. Two blocks never compete: a cell holds one.

That only works if every piece of code that reads or writes those bits first
asks "does *this* block actually use them?". This file is the list of that
code - the "janitors". When modding grows (more states, extended storage, mods
declaring layouts), this is the list to walk, and the list the compiled
per-block layouts (`BlockRegistry::finalize_states`, built at startup from every
registered block, mods included) must keep every janitor consistent with.

A test (`states::layout_tests::the_janitor_list_covers_every_file_that_touches_the_word`)
fails if a file starts touching the word and isn't named on the next line. Add the
file *and* describe the new janitor below.

<!-- janitor-files: src/interact.rs src/states.rs src/world.rs -->

## The rule

> A janitor that reads or writes a vanilla field (`level`, `axis`) must be gated
> on the block id (or have just *written* the block itself). A janitor that
> changes a cell's block must clear the word.

## The janitors

### `src/world.rs` (the owner of the storage)

| janitor | does | gated by |
|---|---|---|
| `ChunkMap::set_block` / `write_block` | **clears the whole word** when the block changes | n/a - it is the clearing rule |
| `ChunkMap::set_block_keep_level` | clears the word, then restores only the fluid level | used where the *fluid* carries on (freezing, mining a waterlogged block, placing one into water) |
| `set_fluid_level_raw`, `set_axis_raw`, `set_state_word_raw` | raw field/word setters | callers (below) |
| `get_fluid_level`, `get_axis`, `state_word` | raw readers | callers (below) |
| `set_fluid_cell` | the fluid sim writes id + level together | called only for fluid ids |
| `recompute_cell` + `fluid_rank` + the lateral/falling checks | read levels | `is_fluid_here` / the neighbour's fluid id |
| `build_padded` | unpacks level and axis for **every** cell for the mesher | the mesher gates: axis by `tables.rotates[id]`, level by fluid / level-shaped id |
| `chunk_fluid_cells` | the fluid save scan reads each cell's level | `Tables::holds_level(id, level)` |
| `record_edits` | reads the cell's **whole word** into `EditLog` | runs on every `BlockSetEvent`, so any state change must send one |
| `enter_world` / `collect_gen_tasks` | rebuild `EditLog`/`PendingEdits` from the save; install a generated chunk (`pack_state`); reapply saved edits (`set_state_word_raw`) and saved fluid (`set_fluid_level_raw`) | the saved edit's block; the saved fluid cell's block |
| `try_freeze_cell` | reads the level, then `set_block_keep_level` | fluid id |
| `unload_far_chunks` | puts a chunk's `EditLog` entries back into `PendingEdits` | n/a |

### `src/interact.rs`

| janitor | does | gated by |
|---|---|---|
| break / mine (Creative and Survival) | `set_block_keep_level(after)` - leaves any fluid its block held | `left_behind` |
| place | `set_block_keep_level` for a waterlogged placement, else `set_block` (clears) | `waterlogged.is_some()` |
| place a rotating block | `set_axis_raw` | `rotation != None` |

### `src/states.rs` (the generic interface)

| janitor | does | gated by |
|---|---|---|
| `STATE_TAGS` rows (`axis_*`, `level_*`, `layer_*`) | read/write the built-in fields | `BlockDef::states` (`enabled`) |
| `set_state` (a block's own state) | writes one field of the word | the block's `state_fields` |
| `allocate_fields` | decides where a block's own states live | n/a - it *is* the layout |

### Anything that only calls `set_block`

`set_block` clears the word, so these are safe without further care: shattering
glass (`items.rs`), `/blockstate`'s `waterlogged` swap, and any future system
that replaces a block. They are not listed above because they never touch a
field themselves.

## Not done yet (and where it would plug in)

- **Extended storage** for a block that needs more than 16 bits: a sparse
  per-chunk table keyed by cell, for only the blocks that ask for it. It would
  add a janitor of its own (clear on `set_block`, save and load) and a new row
  here.
- **Mods declaring layouts.** `state_defs` already works from any block file,
  and `finalize_states` already lays them out per block; a mod loader only has
  to register its blocks before that runs. Anything a mod adds that reads the
  word belongs in this list.
