// Desktop smoke test: `npm run smoke` (builds first, works under xvfb-run).
// Boots the real game window, waits for the world to stream in, prints engine
// stats as JSON, saves a screenshot, and exits non-zero on failure.

const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const fs = require('node:fs');
const { registerScheme, attachHandler, APP_URL } = require('./serve.cjs');

// Allow running as root / in containers and on software GL.
app.commandLine.appendSwitch('no-sandbox');
app.commandLine.appendSwitch('enable-unsafe-swiftshader');

const SHOT = process.env.CRAFT_SMOKE_SHOT || path.join(__dirname, '../smoke.png');

registerScheme();

app.whenReady().then(async () => {
  attachHandler();
  const win = new BrowserWindow({
    width: 1280,
    height: 720,
    show: false,
    webPreferences: { contextIsolation: true, sandbox: false },
  });

  const errors = [];
  win.webContents.on('console-message', (_e, level, message) => {
    if (level >= 3) errors.push(message);
  });
  win.webContents.on('render-process-gone', (_e, details) => {
    console.error(JSON.stringify({ ok: false, reason: 'renderer gone', details }));
    app.exit(2);
  });

  await win.loadURL(APP_URL);
  win.show();

  let state = null;
  for (let i = 0; i < 60; i++) {
    await new Promise((r) => setTimeout(r, 500));
    state = await win.webContents.executeJavaScript(`(() => {
      const c = window.craft;
      if (!c || !c.world) return null;
      return {
        stats: c.world.stats(),
        spawned: c.player.spawned,
        pos: { ...c.player.pos },
        draws: c.renderer.info.render.calls,
        tris: c.renderer.info.render.triangles,
      };
    })()`);
    if (state && state.spawned && state.stats.meshed > 100) break;
  }

  const image = await win.webContents.capturePage();
  fs.writeFileSync(SHOT, image.toPNG());

  const ok = !!(state && state.spawned && state.stats.meshed > 100 && state.tris > 0);
  console.log(JSON.stringify({ ok, state, errors, screenshot: SHOT }, null, 2));
  app.exit(ok ? 0 : 1);
});
