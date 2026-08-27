# Craftmjne Launcher

Downloads, manages and starts Craftmjne versions. Built as a separate
workspace member with [egui](https://github.com/emilk/egui) rather than Bevy,
so it stays small and starts instantly — **nothing here may depend on the
`craftmjne` crate.**

```bash
cargo run -p craftmjne-launcher
cargo test -p craftmjne-launcher
```

See the README at the repo root ("The launcher") for what it does and why it
replaced the game's in-game self-updater.

## `manifest.json`

This file is what the launcher checks on every start to see whether *it*
needs updating. Only the copy on the **`launcher` branch** is ever read:

```
https://raw.githubusercontent.com/<owner>/<repo>/launcher/launcher/manifest.json
```

```json
{
  "version": "1.0.1",
  "assets": {
    "x86_64-pc-windows-msvc": "https://github.com/.../craftmjne-launcher-x86_64-pc-windows-msvc.zip",
    "x86_64-unknown-linux-gnu": "https://github.com/.../craftmjne-launcher-x86_64-unknown-linux-gnu.tar.gz"
  },
  "notes": "optional, shown next to the update message"
}
```

A platform missing from `assets` means "no update for that platform right
now", which is the correct outcome when one build leg fails — not an error,
and not an offer to download something that can't work.

The copy committed here on other branches is an inert placeholder (its
version matches the current build, so it never triggers an update). Don't
hand-edit the one on the `launcher` branch — `launcher-release.yml` rewrites
it after a successful build, so it can never point at downloads that don't
exist yet.

## Publishing a launcher update

1. Bump `version` in `launcher/Cargo.toml`.
2. Push that to the `launcher` branch.

`.github/workflows/launcher-release.yml` builds every platform, publishes
them under the tag `launcher-v<version>`, and writes the matching manifest
back to the branch. Existing launchers pick it up the next time they start.

This is deliberately independent of game releases: shipping a launcher fix
neither requires nor triggers a game version bump.
