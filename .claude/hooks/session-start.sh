#!/bin/bash
set -euo pipefail

# Only relevant on Claude Code on the web / remote sessions - a local dev
# machine already keeps a persistent ~/.cargo cache between runs.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# This is a Bevy project: the dependency graph (wgpu, winit, ~300 crates)
# takes several minutes to compile from a cold cache. Pre-warm it here, in
# the background, so it's ready by the time work actually starts instead of
# stalling the first `cargo check`/`cargo test`/`cargo build` of the session.
echo '{"async": true, "asyncTimeout": 600000}'

cd "$CLAUDE_PROJECT_DIR"
cargo fetch
cargo check --tests
