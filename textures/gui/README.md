# GUI textures

Every menu, the hotbar and the inventory are drawn from these images. The game paints
rustic wood, stone and leather versions itself, so this folder can stay empty. To retexture
the interface, drop a PNG here named after an element (or put it in `textures/gui/` inside
an installed mod - mods load after the game, in folder-name order, and a later one wins).

A PNG must be a **whole multiple** of the element's size below, the same on both axes:
`button.png` at 3x is 48x36. The slice border scales with it, so a texture pack can be as
detailed as it likes. Use nearest-neighbour art; the game draws it unfiltered. A file with
the wrong shape or that won't decode is ignored and the built-in texture stays.

| file (`.png`) | size | border | what |
|---|---|---|---|
| panel | 24x24 | 6 | The framed oak board a screen sits on. Centre and edges tile. |
| card, card_alt | 16x16 | 4 | Leather patches: settings body, list entries. |
| button, button_hover, button_pressed, button_selected | 16x12 | 4 | Stone slab button and its states (selected = a chosen tab/choice). |
| button_primary, _hover, _pressed | 16x12 | 4 | The main action (Create, Resume). |
| button_danger, _hover, _pressed | 16x12 | 4 | Quit and other leaving actions. |
| field, field_focus | 16x12 | 4 | Text boxes (focus = being typed in). |
| slot, slot_hover | 14x14 | 3 | Inventory and hotbar slots. |
| plank | 16x4 | 0 | The short plank under a heading (stretched). |
| background | 16x16 | 0 | Tiled behind every menu (tiled, not stretched). |

*border* is how many pixels at each edge keep their size when the element is stretched;
the middle tiles.
