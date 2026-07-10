# Craftmjne — read this before doing anything else

This is a **Rust + Bevy native game** (see `Cargo.toml`, `src/*.rs`). It is a
complete, working, well-optimized voxel engine framework with procedurally
generated 16x16 textures, chunked async terrain generation/meshing, physics,
a main menu with per-user saves, a Windows installer, and a self-updater.
Full details: `README.md`.

## Do not rewrite this project

A past session mistakenly assumed the repo was empty (it was working from a
stale local checkout that predated this project) and started building a
parallel implementation in JavaScript/Three.js from scratch. **Do not repeat
that mistake.** Specifically:

- **Never start a rewrite in another language or framework** (JS, Electron,
  Unity, Godot, etc.) unless the user explicitly asks for a full rewrite and
  confirms they understand the existing Rust/Bevy game will be replaced.
- **Never assume the repo is empty or minimal** based on `ls` or an old local
  clone. Before concluding there's little/nothing to build on, run
  `git fetch origin main && git log origin/main` and compare against your
  local `HEAD` — local checkouts in this environment can be stale relative
  to GitHub.
- If asked to "build a Minecraft clone", "make it a framework", "optimize
  it", "add 16x16 textures", etc. — that almost certainly means **extend
  this existing Rust/Bevy project**, not start over. Read `README.md`'s
  "Extending the framework" section and add a Bevy plugin.
- If you genuinely believe a rewrite is warranted (e.g. the user wants a
  browser-playable version alongside the native one), say so explicitly and
  get clear confirmation before writing any code — this is a decision only
  the user should make, not something to infer from an ambiguous request.

## Quick orientation

- `cargo run --release` to play; `cargo test` to run the test suite.
- `src/` is organized as Bevy plugins — one file per plugin/subsystem
  (`world.rs`, `player.rs`, `terrain.rs`, `mesher.rs`, `atlas.rs`, etc.).
  `main.rs` just assembles them.
- Building the Windows installer: see README's "Building the Windows
  installer yourself" section (`rustup target add x86_64-pc-windows-gnu`,
  cross-compile, then `makensis` against `installer/craftmjne.nsi`).
- Releases are cut by tagging `vX.Y.Z` (matching `Cargo.toml`'s `version`)
  and pushing the tag; `.github/workflows/release.yml` builds and publishes
  binaries + the installer automatically.
